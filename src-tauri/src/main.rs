// 生产构建隐藏额外控制台窗口，切勿移除
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod kugou;
mod platform;
mod player;
mod storage;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};

use audio::{AudioDeviceInfo, DeviceManager, WindowsPlaybackEngine};
use kugou::KugouClient;
use platform::{ShortcutConfig, ShortcutManager, TrayManager, WindowManager};
use player::{PlaybackCoordinator, PlaybackMode, PlayerSnapshot, Track};
use storage::{get_log_dir, init_logger, log_info, log_warn, BackupManager, Database};

/// 控制进程是否允许真正退出的全局原子门禁（防止关闭窗口销毁 WebView2 时误退出后台宿主）
pub static ALLOW_APP_EXIT: AtomicBool = AtomicBool::new(false);
static WINDOW_MINIMIZED_FLAG: AtomicBool = AtomicBool::new(false);

pub struct AppState {
    pub coordinator: Arc<PlaybackCoordinator>,
    pub kugou: Arc<KugouClient>,
    pub db: Arc<Database>,
    pub storage_dir: PathBuf,
    pub pending_nav_tab: std::sync::Mutex<Option<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthProfile {
    pub is_logged_in: bool,
    pub userid: String,
    pub nickname: String,
    pub pic: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettingsPayload {
    pub theme: String,
    pub close_behavior: String,
    pub selected_device_id: String,
    pub storage_dir: String,
    pub shortcuts: ShortcutConfig,
}

fn persist_login_from_response(db: &Database, data: &Value) -> Option<(String, String, Option<String>)> {
    let token = data.get("token").and_then(|v| v.as_str()).unwrap_or("").trim();
    if token.is_empty() {
        return None;
    }
    let userid = data
        .get("userid")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "0".to_string());
    if userid.is_empty() || userid == "0" {
        return None;
    }

    let nickname = data
        .get("nickname")
        .or_else(|| data.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("酷音用户");
    let pic = data
        .get("pic")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let t1_opt = data
        .get("t1")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let _ = db.set_setting("user_token", token);
    let _ = db.set_setting("user_id", &userid);
    let _ = db.set_setting("user_nickname", nickname);
    let _ = db.set_setting("user_pic", pic);
    if let Some(ref t1) = t1_opt {
        let _ = db.set_setting("user_t1", t1);
    }

    Some((token.to_string(), userid, t1_opt))
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<PlayerSnapshot, String> {
    Ok(state.coordinator.get_snapshot().await)
}

#[tauri::command]
async fn play(state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.resume_or_play().await
}

#[tauri::command]
async fn pause(state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.pause().await
}

#[tauri::command]
async fn next_track(state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.next().await
}

#[tauri::command]
async fn previous_track(state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.previous().await
}

#[tauri::command]
async fn seek(position_ms: u64, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.seek(position_ms).await
}

#[tauri::command]
async fn set_volume(volume: f64, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.set_volume(volume).await
}

#[tauri::command]
async fn set_mode(mode: PlaybackMode, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.set_mode(mode).await;
    Ok(())
}

#[tauri::command]
async fn set_quality(quality: String, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.set_quality(&quality).await
}

#[tauri::command]
async fn play_song(track: Track, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.play_track(track).await
}

#[tauri::command]
async fn play_queue_index(index: usize, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.play_at_index(index).await
}

#[tauri::command]
async fn set_queue(
    tracks: Vec<Track>,
    start_index: Option<usize>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.coordinator.set_queue(tracks, start_index).await
}

#[tauri::command]
async fn insert_next_queue(track: Track, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.insert_next_in_queue(track).await;
    Ok(())
}

#[tauri::command]
async fn append_song_to_queue(track: Track, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.append_one_to_queue(track).await;
    Ok(())
}

#[tauri::command]
async fn append_queue(tracks: Vec<Track>, state: State<'_, AppState>) -> Result<usize, String> {
    Ok(state.coordinator.append_tracks_to_queue(tracks).await)
}

#[tauri::command]
async fn remove_from_queue(index: usize, state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.remove_from_queue(index).await;
    Ok(())
}

#[tauri::command]
async fn move_queue_item(
    from: usize,
    to: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.coordinator.move_in_queue(from, to).await;
    Ok(())
}

#[tauri::command]
async fn clear_queue(state: State<'_, AppState>) -> Result<(), String> {
    state.coordinator.clear_queue().await
}

#[tauri::command]
async fn get_queue(state: State<'_, AppState>) -> Result<Vec<Track>, String> {
    Ok(state.coordinator.get_queue_tracks().await)
}

#[tauri::command]
async fn search(
    keyword: String,
    page: u32,
    pagesize: u32,
    search_type: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .search_music(&keyword, page, pagesize, &search_type)
        .await
}

#[tauri::command]
async fn search_suggest(keyword: String, state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.get_search_suggest(&keyword).await
}

#[tauri::command]
async fn get_lyric(
    hash: String,
    duration_ms: u64,
    keyword: Option<String>,
    album_audio_id: Option<i64>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state
        .kugou
        .get_song_lyric(&hash, duration_ms, keyword.as_deref(), album_audio_id)
        .await
}

#[tauri::command]
async fn get_auth_profile(state: State<'_, AppState>) -> Result<AuthProfile, String> {
    let token = state.db.get_setting("user_token").unwrap_or_default();
    let userid = state.db.get_setting("user_id").unwrap_or_default();
    let nickname = state
        .db
        .get_setting("user_nickname")
        .unwrap_or_else(|| "未登录".to_string());
    let pic = state.db.get_setting("user_pic").unwrap_or_default();
    let is_logged_in = !token.is_empty() && !userid.is_empty() && userid != "0";

    Ok(AuthProfile {
        is_logged_in,
        userid,
        nickname,
        pic,
    })
}

#[tauri::command]
async fn get_qr_key(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.get_qr_key().await
}

#[tauri::command]
async fn get_wx_qr(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.create_wx_qr().await
}

#[tauri::command]
async fn check_wx_qr_status(uuid: String, state: State<'_, AppState>) -> Result<Value, String> {
    let mut res = state.kugou.check_wx_qr_status(&uuid).await?;
    let wx_errcode = res
        .get("wx_errcode")
        .or_else(|| res.get("status"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if wx_errcode == 405 {
        if let Some(wx_code) = res
            .get("wx_code")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
        {
            let login_res = state.kugou.login_by_wx_code(&wx_code).await?;
            if login_res.get("status").and_then(|v| v.as_i64()) == Some(1) {
                if let Some(data) = login_res.get("data") {
                    if let Some((token, uid, t1)) = persist_login_from_response(&state.db, data) {
                        state
                            .kugou
                            .update_auth(&token, &uid, t1.as_deref())
                            .await;
                        log_info(&format!("微信扫码登录成功，用户 ID: {uid}"));
                    }
                }
            }
            if let Some(obj) = res.as_object_mut() {
                obj.insert("login".to_string(), login_res);
            }
        }
    }
    Ok(res)
}

#[tauri::command]
async fn check_qr_status(qrcode: String, state: State<'_, AppState>) -> Result<Value, String> {
    let res = state.kugou.check_qr_status(&qrcode).await?;
    if let Some(data) = res.get("data") {
        let status = data.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        if status == 4 {
            if let Some((token, uid, t1)) = persist_login_from_response(&state.db, data) {
                state
                    .kugou
                    .update_auth(&token, &uid, t1.as_deref())
                    .await;
                log_info(&format!("扫码登录成功，用户 ID: {uid}"));
            }
        }
    }
    Ok(res)
}

#[tauri::command]
async fn send_sms_code(mobile: String, state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.send_mobile_code(&mobile).await
}

#[tauri::command]
async fn login_cellphone(
    mobile: String,
    code: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let res = state.kugou.login_by_mobile(&mobile, &code).await?;
    if res.get("status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(data) = res.get("data") {
            if let Some((token, uid, t1)) = persist_login_from_response(&state.db, data) {
                state
                    .kugou
                    .update_auth(&token, &uid, t1.as_deref())
                    .await;
                log_info(&format!("手机验证码登录成功，用户 ID: {uid}"));
            }
        }
    }
    Ok(res)
}

#[tauri::command]
async fn login_password(
    username: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let res = state.kugou.login_by_password(&username, &password).await?;
    if res.get("status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(data) = res.get("data") {
            if let Some((token, uid, t1)) = persist_login_from_response(&state.db, data) {
                state
                    .kugou
                    .update_auth(&token, &uid, t1.as_deref())
                    .await;
                log_info(&format!("账号密码登录成功，用户 ID: {uid}"));
            }
        }
    }
    Ok(res)
}

#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let _ = state.db.delete_setting("user_token");
    let _ = state.db.delete_setting("user_id");
    let _ = state.db.delete_setting("user_nickname");
    let _ = state.db.delete_setting("user_pic");
    let _ = state.db.delete_setting("user_t1");
    state.kugou.update_auth("", "0", Some("")).await;
    log_info("用户已退出登录并清除本地凭证");
    Ok(())
}

#[tauri::command]
async fn claim_vip(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.claim_youth_vip().await
}

#[tauri::command]
async fn get_vip_detail(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.get_vip_detail().await
}

#[tauri::command]
async fn get_daily_recommend(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.get_daily_recommend().await
}

#[tauri::command]
async fn get_top_playlists(
    page: u32,
    pagesize: u32,
    refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .get_top_playlists(page, pagesize, refresh.unwrap_or(false))
        .await
}

#[tauri::command]
async fn get_playlist_detail(
    id: String,
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state.kugou.get_playlist_detail(&id, page, pagesize).await
}

#[tauri::command]
async fn get_user_playlists(
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state.kugou.get_user_playlists(page, pagesize).await
}

#[tauri::command]
async fn get_user_playlist_tracks(
    listid: i64,
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .get_user_playlist_tracks(listid, page, pagesize)
        .await
}

#[tauri::command]
async fn favorite_song(
    listid: i64,
    track: Track,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let song_full_name = format!("{} - {}", track.singer, track.name);
    state
        .kugou
        .playlist_add_song(
            listid,
            &song_full_name,
            &track.hash,
            track.album_id.as_deref(),
            track.album_audio_id,
        )
        .await
}

#[tauri::command]
async fn remove_playlist_song(
    listid: i64,
    fileid: i64,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state.kugou.playlist_del_song(listid, fileid).await
}

#[tauri::command]
async fn create_or_collect_playlist(
    name: String,
    list_type: u32,
    list_create_userid: Option<u64>,
    list_create_listid: Option<i64>,
    list_create_gid: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .create_or_collect_playlist(
            &name,
            list_type,
            list_create_userid,
            list_create_listid,
            list_create_gid.as_deref(),
        )
        .await
}

#[tauri::command]
async fn delete_playlist(listid: i64, state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.delete_playlist(listid).await
}

#[tauri::command]
async fn get_artist_songs(
    author_id: String,
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .get_artist_songs(&author_id, page, pagesize)
        .await
}

#[tauri::command]
async fn get_album_songs(
    album_id: String,
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state.kugou.get_album_songs(&album_id, page, pagesize).await
}

#[tauri::command]
async fn get_personal_fm(state: State<'_, AppState>) -> Result<Value, String> {
    state.kugou.get_personal_fm().await
}

#[tauri::command]
async fn get_rankings(
    refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state.kugou.get_rankings(refresh.unwrap_or(false)).await
}

#[tauri::command]
async fn get_ranking_songs(
    rank_id: String,
    page: u32,
    pagesize: u32,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    state
        .kugou
        .get_ranking_songs(&rank_id, page, pagesize)
        .await
}

#[tauri::command]
async fn get_history(limit: u32, state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    state.db.get_history(limit)
}

#[tauri::command]
async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.db.clear_history()
}

#[tauri::command]
async fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    DeviceManager::list_output_devices().await
}

