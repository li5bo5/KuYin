use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::audio::{DeviceManager, DeviceWatchEvent, EngineEvent, WindowsPlaybackEngine};
use crate::kugou::KugouClient;
use crate::platform::smtc::{SmtcAction, SmtcManager};
use crate::storage::{log_error, log_info, log_warn, Database};

use super::model::{PlaybackMode, PlayerSnapshot, Track};
use super::queue::PlaybackQueue;

struct ActiveTrackRuntime {
    track: Option<Track>,
    active_url: Option<String>,
    backup_urls: VecDeque<String>,
    reauth_attempted: bool,
}

type TrayCoverCache = Option<(String, Vec<u8>)>;

pub struct PlaybackCoordinator {
    engine: Arc<WindowsPlaybackEngine>,
    kugou: Arc<KugouClient>,
    db: Arc<Database>,
    queue: Arc<Mutex<PlaybackQueue>>,
    runtime: Arc<Mutex<ActiveTrackRuntime>>,
    switch_cancel: Arc<Mutex<Option<CancellationToken>>>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
    consecutive_failures: AtomicU32,
    has_active_source: AtomicBool,
    pending_resume_ms: AtomicU64,
    last_known_pos_ms: AtomicU64,
    current_quality: Arc<Mutex<String>>,
    watched_device_id: Arc<Mutex<Option<String>>>,
    watched_device_online: AtomicBool,
    tray_cover_bgra: Arc<Mutex<TrayCoverCache>>,
}

impl PlaybackCoordinator {
    pub fn new(
        engine: Arc<WindowsPlaybackEngine>,
        kugou: Arc<KugouClient>,
        db: Arc<Database>,
    ) -> Self {
        let quality = db
            .get_setting("audio_quality")
            .unwrap_or_else(|| "320".to_string());
        let initial_dev = db
            .get_setting("audio_device_id")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Self {
            engine,
            kugou,
            db,
            queue: Arc::new(Mutex::new(PlaybackQueue::new())),
            runtime: Arc::new(Mutex::new(ActiveTrackRuntime {
                track: None,
                active_url: None,
                backup_urls: VecDeque::new(),
                reauth_attempted: false,
            })),
            switch_cancel: Arc::new(Mutex::new(None)),
            app_handle: Arc::new(Mutex::new(None)),
            consecutive_failures: AtomicU32::new(0),
            has_active_source: AtomicBool::new(false),
            pending_resume_ms: AtomicU64::new(0),
            last_known_pos_ms: AtomicU64::new(0),
            current_quality: Arc::new(Mutex::new(quality)),
            watched_device_id: Arc::new(Mutex::new(initial_dev)),
            watched_device_online: AtomicBool::new(true),
            tray_cover_bgra: Arc::new(Mutex::new(None)),
        }
    }

    pub fn raw_engine(&self) -> &WindowsPlaybackEngine {
        &self.engine
    }

    /// 从 SQLite 恢复上次退出时的播放队列、曲目、进度、音量与模式（按需延迟换链，启动零流量浪费）
    pub async fn restore_from_db(&self) {
        let Some(saved) = self.db.load_playback_snapshot() else {
            return;
        };

        let _ = self.engine.set_volume(saved.volume).await;
        let mode = PlaybackMode::from_db_str(&saved.mode);
        let tracks: Vec<Track> = serde_json::from_str(&saved.queue_json).unwrap_or_default();

        if !tracks.is_empty() {
            let start_idx = saved
                .current_track_hash
                .as_ref()
                .and_then(|h| tracks.iter().position(|t| t.hash.eq_ignore_ascii_case(h)))
                .or(Some(0));

            let mut q = self.queue.lock().await;
            q.set_mode(mode);
            q.set_queue(tracks, start_idx);
            if let Some(curr) = q.get_current_track() {
                #[cfg(windows)]
                {
                    let _ = SmtcManager::update_metadata(
                        self.engine.raw_player(),
                        &curr.name,
                        &curr.singer,
                        curr.cover_url.as_deref(),
                    );
                }
                if let Some(ref url) = curr.cover_url {
                    if let Some(bgra) = self.kugou.fetch_cover_bgra_thumb(url, 40).await {
                        let mut cover_guard = self.tray_cover_bgra.lock().await;
                        *cover_guard = Some((curr.hash.clone(), bgra));
                    }
                }
            }
        } else {
            let mut q = self.queue.lock().await;
            q.set_mode(mode);
        }

        self.pending_resume_ms
            .store(saved.position_ms, Ordering::SeqCst);
        self.last_known_pos_ms
            .store(saved.position_ms, Ordering::SeqCst);
        log_info(&format!(
            "已从 SQLite 恢复播放断点状态 (进度: {}ms, 模式: {})",
            saved.position_ms,
            mode.as_str()
        ));
    }

