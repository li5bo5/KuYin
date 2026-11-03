use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
use windows::core::{IInspectable, HSTRING};
#[cfg(windows)]
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
#[cfg(windows)]
use windows::Media::Core::MediaSource;
#[cfg(windows)]
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer, MediaPlayerFailedEventArgs};

#[derive(Clone, Debug)]
pub enum EngineEvent {
    MediaEnded,
    MediaFailed(String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlaybackProgress {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub volume: f64,
}

pub struct WindowsPlaybackEngine {
    #[cfg(windows)]
    player: MediaPlayer,
    current_volume: Arc<Mutex<f64>>,
    is_fading: Arc<AtomicBool>,
    is_loading_source: Arc<AtomicBool>,
    load_failed_msg: Arc<StdMutex<Option<String>>>,
}

struct LoadingFlagGuard<'a>(&'a AtomicBool);

impl Drop for LoadingFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl WindowsPlaybackEngine {
    pub fn new() -> Result<Self, String> {
        #[cfg(windows)]
        {
            let player =
                MediaPlayer::new().map_err(|e| format!("创建原生 MediaPlayer 失败: {e:?}"))?;
            // 禁用内置 CommandManager 自动覆盖，以便由 SmtcManager 完全接管上一首/下一首状态
            if let Ok(cmd_mgr) = player.CommandManager() {
                let _ = cmd_mgr.SetIsEnabled(false);
            }
            player
                .SetAutoPlay(true)
                .map_err(|e| format!("设置 AutoPlay 失败: {e:?}"))?;
            player
                .SetVolume(1.0)
                .map_err(|e| format!("设置初始音量失败: {e:?}"))?;

            Ok(Self {
                player,
                current_volume: Arc::new(Mutex::new(1.0)),
                is_fading: Arc::new(AtomicBool::new(false)),
                is_loading_source: Arc::new(AtomicBool::new(false)),
                load_failed_msg: Arc::new(StdMutex::new(None)),
            })
        }
        #[cfg(not(windows))]
        {
            Err("非 Windows 平台不支持原生 Windows.Media.Playback.MediaPlayer".to_string())
        }
    }

    #[cfg(windows)]
    pub fn raw_player(&self) -> &MediaPlayer {
        &self.player
    }

    /// 绑定内核播放结束与播放失败事件（即使 WebView2 完全销毁也能在后台独立切歌与容灾）
    pub fn bind_events(&self, tx: UnboundedSender<EngineEvent>) -> Result<(), String> {
        #[cfg(windows)]
        {
            let tx_ended = tx.clone();
            self.player
                .MediaEnded(&TypedEventHandler::<MediaPlayer, IInspectable>::new(
                    move |_sender, _args| {
                        let _ = tx_ended.send(EngineEvent::MediaEnded);
                        Ok(())
                    },
                ))
                .map_err(|e| format!("绑定 MediaEnded 事件失败: {e:?}"))?;

            let tx_failed = tx;
            let loading_flag = Arc::clone(&self.is_loading_source);
            let failed_slot = Arc::clone(&self.load_failed_msg);
            self.player
                .MediaFailed(
                    &TypedEventHandler::<MediaPlayer, MediaPlayerFailedEventArgs>::new(
                        move |_sender, args| {
                            let msg = args
                                .as_ref()
                                .and_then(|a| a.ErrorMessage().ok())
                                .map(|h| h.to_string())
                                .unwrap_or_else(|| "音频流加载或解码失败".to_string());
                            if loading_flag.load(Ordering::SeqCst) {
                                if let Ok(mut slot) = failed_slot.lock() {
                                    *slot = Some(msg);
                                }
                            } else {
                                let _ = tx_failed.send(EngineEvent::MediaFailed(msg));
                            }
                            Ok(())
                        },
                    ),
                )
                .map_err(|e| format!("绑定 MediaFailed 事件失败: {e:?}"))?;

            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = tx;
            Ok(())
        }
    }