#[tauri::command]
async fn set_audio_device(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let saved_id = device_id.clone().unwrap_or_default();
    let _ = state.db.set_setting("audio_device_id", &saved_id);
    state
        .coordinator
        .update_watched_device(device_id.clone())
        .await;
    #[cfg(windows)]
    {
        DeviceManager::set_audio_device(state.coordinator.raw_engine().raw_player(), device_id)
            .await?;
        state
            .coordinator
            .rebind_stream_after_device_switch()
            .await;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

#[tauri::command]
async fn frontend_ready(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    Ok(())
}

#[tauri::command]
async fn set_theme(
    app: AppHandle,
    theme: String,
    resolved_dark: Option<bool>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let normalized = match theme.as_str() {
        "dark" => "dark",
        "system" => "system",
        _ => "light",
    };
    state.db.set_setting("theme", normalized)?;
    let is_dark = resolved_dark.unwrap_or(normalized == "dark");
    if let Some(win) = app.get_webview_window("main") {
        WindowManager::apply_win11_mica(&win, is_dark);
    }
    Ok(())
}

#[tauri::command]
async fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettingsPayload, String> {
    let theme = state
        .db
        .get_setting("theme")
        .unwrap_or_else(|| "light".to_string());
    let close_behavior = state
        .db
        .get_setting("close_behavior")
        .unwrap_or_else(|| "tray".to_string());
    let selected_device_id = state
        .db
        .get_setting("audio_device_id")
        .unwrap_or_default();
    let storage_dir = state.storage_dir.display().to_string();
    let shortcuts = ShortcutManager::load_config(&state.db);

    Ok(AppSettingsPayload {
        theme,
        close_behavior,
        selected_device_id,
        storage_dir,
        shortcuts,
    })
}

#[tauri::command]
async fn set_close_behavior(
    behavior: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let normalized = if behavior == "exit" { "exit" } else { "tray" };
    state.db.set_setting("close_behavior", normalized)
}

#[tauri::command]
async fn close_main_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let close_behavior = state
        .db
        .get_setting("close_behavior")
        .unwrap_or_else(|| "tray".to_string());

    if let Some(win) = app.get_webview_window("main") {
        WindowManager::save_window_bounds(&win, &state.db);
        if close_behavior == "exit" {
            state.coordinator.persist_snapshot_now().await;
            ALLOW_APP_EXIT.store(true, Ordering::SeqCst);
            app.exit(0);
        } else {
            log_info("触发第 2 级托盘休眠：执行 window.destroy() 彻底销毁 WebView2 进程组");
            let _ = win.destroy();
        }
    }
    Ok(())
}

#[tauri::command]
async fn get_storage_dir(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.storage_dir.display().to_string())
}