    /// 启动独立于 WebView2 的后台事件守护协程与低频（6秒）落盘节流器
    pub async fn attach_background_services(self: &Arc<Self>, app: AppHandle) {
        {
            let mut guard = self.app_handle.lock().await;
            *guard = Some(app);
        }

        // 启动时若用户已配置指定音频设备，尝试绑定并同步在线状态
        let init_dev = self.watched_device_id.lock().await.clone();
        if let Some(ref dev_id) = init_dev {
            let online = DeviceManager::is_device_online(dev_id).await;
            self.watched_device_online.store(online, Ordering::SeqCst);
            #[cfg(windows)]
            if online {
                let _ = DeviceManager::set_audio_device(
                    self.engine.raw_player(),
                    Some(dev_id.clone()),
                )
                .await;
            }
        }

        let (engine_tx, mut engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        let _ = self.engine.bind_events(engine_tx);

        let (smtc_tx, mut smtc_rx) = mpsc::unbounded_channel::<SmtcAction>();
        #[cfg(windows)]
        {
            let _ = SmtcManager::setup_smtc(self.engine.raw_player(), smtc_tx);
        }
        #[cfg(not(windows))]
        {
            let _ = smtc_tx;
        }

        let (dev_tx, mut dev_rx) = mpsc::unbounded_channel::<DeviceWatchEvent>();
        DeviceManager::start_device_watcher(dev_tx);

        // 守护协程 1：监听内核 MediaEnded / MediaFailed、硬件 SMTC 按键及指定音频设备断开/重连
        let coord_events = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(ev) = engine_rx.recv() => {
                        match ev {
                            EngineEvent::MediaEnded => {
                                coord_events.handle_media_ended().await;
                            }
                            EngineEvent::MediaFailed(reason) => {
                                coord_events.handle_media_failed(reason).await;
                            }
                        }
                    }
                    Some(action) = smtc_rx.recv() => {
                        match action {
                            SmtcAction::Play => { let _ = coord_events.resume_or_play().await; }
                            SmtcAction::Pause => { let _ = coord_events.pause().await; }
                            SmtcAction::Next => { let _ = coord_events.next().await; }
                            SmtcAction::Previous => { let _ = coord_events.previous().await; }
                        }
                    }
                    Some(first_ev) = dev_rx.recv() => {
                        let mut events = vec![first_ev];
                        tokio::time::sleep(Duration::from_millis(80)).await;
                        while let Ok(ev) = dev_rx.try_recv() {
                            events.push(ev);
                        }
                        coord_events.handle_device_change(&events).await;
                    }
                    else => break,
                }
            }
        });

        // 守护协程 2：低频节流持久化（每 6 秒仅在播放中同步一次进度到 SQLite，严格遵守 ≥ 5s 红线）
        let coord_persist = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(6));
            loop {
                ticker.tick().await;
                let progress = coord_persist.engine.get_progress().await;
                if progress.is_playing {
                    coord_persist
                        .last_known_pos_ms
                        .store(progress.position_ms, Ordering::SeqCst);
                    coord_persist.persist_snapshot_now().await;
                }
            }
        });
    }

    /// 更新当前绑定的指定输出设备（非空时自动开启断开暂停/重连恢复；为空即跟随系统默认时自动关闭）
    pub async fn update_watched_device(&self, device_id: Option<String>) {
        let norm = device_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(ref id) = norm {
            let online = DeviceManager::is_device_online(id).await;
            self.watched_device_online.store(online, Ordering::SeqCst);
        }
        let mut guard = self.watched_device_id.lock().await;
        *guard = norm;
    }

    async fn handle_device_change(&self, events: &[DeviceWatchEvent]) {
        let target_opt = self.watched_device_id.lock().await.clone();
        let Some(target_id) = target_opt else {
            return;
        };

        let removed_explicit = events.iter().any(|ev| match ev {
            DeviceWatchEvent::Removed(id) => id.eq_ignore_ascii_case(&target_id),
            _ => false,
        });
        let now_online = if removed_explicit {
            false
        } else {
            DeviceManager::is_device_online(&target_id).await
        };
        let was_online = self.watched_device_online.swap(now_online, Ordering::SeqCst);

        if was_online && !now_online {
            log_info(&format!(
                "检测到指定音频输出设备已断开 ({target_id})，立即暂停播放并释放旧设备拓扑"
            ));
            self.invalidate_device_topology_and_pause().await;
        } else if !was_online && now_online {
            log_info(&format!(
                "检测到指定音频输出设备已重新连接 ({target_id})，等待 200ms 后重建音频拓扑并恢复播放"
            ));
            tokio::time::sleep(Duration::from_millis(200)).await;
            #[cfg(windows)]
            {
                let _ = DeviceManager::set_audio_device(
                    self.engine.raw_player(),
                    Some(target_id),
                )
                .await;
            }
            let _ = self.rebind_stream_to_current_device(true).await;
        }
    }

    /// 设备断开时保存当前毫秒进度、暂停播放并标记旧 WASAPI 拓扑失效
    async fn invalidate_device_topology_and_pause(&self) {
        let progress = self.engine.get_progress().await;
        let pos_ms = if self.has_active_source.load(Ordering::SeqCst) && progress.position_ms > 0 {
            progress.position_ms
        } else {
            self.last_known_pos_ms.load(Ordering::SeqCst)
        };
        self.last_known_pos_ms.store(pos_ms, Ordering::SeqCst);
        self.pending_resume_ms.store(pos_ms, Ordering::SeqCst);
        let _ = self.engine.pause().await;
        self.has_active_source.store(false, Ordering::SeqCst);
        #[cfg(windows)]
        SmtcManager::update_playback_status(self.engine.raw_player(), false);
        self.persist_snapshot_now().await;
    }

    /// 通过重新调用 SetSource (load_uri_and_verify) 强制 Media Foundation 在新输出设备上重建音频渲染拓扑，并恢复至原毫秒进度
    async fn rebind_stream_to_current_device(&self, should_play: bool) -> Result<(), String> {
        let current = {
            let q = self.queue.lock().await;
            q.get_current_track()
        };
        let Some(track) = current else {
            return Ok(());
        };

        let resume_ms = if self.has_active_source.load(Ordering::SeqCst) {
            let p = self.engine.get_progress().await.position_ms;
            if p > 0 {
                p
            } else {
                self.last_known_pos_ms.load(Ordering::SeqCst)
            }
        } else {
            let p = self.last_known_pos_ms.load(Ordering::SeqCst);
            if p > 0 {
                p
            } else {
                self.pending_resume_ms.load(Ordering::SeqCst)
            }
        };

        let cached_url = {
            let rt = self.runtime.lock().await;
            if rt
                .track
                .as_ref()
                .map(|t| t.hash.eq_ignore_ascii_case(&track.hash))
                .unwrap_or(false)
            {
                rt.active_url.clone()
            } else {
                None
            }
        };

        if let Some(url) = cached_url {
            if self
                .engine
                .load_uri_and_verify(&url, should_play, None)
                .await
                .is_ok()
            {
                if resume_ms > 0 || !should_play {
                    let _ = self.engine.seek(resume_ms).await;
                }
                let target_vol = self.engine.get_user_volume().await;
                let _ = self.engine.set_volume(target_vol).await;
                self.has_active_source.store(true, Ordering::SeqCst);
                self.pending_resume_ms.store(0, Ordering::SeqCst);
                self.last_known_pos_ms.store(resume_ms, Ordering::SeqCst);
                #[cfg(windows)]
                SmtcManager::update_playback_status(self.engine.raw_player(), should_play);
                log_info(&format!(
                    "已在新音频输出设备上重建 MediaSource 拓扑并恢复至 {resume_ms}ms"
                ));
                return Ok(());
            }
        }

        self.load_and_play_loop(track, resume_ms, should_play).await
    }

    /// 用户在设置页手动切换输出设备后，若当前已有打开的音频源，立即在新设备上重建拓扑以实时切换声音输出
    pub async fn rebind_stream_after_device_switch(&self) {
        if self.has_active_source.load(Ordering::SeqCst) {
            let is_playing = self.engine.get_progress().await.is_playing;
            let _ = self.rebind_stream_to_current_device(is_playing).await;
        }
    }

    async fn emit_toast(&self, message: &str) {
        let guard = self.app_handle.lock().await;
        if let Some(ref app) = *guard {
            let _ = app.emit("player-toast", message.to_string());
        }
    }

    async fn next_switch_token(&self) -> CancellationToken {
        let mut guard = self.switch_cancel.lock().await;
        if let Some(prev) = guard.take() {
            prev.cancel();
        }
        let token = CancellationToken::new();
        *guard = Some(token.clone());
        token
    }

    /// 持久化当前快照至 SQLite
    pub async fn persist_snapshot_now(&self) {
        let progress = self.engine.get_progress().await;
        let pos_ms = if self.has_active_source.load(Ordering::SeqCst) {
            progress.position_ms
        } else {
            self.pending_resume_ms.load(Ordering::SeqCst)
        };

        let (track_hash, mode_str, queue_json) = {
            let q = self.queue.lock().await;
            let hash = q.get_current_track().map(|t| t.hash);
            let m = q.get_mode().as_str().to_string();
            let q_json = serde_json::to_string(q.get_tracks()).unwrap_or_else(|_| "[]".to_string());
            (hash, m, q_json)
        };

        let _ = self.db.save_playback_snapshot(
            track_hash.as_deref(),
            pos_ms,
            progress.volume,
            &mode_str,
            &queue_json,
        );
    }

    /// 恢复播放或首次按需换链续播
    pub async fn resume_or_play(&self) -> Result<(), String> {
        if self.has_active_source.load(Ordering::SeqCst) {
            self.engine.play().await?;
            #[cfg(windows)]
            SmtcManager::update_playback_status(self.engine.raw_player(), true);
            return Ok(());
        }

        let current = {
            let q = self.queue.lock().await;
            q.get_current_track()
        };

        if let Some(track) = current {
            let resume_ms = self.pending_resume_ms.swap(0, Ordering::SeqCst);
            self.load_and_play_loop(track, resume_ms, true).await
        } else {
            Ok(())
        }
    }

    pub async fn pause(&self) -> Result<(), String> {
        let progress = self.engine.get_progress().await;
        self.last_known_pos_ms
            .store(progress.position_ms, Ordering::SeqCst);
        self.engine.pause().await?;
        #[cfg(windows)]
        SmtcManager::update_playback_status(self.engine.raw_player(), false);
        self.persist_snapshot_now().await;
        Ok(())
    }

    pub async fn seek(&self, position_ms: u64) -> Result<(), String> {
        if !self.has_active_source.load(Ordering::SeqCst) {
            self.pending_resume_ms.store(position_ms, Ordering::SeqCst);
            self.last_known_pos_ms.store(position_ms, Ordering::SeqCst);
            return Ok(());
        }
        self.engine.seek(position_ms).await?;
        self.last_known_pos_ms.store(position_ms, Ordering::SeqCst);
        Ok(())
    }

    pub async fn set_volume(&self, volume: f64) -> Result<(), String> {
        let prev_vol = self.engine.get_user_volume().await;
        let clamped = volume.clamp(0.0, 1.0);
        self.engine.set_volume(clamped).await?;
        if clamped <= 0.0001 {
            let _ = self.pause().await;
        } else if prev_vol <= 0.0001 {
            let _ = self.resume_or_play().await;
        }
        Ok(())
    }

    pub async fn set_quality(&self, quality: &str) -> Result<(), String> {
        let normalized = match quality {
            "128" | "320" | "flac" => quality.to_string(),
            _ => "320".to_string(),
        };
        let prev = {
            let mut q = self.current_quality.lock().await;
            let old = q.clone();
            *q = normalized.clone();
            old
        };
        self.db.set_setting("audio_quality", &normalized)?;

        // 若当前已有激活音频源且切换了新音质，立即以新音质重新换链并无缝 Seek 到原进度
        if prev != normalized && self.has_active_source.load(Ordering::SeqCst) {
            let current_opt = {
                let q = self.queue.lock().await;
                q.get_current_track()
            };
            if let Some(track) = current_opt {
                let progress = self.engine.get_progress().await;
                self.load_and_play_internal(track, progress.position_ms, progress.is_playing)
                    .await?;
            }
        }
        Ok(())
    }

    /// 播放单曲（显式触发播放动作：若不在队列则插入队列并立即开始播放）
    pub async fn play_track(&self, track: Track) -> Result<(), String> {
        {
            let mut q = self.queue.lock().await;
            q.select_or_insert(track.clone());
        }
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.load_and_play_loop(track, 0, true).await
    }

    /// 按队列索引跳转（显式触发播放动作：立即开始播放选中曲目）
    pub async fn play_at_index(&self, index: usize) -> Result<(), String> {
        let target = {
            let mut q = self.queue.lock().await;
            q.set_current_index(Some(index));
            q.get_current_track()
        };
        if let Some(track) = target {
            self.consecutive_failures.store(0, Ordering::SeqCst);
            self.load_and_play_loop(track, 0, true).await
        } else {
            Err("无效的队列索引".to_string())
        }
    }

    fn build_quality_rotation(preferred: &str) -> [&'static str; 3] {
        match preferred {
            "flac" => ["flac", "320", "128"],
            "320" => ["320", "128", "flac"],
            _ => ["128", "320", "flac"],
        }
    }

    async fn load_and_play_loop(
        &self,
        initial_track: Track,
        initial_seek_ms: u64,
        keep_playing: bool,
    ) -> Result<(), String> {
        self.load_and_play_internal(initial_track, initial_seek_ms, keep_playing)
            .await
    }

    /// 核心非递归切歌、三级音质 × 各重试 2 次自动轮转与连续 5 首熔断循环（需求 #11, #12 + 架构红线六.1 ~ 六.3）
    async fn load_and_play_internal(
        &self,
        initial_track: Track,
        initial_seek_ms: u64,
        keep_playing: bool,
    ) -> Result<(), String> {
        let cancel_token = self.next_switch_token().await;
        let mut candidate = initial_track;
        let mut seek_after_load = initial_seek_ms;
        let should_play = keep_playing;

        loop {
            if cancel_token.is_cancelled() {
                return Ok(());
            }

            let preferred_quality = self.current_quality.lock().await.clone();
            let quality_chain = Self::build_quality_rotation(&preferred_quality);

            log_info(&format!(
                "按需实时换链曲目: {} - {} ({}, 首选音质: {})",
                candidate.name, candidate.singer, candidate.hash, preferred_quality
            ));

            // 先查询 `/v2/get_res_privilege/lite` 提取 128K MP3 / 320K MP3 / FLAC 各自独立的音源 hash
            let priv_hashes = self
                .kugou
                .get_privilege_hash_map(
                    &candidate.hash,
                    candidate.album_id.as_deref(),
                    Some(cancel_token.clone()),
                )
                .await;

            if cancel_token.is_cancelled() {
                return Ok(());
            }

            let target_vol = self.engine.get_user_volume().await;

            if !should_play {
                self.engine.ramp_raw_volume(0.0, 0.0, 10).await;
            } else if self.has_active_source.load(Ordering::SeqCst) {
                self.engine.ramp_raw_volume(target_vol, 0.0, 160).await;
            }

            let mut loaded = false;
            let mut active_stream_url = None;
            let mut remaining_backups = VecDeque::new();

            // 外层遍历全部 3 种歌曲品质；内层每个品质执行「初次尝试 + 失败重试 2 次」
            'quality_loop: for (q_idx, &q) in quality_chain.iter().enumerate() {
                if cancel_token.is_cancelled() {
                    let _ = self.engine.set_volume(target_vol).await;
                    return Ok(());
                }

                if q_idx > 0 {
                    log_warn(&format!(
                        "曲目《{}》前一音质重试未果，自动切换至品质 {} 继续尝试",
                        candidate.name, q
                    ));
                }

                let target_hash = priv_hashes
                    .get(q)
                    .cloned()
                    .unwrap_or_else(|| candidate.hash.clone());

                for retry in 0..=2usize {
                    if cancel_token.is_cancelled() {
                        let _ = self.engine.set_volume(target_vol).await;
                        return Ok(());
                    }

                    if retry > 0 {
                        log_warn(&format!(
                            "曲目《{}》品质 {} 播放失败，正在进行第 {retry}/2 次重试...",
                            candidate.name, q
                        ));
                        if retry == 2 {
                            self.kugou.set_verify_auth("").await;
                        }
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                            _ = cancel_token.cancelled() => {
                                let _ = self.engine.set_volume(target_vol).await;
                                return Ok(());
                            }
                        }
                    }

                    let urls_res = self
                        .kugou
                        .get_song_urls_for_quality(
                            &target_hash,
                            candidate.album_id.as_deref(),
                            candidate.album_audio_id,
                            q,
                            Some(cancel_token.clone()),
                        )
                        .await;

                    if cancel_token.is_cancelled() {
                        let _ = self.engine.set_volume(target_vol).await;
                        return Ok(());
                    }

                    if let Some(alert) = self.kugou.take_gateway_alert().await {
                        self.emit_toast(&alert).await;
                    }

                    if let Ok(url_list) = urls_res {
                        let mut deque: VecDeque<String> = url_list.into_iter().collect();
                        while let Some(stream_url) = deque.pop_front() {
                            if self
                                .engine
                                .load_uri_and_verify(&stream_url, should_play, Some(&cancel_token))
                                .await
                                .is_ok()
                            {
                                loaded = true;
                                active_stream_url = Some(stream_url);
                                remaining_backups = deque;
                                break 'quality_loop;
                            }
                            if cancel_token.is_cancelled() {
                                let _ = self.engine.set_volume(target_vol).await;
                                return Ok(());
                            }
                        }
                    }
                }
            }

            if loaded {
                if seek_after_load > 0 || !should_play {
                    let _ = self.engine.seek(seek_after_load).await;
                }

                if should_play {
                    self.engine.ramp_raw_volume(0.0, target_vol, 190).await;
                } else {
                    let _ = self.engine.set_volume(target_vol).await;
                }

                self.has_active_source.store(true, Ordering::SeqCst);
                self.consecutive_failures.store(0, Ordering::SeqCst);
                self.pending_resume_ms.store(0, Ordering::SeqCst);
                self.last_known_pos_ms
                    .store(seek_after_load, Ordering::SeqCst);

                {
                    let mut rt = self.runtime.lock().await;
                    rt.track = Some(candidate.clone());
                    rt.active_url = active_stream_url;
                    rt.backup_urls = remaining_backups;
                    rt.reauth_attempted = false;
                }

                #[cfg(windows)]
                {
                    let _ = SmtcManager::update_metadata(
                        self.engine.raw_player(),
                        &candidate.name,
                        &candidate.singer,
                        candidate.cover_url.as_deref(),
                    );
                    SmtcManager::update_playback_status(self.engine.raw_player(), should_play);
                }

                // 异步写入历史、补齐缺失封面、缓存托盘 40x40 缩略图、上报云端听歌记录与低频快照
                let db = Arc::clone(&self.db);
                let kugou_meta = Arc::clone(&self.kugou);
                let queue_meta = Arc::clone(&self.queue);
                let engine_meta = Arc::clone(&self.engine);
                let tray_cover_meta = Arc::clone(&self.tray_cover_bgra);
                let hist_track = candidate.clone();
                tokio::spawn(async move {
                    let mut final_cover = hist_track.cover_url.clone();
                    if final_cover.as_deref().unwrap_or("").is_empty() {
                        if let Ok(Some(fetched_cover)) =
                            kugou_meta.get_audio_metadata(&hist_track.hash).await
                        {
                            {
                                let mut q = queue_meta.lock().await;
                                q.update_track_cover(&hist_track.hash, &fetched_cover);
                            }
                            #[cfg(windows)]
                            {
                                let _ = SmtcManager::update_metadata(
                                    engine_meta.raw_player(),
                                    &hist_track.name,
                                    &hist_track.singer,
                                    Some(fetched_cover.as_str()),
                                );
                            }
                            final_cover = Some(fetched_cover);
                        }
                    }

                    if let Some(ref url) = final_cover {
                        if !url.is_empty() {
                            if let Some(bgra) = kugou_meta.fetch_cover_bgra_thumb(url, 40).await {
                                let mut cg = tray_cover_meta.lock().await;
                                *cg = Some((hist_track.hash.clone(), bgra));
                            }
                        }
                    } else {
                        let mut cg = tray_cover_meta.lock().await;
                        *cg = None;
                    }

                    let _ = db.record_history(
                        &hist_track.hash,
                        &hist_track.name,
                        &hist_track.singer,
                        hist_track.album_id.as_deref(),
                        hist_track.album_audio_id,
                        hist_track.duration,
                        final_cover.as_deref(),
                    );

                    if let Some(mxid) = hist_track.album_audio_id {
                        if mxid > 0 {
                            let _ = kugou_meta.upload_play_history(mxid).await;
                        }
                    }
                });
                self.persist_snapshot_now().await;
                return Ok(());
            }

            // 全部 3 种品质均重试 2 次后仍不能播放：递增连续失败计数并检查 5 次熔断红线
            let _ = self.engine.set_volume(target_vol).await;
            let fails = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
            if fails >= 5 {
                let msg = "已连续 5 首歌曲全部品质重试均失败，已触发熔断停止自动切歌";
                log_error(msg);
                self.emit_toast(msg).await;
                let _ = self.engine.pause().await;
                #[cfg(windows)]
                SmtcManager::update_playback_status(self.engine.raw_player(), false);
                return Err(msg.to_string());
            }

            self.emit_toast(&format!(
                "《{}》各音质重试均不可播，已自动跳过 ({fails}/5)",
                candidate.name
            ))
            .await;

            // 根据当前播放规则取下一首继续循环尝试（无异步递归）
            let next_opt = {
                let mut q = self.queue.lock().await;
                q.next_on_skip()
            };

            match next_opt {
                Some(next_t) if next_t.hash != candidate.hash => {
                    candidate = next_t;
                    seek_after_load = 0;
                }
                _ => {
                    let _ = self.engine.pause().await;
                    return Err("当前曲目无法播放且队列无更多可切换歌曲".to_string());
                }
            }
        }
    }

    async fn handle_media_ended(&self) {
        let next_track = {
            let mut q = self.queue.lock().await;
            q.next_on_ended()
        };

        if let Some(track) = next_track {
            let _ = self.load_and_play_loop(track, 0, true).await;
        } else {
            log_info("顺序播放队列已到达末尾");
            let _ = self.engine.pause().await;
            #[cfg(windows)]
            SmtcManager::update_playback_status(self.engine.raw_player(), false);
            self.persist_snapshot_now().await;
        }
    }

    /// 架构红线（六.3）：主链失败切 backup_url + Seek；403 签名过期自动重签换链 + Seek 断点续播
    async fn handle_media_failed(&self, reason: String) {
        // 若用户指定了音频输出设备，先检查该设备是否已断开/离线；若是硬件断开导致的流中断，立即暂停并释放失效拓扑等待重连
        if let Some(target_id) = self.watched_device_id.lock().await.clone() {
            if !self.watched_device_online.load(Ordering::SeqCst)
                || !DeviceManager::is_device_online(&target_id).await
            {
                self.watched_device_online.store(false, Ordering::SeqCst);
                log_info(&format!(
                    "媒体流中断源于指定输出设备已断开 ({target_id})，立即暂停播放并等待重连"
                ));
                self.invalidate_device_topology_and_pause().await;
                return;
            }
        }

        let resume_ms = self.last_known_pos_ms.load(Ordering::SeqCst);
        log_warn(&format!(
            "检测到媒体流中断 ({reason})，尝试在 {resume_ms}ms 处执行无缝容灾恢复..."
        ));

        let (backup_opt, need_reauth_track) = {
            let mut rt = self.runtime.lock().await;
            if let Some(backup) = rt.backup_urls.pop_front() {
                (Some(backup), None)
            } else if !rt.reauth_attempted {
                rt.reauth_attempted = true;
                (None, rt.track.clone())
            } else {
                (None, None)
            }
        };

        // 第 1 级容灾：无缝切换至备用 CDN 地址并定位到中断进度
        if let Some(backup_url) = backup_opt {
            if self
                .engine
                .load_uri_and_verify(&backup_url, true, None)
                .await
                .is_ok()
            {
                if resume_ms > 0 {
                    let _ = self.engine.seek(resume_ms).await;
                }
                {
                    let mut rt = self.runtime.lock().await;
                    rt.active_url = Some(backup_url);
                }
                log_info("已无缝切换至备用 backup_url 恢复播放");
                return;
            }
        }

        // 第 2 级容灾：暂停过久 URL 签名过期（HTTP 403）或流中断，清空短期授权并进入三级音质重试/断点恢复
        if let Some(track) = need_reauth_track {
            self.kugou.set_verify_auth("").await;
            if self.load_and_play_loop(track, resume_ms, true).await.is_ok() {
                log_info("已通过重新签名换链从断点恢复播放");
            }
            return;
        }

        // 第 3 级容灾：该曲目彻底不可用，计入连续失败并按播放规则跳到下一首（满 5 次熔断）
        let fails = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if fails >= 5 {
            let msg = "连续 5 首歌曲播放失败，已触发熔断停止自动切歌";
            log_error(msg);
            self.emit_toast(msg).await;
            let _ = self.engine.pause().await;
            return;
        }

        let next_opt = {
            let mut q = self.queue.lock().await;
            q.next_on_skip()
        };
        if let Some(next_t) = next_opt {
            let _ = self.load_and_play_loop(next_t, 0, true).await;
        }
    }

    pub async fn next(&self) -> Result<(), String> {
        let should_play = self.engine.get_progress().await.is_playing;
        let next_track = {
            let mut q = self.queue.lock().await;
            q.next_manual()
        };

        if let Some(track) = next_track {
            self.load_and_play_loop(track, 0, should_play).await
        } else {
            self.pause().await
        }
    }

    pub async fn previous(&self) -> Result<(), String> {
        let should_play = self.engine.get_progress().await.is_playing;
        let prev_track = {
            let mut q = self.queue.lock().await;
            q.previous_track()
        };

        if let Some(track) = prev_track {
            self.load_and_play_loop(track, 0, should_play).await
        } else {
            Ok(())
        }
    }

    pub async fn set_mode(&self, mode: PlaybackMode) {
        {
            let mut q = self.queue.lock().await;
            q.set_mode(mode);
        }
        self.persist_snapshot_now().await;
    }

    pub async fn set_queue(
        &self,
        tracks: Vec<Track>,
        start_index: Option<usize>,
    ) -> Result<(), String> {
        let first_to_play = {
            let mut q = self.queue.lock().await;
            q.set_queue(tracks, start_index);
            q.get_current_track()
        };

        self.consecutive_failures.store(0, Ordering::SeqCst);
        if let Some(track) = first_to_play {
            self.load_and_play_loop(track, 0, true).await?;
        }
        Ok(())
    }

    pub async fn insert_next_in_queue(&self, track: Track) {
        {
            let mut q = self.queue.lock().await;
            q.insert_next(track);
        }
        self.persist_snapshot_now().await;
    }

    pub async fn append_one_to_queue(&self, track: Track) {
        {
            let mut q = self.queue.lock().await;
            q.append_one(track);
        }
        self.persist_snapshot_now().await;
    }

    pub async fn append_tracks_to_queue(&self, tracks: Vec<Track>) -> usize {
        let added = {
            let mut q = self.queue.lock().await;
            q.append_tracks(tracks)
        };
        self.persist_snapshot_now().await;
        added
    }

    pub async fn remove_from_queue(&self, index: usize) {
        {
            let mut q = self.queue.lock().await;
            q.remove_at(index);
        }
        self.persist_snapshot_now().await;
    }

    pub async fn move_in_queue(&self, from: usize, to: usize) {
        {
            let mut q = self.queue.lock().await;
            q.move_item(from, to);
        }
        self.persist_snapshot_now().await;
    }

    pub async fn clear_queue(&self) -> Result<(), String> {
        {
            let mut q = self.queue.lock().await;
            q.clear();
        }
        let _ = self.engine.pause().await;
        self.has_active_source.store(false, Ordering::SeqCst);
        self.persist_snapshot_now().await;
        Ok(())
    }

    pub async fn get_queue_tracks(&self) -> Vec<Track> {
        let q = self.queue.lock().await;
        q.get_tracks().to_vec()
    }

    /// 获取当前曲目的 40x40 托盘封面 BGRA 像素缓存（若尚未解码则实时解码并缓存）
    pub async fn get_tray_cover_bgra(&self) -> Option<Vec<u8>> {
        let current = {
            let q = self.queue.lock().await;
            q.get_current_track()
        }?;

        {
            let guard = self.tray_cover_bgra.lock().await;
            if let Some((ref cached_hash, ref bytes)) = *guard {
                if cached_hash.eq_ignore_ascii_case(&current.hash) {
                    return Some(bytes.clone());
                }
            }
        }

        let cover_url = current.cover_url.as_deref()?;
        if cover_url.is_empty() {
            return None;
        }
        let bgra = self.kugou.fetch_cover_bgra_thumb(cover_url, 40).await?;
        {
            let mut guard = self.tray_cover_bgra.lock().await;
            *guard = Some((current.hash, bgra.clone()));
        }
        Some(bgra)
    }

    /// 生成同步给无状态前端视图的完整状态快照
    pub async fn get_snapshot(&self) -> PlayerSnapshot {
        let progress = self.engine.get_progress().await;
        if progress.is_playing {
            self.last_known_pos_ms
                .store(progress.position_ms, Ordering::SeqCst);
        }

        let (current_track, mode, queue_count, current_index) = {
            let q = self.queue.lock().await;
            (
                q.get_current_track(),
                q.get_mode(),
                q.len(),
                q.get_current_index(),
            )
        };

        let pos_ms = if self.has_active_source.load(Ordering::SeqCst) {
            progress.position_ms
        } else {
            self.pending_resume_ms.load(Ordering::SeqCst)
        };

        let dur_ms = if progress.duration_ms > 0 {
            progress.duration_ms
        } else {
            current_track
                .as_ref()
                .map(|t| (t.duration as u64) * 1000)
                .unwrap_or(0)
        };

        let quality = self.current_quality.lock().await.clone();

        PlayerSnapshot {
            current_track,
            is_playing: progress.is_playing,
            is_buffering: progress.is_buffering,
            position_ms: pos_ms,
            duration_ms: dur_ms,
            volume: progress.volume,
            mode,
            quality,
            queue_count,
            current_index,
        }
    }
}