    /// 加载网络音频流并同步等待流容器就绪（捕获异步连接期 HTTP 403/404 或解码失败，确保外层重试状态机精准感知）
    pub async fn load_uri_and_verify(
        &self,
        url: &str,
        auto_play: bool,
        cancel_token: Option<&CancellationToken>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            if let Ok(mut slot) = self.load_failed_msg.lock() {
                *slot = None;
            }
            self.is_loading_source.store(true, Ordering::SeqCst);
            let _guard = LoadingFlagGuard(&self.is_loading_source);

            let win_uri = Uri::CreateUri(&HSTRING::from(url))
                .map_err(|e| format!("创建音频流 Uri 失败: {e:?}"))?;
            let source = MediaSource::CreateFromUri(&win_uri)
                .map_err(|e| format!("创建 MediaSource 失败: {e:?}"))?;

            self.player
                .SetSource(&source)
                .map_err(|e| format!("设置音频源失败: {e:?}"))?;

            self.play().await?;

            let mut buffering_ticks = 0u32;
            for _ in 0..100 {
                if let Some(ct) = cancel_token {
                    if ct.is_cancelled() {
                        return Err("切歌加载已被取消".to_string());
                    }
                }

                if let Ok(mut slot) = self.load_failed_msg.lock() {
                    if let Some(err_msg) = slot.take() {
                        return Err(err_msg);
                    }
                }

                if let Ok(session) = self.player.PlaybackSession() {
                    let state = session.PlaybackState().unwrap_or(MediaPlaybackState::None);
                    match state {
                        MediaPlaybackState::Playing | MediaPlaybackState::Paused => {
                            if !auto_play {
                                let _ = self.player.Pause();
                            }
                            return Ok(());
                        }
                        MediaPlaybackState::Buffering => {
                            buffering_ticks += 1;
                            if buffering_ticks >= 24 {
                                if !auto_play {
                                    let _ = self.player.Pause();
                                }
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(ct) = cancel_token {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(45)) => {}
                        _ = ct.cancelled() => return Err("切歌加载已被取消".to_string()),
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(45)).await;
                }
            }

            if let Ok(mut slot) = self.load_failed_msg.lock() {
                if let Some(err_msg) = slot.take() {
                    return Err(err_msg);
                }
            }
            Err("音频流连接打开超时".to_string())
        }
        #[cfg(not(windows))]
        {
            let _ = (url, auto_play, cancel_token);
            Err("不支持的平台".to_string())
        }
    }

    pub async fn play(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.player
                .Play()
                .map_err(|e| format!("播放失败: {e:?}"))?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }

    pub async fn pause(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.player
                .Pause()
                .map_err(|e| format!("暂停失败: {e:?}"))?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }

    /// 毫秒级精准定位进度（1 ms = 10,000 * 100ns）
    pub async fn seek(&self, position_ms: u64) -> Result<(), String> {
        #[cfg(windows)]
        {
            let session = self
                .player
                .PlaybackSession()
                .map_err(|e| format!("获取 PlaybackSession 失败: {e:?}"))?;

            let time_span = TimeSpan {
                Duration: (position_ms as i64) * 10_000,
            };
            session
                .SetPosition(time_span)
                .map_err(|e| format!("Seek 失败: {e:?}"))?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = position_ms;
            Ok(())
        }
    }

    /// 获取当前播放进度与状态
    pub async fn get_progress(&self) -> PlaybackProgress {
        let user_vol = *self.current_volume.lock().await;
        #[cfg(windows)]
        {
            let Ok(session) = self.player.PlaybackSession() else {
                return PlaybackProgress {
                    position_ms: 0,
                    duration_ms: 0,
                    is_playing: false,
                    is_buffering: false,
                    volume: user_vol,
                };
            };

            let pos = session
                .Position()
                .map(|t| (t.Duration.max(0) / 10_000) as u64)
                .unwrap_or(0);
            let dur = session
                .NaturalDuration()
                .map(|t| (t.Duration.max(0) / 10_000) as u64)
                .unwrap_or(0);
            let state = session.PlaybackState().unwrap_or(MediaPlaybackState::None);
            let is_buffering = matches!(
                state,
                MediaPlaybackState::Opening | MediaPlaybackState::Buffering
            );

            PlaybackProgress {
                position_ms: pos,
                duration_ms: dur,
                is_playing: matches!(state, MediaPlaybackState::Playing),
                is_buffering,
                volume: user_vol,
            }
        }
        #[cfg(not(windows))]
        {
            PlaybackProgress {
                position_ms: 0,
                duration_ms: 0,
                is_playing: false,
                is_buffering: false,
                volume: user_vol,
            }
        }
    }

    pub async fn get_user_volume(&self) -> f64 {
        *self.current_volume.lock().await
    }

    /// 设定用户目标音量 (0.0 ~ 1.0)
    pub async fn set_volume(&self, target_volume: f64) -> Result<(), String> {
        let clamped = target_volume.clamp(0.0, 1.0);
        #[cfg(windows)]
        {
            self.player
                .SetVolume(clamped)
                .map_err(|e| format!("设置音量失败: {e:?}"))?;
        }
        let mut vol = self.current_volume.lock().await;
        *vol = clamped;
        Ok(())
    }

    /// 平滑音量插值（遵守 architecture.md 二.2：零重型 DSP，仅调节 MediaPlayer::SetVolume）
    pub async fn ramp_raw_volume(&self, from: f64, to: f64, duration_ms: u64) {
        if self.is_fading.swap(true, Ordering::SeqCst) {
            return;
        }
        let steps = 10u64;
        let step_dur = Duration::from_millis((duration_ms / steps).max(10));
        let diff = to - from;
        for i in 1..=steps {
            let v = (from + diff * (i as f64 / steps as f64)).clamp(0.0, 1.0);
            #[cfg(windows)]
            {
                let _ = self.player.SetVolume(v);
            }
            tokio::time::sleep(step_dur).await;
        }
        self.is_fading.store(false, Ordering::SeqCst);
    }
}