#[tauri::command]
async fn pick_storage_folder() -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::{
                FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
                SIGDN_FILESYSPATH,
            };

            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> Option<String> {
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
                let opts = dialog.GetOptions().ok()?;
                dialog
                    .SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
                    .ok()?;
                if dialog.Show(None).is_err() {
                    return None;
                }
                let item = dialog.GetResult().ok()?;
                let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
                let path_str = pwstr.to_string().ok();
                CoTaskMemFree(Some(pwstr.0 as _));
                path_str
            })();
            CoUninitialize();
            Ok(result)
        }
        #[cfg(not(windows))]
        {
            Ok(None)
        }
    })
    .await
    .map_err(|e| format!("打开文件夹选择器失败: {e}"))?
}

#[tauri::command]
async fn set_storage_dir(new_dir: String, state: State<'_, AppState>) -> Result<String, String> {
    let trimmed = new_dir.trim();
    if trimmed.is_empty() {
        return Err("缓存目录路径不能为空".to_string());
    }
    let target_dir = PathBuf::from(trimmed);
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("创建目标缓存目录失败: {e:?}"))?;

    // 先将最新播放快照写入当前数据库，随后迁移至新目录
    state.coordinator.persist_snapshot_now().await;

    let current_db = state.storage_dir.join("kuyin.db");
    let target_db = target_dir.join("kuyin.db");
    if current_db != target_db && current_db.exists() {
        let _ = std::fs::copy(&current_db, &target_db);
    }

    let bootstrap_dir = app_data_dir_fallback();
    let _ = std::fs::create_dir_all(&bootstrap_dir);
    let pointer_file = bootstrap_dir.join("storage_path.txt");
    std::fs::write(&pointer_file, target_dir.display().to_string())
        .map_err(|e| format!("写入自定义缓存路径配置失败: {e:?}"))?;

    log_info(&format!(
        "已更新自定义缓存存储目录并迁移数据至: {}",
        target_dir.display()
    ));
    Ok(target_dir.display().to_string())
}

#[tauri::command]
async fn open_storage_folder(state: State<'_, AppState>) -> Result<String, String> {
    let dir = state.storage_dir.clone();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    Ok(dir.display().to_string())
}

#[tauri::command]
async fn backup_database_now(state: State<'_, AppState>) -> Result<String, String> {
    state.coordinator.persist_snapshot_now().await;
    let db_path = state.storage_dir.join("kuyin.db");
    BackupManager::manual_backup_now(&db_path)
}

#[tauri::command]
async fn update_shortcuts(
    app: AppHandle,
    config: ShortcutConfig,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let json_str =
        serde_json::to_string(&config).map_err(|e| format!("序列化热键失败: {e:?}"))?;
    state.db.set_setting("global_shortcuts", &json_str)?;
    let conflicts = ShortcutManager::apply_shortcuts(&app, &config)?;
    Ok(json!({ "conflicts": conflicts }))
}

#[tauri::command]
async fn open_log_folder() -> Result<String, String> {
    let dir = get_log_dir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| resolve_storage_dir().join("logs"));
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    Ok(dir.display().to_string())
}

#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err("仅支持打开 http/https 安全链接".to_string());
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(trimmed).spawn();
    }
    Ok(())
}

#[tauri::command]
async fn take_pending_nav_tab(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let mut guard = state
        .pending_nav_tab
        .lock()
        .map_err(|_| "导航状态锁获取失败".to_string())?;
    Ok(guard.take())
}

#[tauri::command]
async fn clean_temp_cache(state: State<'_, AppState>) -> Result<String, String> {
    state.kugou.clear_cache().await;
    state.db.checkpoint_wal();
    let dir = state.storage_dir.clone();
    let removed = tokio::task::spawn_blocking(move || purge_webview_temp_cache(&dir, 0))
        .await
        .unwrap_or(0);
    let msg = format!(
        "已清理内存 API 缓存、回收 SQLite WAL 日志并清理 {removed} 个 WebView2 临时缓存文件"
    );
    log_info(&msg);
    Ok(msg)
}

/// 清理 WebView2 产生的 HTTP 磁盘缓存目录（`Cache`、`Code Cache`、`GPUCache`），绝不触碰 `kuyin.db`
pub fn purge_webview_temp_cache(storage_dir: &std::path::Path, max_age_secs: u64) -> usize {
    let mut candidate_roots = vec![storage_dir.to_path_buf()];
    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local_appdata);
        candidate_roots.push(base.join("com.kuyin.music"));
        candidate_roots.push(base.join("KuYin"));
    }

    let sub_dirs = [
        "EBWebView/Default/Cache",
        "EBWebView/Default/Code Cache",
        "EBWebView/Default/GPUCache",
    ];

    let now = SystemTime::now();
    let mut removed_count = 0usize;

    for root in candidate_roots {
        for sub in &sub_dirs {
            let dir = root.join(sub);
            if !dir.exists() {
                continue;
            }
            removed_count += clean_dir_files_older_than(&dir, now, max_age_secs);
        }
    }
    removed_count
}

fn clean_dir_files_older_than(
    dir: &std::path::Path,
    now: SystemTime,
    max_age_secs: u64,
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += clean_dir_files_older_than(&path, now, max_age_secs);
        } else if path.is_file() {
            let should_delete = if max_age_secs == 0 {
                true
            } else {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|mtime| now.duration_since(mtime).ok())
                    .map(|age| age.as_secs() >= max_age_secs)
                    .unwrap_or(false)
            };
            if should_delete && std::fs::remove_file(&path).is_ok() {
                count += 1;
            }
        }
    }
    count
}

fn main() {
    // 0. 在创建 MediaPlayer 与 Tauri 宿主前提前锁定进程 AUMID 并同步开始菜单快捷方式标识（解决便携版 Win+A 显示“未知应用”问题）
    platform::smtc::SmtcManager::ensure_app_identity();

    let context = tauri::generate_context!();

    // 1. 解析应用数据/缓存目录（支持用户自定义位置）并初始化 5MB/2文件滚动日志系统
    let app_dir = resolve_storage_dir();
    let _ = init_logger(app_dir.clone());
    log_info(&format!("酷音 KuYin 启动初始化，数据目录: {}", app_dir.display()));

    // 2. 数据库完整性自检与 .bak 自动备份自愈
    let db_path = app_dir.join("kuyin.db");
    let _ = BackupManager::verify_and_auto_heal(&db_path);
    let db = Arc::new(Database::init(&db_path).expect("初始化 SQLite 数据库失败"));

    // 3. 初始化系统原生 MediaPlayer 硬件解码引擎
    let engine =
        Arc::new(WindowsPlaybackEngine::new().expect("初始化原生 Windows MediaPlayer 失败"));

    // 4. 初始化酷狗概念版直连引擎并恢复设备指纹与登录凭证
    let guid = db.get_setting("guid").unwrap_or_else(|| {
        let g = kugou::config::generate_guid();
        let _ = db.set_setting("guid", &g);
        g
    });
    let mac = db.get_setting("device_mac").unwrap_or_else(|| {
        let m = kugou::config::generate_mac();
        let _ = db.set_setting("device_mac", &m);
        m
    });
    let dfid = db.get_setting("dfid").unwrap_or_default();
    let kugou_client = Arc::new(KugouClient::new(guid, mac, dfid.clone()));

    // 5. 初始化独立播放状态机与协调器
    let coordinator = Arc::new(PlaybackCoordinator::new(
        Arc::clone(&engine),
        Arc::clone(&kugou_client),
        Arc::clone(&db),
    ));

    let app_state = AppState {
        coordinator: Arc::clone(&coordinator),
        kugou: Arc::clone(&kugou_client),
        db: Arc::clone(&db),
        storage_dir: app_dir.clone(),
        pending_nav_tab: std::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            TrayManager::restore_or_create_window(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state)
        .setup(move |app| {
            let handle = app.handle().clone();

            // 系统托盘与 WM_TASKBARCREATED 守护监听
            TrayManager::setup_tray(&handle)?;

            // 注册全局快捷键
            let sc_cfg = ShortcutManager::load_config(&db);
            let _ = ShortcutManager::apply_shortcuts(&handle, &sc_cfg);

            // 主窗口应用 Win11 Mica 云母材质（默认浅色模式）并恢复多显示器安全坐标
            let is_dark = db
                .get_setting("theme")
                .map(|t| t == "dark")
                .unwrap_or(false);
            if let Some(win) = app.get_webview_window("main") {
                WindowManager::restore_window_bounds(&win, &db);
                WindowManager::apply_win11_mica(&win, is_dark);
            }

            // 异步启动后台播放守护、恢复断点、刷新登录态、每日静默续期概念版 VIP 与设备 dfid
            let coord_bg = Arc::clone(&coordinator);
            let kugou_bg = Arc::clone(&kugou_client);
            let db_bg = Arc::clone(&db);
            let app_dir_bg = app_dir.clone();
            let handle_bg = handle.clone();
            tauri::async_runtime::spawn(async move {
                // 启动时静默回收 SQLite WAL 预写日志并清理超过 3 天的 WebView2 HTTP 临时缓存
                db_bg.checkpoint_wal();
                let _ = tokio::task::spawn_blocking(move || {
                    purge_webview_temp_cache(&app_dir_bg, 3 * 86_400)
                })
                .await;

                // 恢复已保存的音频输出设备偏好
                if let Some(dev_id) = db_bg.get_setting("audio_device_id") {
                    if !dev_id.is_empty() {
                        #[cfg(windows)]
                        {
                            let _ = DeviceManager::set_audio_device(
                                coord_bg.raw_engine().raw_player(),
                                Some(dev_id),
                            )
                            .await;
                        }
                    }
                }

                // 恢复登录凭证并静默刷新 Token / 补全用户资料 / 每日自动领取概念版 VIP
                let saved_token = db_bg.get_setting("user_token").unwrap_or_default();
                let saved_uid = db_bg.get_setting("user_id").unwrap_or_default();
                let saved_t1 = db_bg.get_setting("user_t1");
                if !saved_token.is_empty() && !saved_uid.is_empty() && saved_uid != "0" {
                    kugou_bg
                        .update_auth(&saved_token, &saved_uid, saved_t1.as_deref())
                        .await;
                    if let Ok(refreshed) = kugou_bg.refresh_login_token().await {
                        if let Some(data) = refreshed.get("data") {
                            if let Some((new_tk, new_uid, new_t1)) =
                                persist_login_from_response(&db_bg, data)
                            {
                                kugou_bg
                                    .update_auth(&new_tk, &new_uid, new_t1.as_deref())
                                    .await;
                            }
                        }
                    }
                    let _ = kugou_bg.get_user_detail().await;

                    // 每日静默自动签到领取概念版畅听 VIP
                    let day_key = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| (d.as_secs() / 86_400).to_string())
                        .unwrap_or_default();
                    if !day_key.is_empty()
                        && db_bg.get_setting("last_auto_vip_day").as_deref() != Some(day_key.as_str())
                        && kugou_bg.claim_youth_vip().await.is_ok()
                    {
                        let _ = db_bg.set_setting("last_auto_vip_day", &day_key);
                        log_info("已完成今日概念版 VIP 自动签到领取");
                    }
                }

                // 首次无 dfid 时自动注册虚拟设备指纹
                if dfid.is_empty() {
                    if let Ok(new_dfid) = kugou_bg.register_dev().await {
                        if !new_dfid.is_empty() {
                            let _ = db_bg.set_setting("dfid", &new_dfid);
                        }
                    }
                }

                // 恢复上次播放队列与断点，启动内核事件与低频落盘服务
                coord_bg.restore_from_db().await;
                coord_bg.attach_background_services(handle_bg).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let state = app.state::<AppState>();
                let close_behavior = state
                    .db
                    .get_setting("close_behavior")
                    .unwrap_or_else(|| "tray".to_string());

                if let Some(ww) = app.get_webview_window(window.label()) {
                    WindowManager::save_window_bounds(&ww, &state.db);
                }

                if close_behavior == "exit" {
                    api.prevent_close();
                    tauri::async_runtime::spawn(async move {
                        let st = app.state::<AppState>();
                        st.coordinator.persist_snapshot_now().await;
                        st.db.checkpoint_wal();
                        let _ = purge_webview_temp_cache(&st.storage_dir, 3 * 86_400);
                        ALLOW_APP_EXIT.store(true, Ordering::SeqCst);
                        app.exit(0);
                    });
                } else {
                    // 架构红线（四.1）：第 2 级窗口休眠——保存坐标后调用 window.destroy() 彻底销毁 WebView2 进程组，后台内存回落至 12~18MB！
                    api.prevent_close();
                    log_info("触发第 2 级托盘休眠：执行 window.destroy() 彻底销毁 WebView2 进程组");
                    let _ = window.destroy();
                }
            } else if let WindowEvent::Resized(_) = event {
                let is_min = window.is_minimized().unwrap_or(false);
                let prev_min = WINDOW_MINIMIZED_FLAG.swap(is_min, Ordering::SeqCst);
                if is_min && !prev_min {
                    log_warn("触发第 1 级任务栏休眠：调用 TrySuspend 挂起 WebView2 渲染页，保留窗口支持 0ms 秒开");
                    if let Some(ww) = window.app_handle().get_webview_window(window.label()) {
                        WindowManager::suspend_webview(&ww);
                    }
                } else if !is_min && prev_min {
                    if let Some(ww) = window.app_handle().get_webview_window(window.label()) {
                        WindowManager::resume_webview(&ww);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            play,
            pause,
            next_track,
            previous_track,
            seek,
            set_volume,
            set_mode,
            set_quality,
            play_song,
            play_queue_index,
            set_queue,
            insert_next_queue,
            append_song_to_queue,
            append_queue,
            remove_from_queue,
            move_queue_item,
            clear_queue,
            get_queue,
            search,
            search_suggest,
            get_lyric,
            get_auth_profile,
            get_qr_key,
            get_wx_qr,
            check_wx_qr_status,
            check_qr_status,
            send_sms_code,
            login_cellphone,
            login_password,
            logout,
            claim_vip,
            get_vip_detail,
            get_daily_recommend,
            get_top_playlists,
            get_playlist_detail,
            get_user_playlists,
            get_user_playlist_tracks,
            favorite_song,
            remove_playlist_song,
            create_or_collect_playlist,
            delete_playlist,
            get_artist_songs,
            get_album_songs,
            get_personal_fm,
            get_rankings,
            get_ranking_songs,
            get_history,
            clear_history,
            list_audio_devices,
            set_audio_device,
            frontend_ready,
            set_theme,
            get_app_settings,
            set_close_behavior,
            close_main_window,
            get_storage_dir,
            pick_storage_folder,
            set_storage_dir,
            open_storage_folder,
            backup_database_now,
            update_shortcuts,
            open_log_folder,
            open_external_url,
            take_pending_nav_tab,
            clean_temp_cache,
        ])
        .build(context)
        .expect("构建酷音 KuYin 宿主失败")
        .run(|_app, event| {
            // 阻止关闭最后一个窗口（window.destroy()）后事件循环自动退出进程，确保托盘零 WebView2 后台常驻！
            if let RunEvent::ExitRequested { api, .. } = event {
                if !ALLOW_APP_EXIT.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}

fn resolve_storage_dir() -> PathBuf {
    let default_dir = app_data_dir_fallback();
    let pointer_file = default_dir.join("storage_path.txt");
    if let Ok(raw) = std::fs::read_to_string(&pointer_file) {
        let custom = raw.trim();
        if !custom.is_empty() {
            let custom_path = PathBuf::from(custom);
            if std::fs::create_dir_all(&custom_path).is_ok() {
                return custom_path;
            }
        }
    }
    let _ = std::fs::create_dir_all(&default_dir);
    default_dir
}

fn app_data_dir_fallback() -> PathBuf {
    if let Some(dir) = std::env::var_os("APPDATA") {
        PathBuf::from(dir).join("KuYin")
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(".kuyin_data")
    }
}
