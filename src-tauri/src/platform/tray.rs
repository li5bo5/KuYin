use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::player::PlaybackMode;
use crate::storage::{log_info, log_warn};
use crate::AppState;

use super::WindowManager;

const MAIN_TRAY_ID: &str = "kuyin-main-tray";

#[cfg(windows)]
static WM_TASKBAR_CREATED_ID: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
static WATCHER_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub struct TrayManager;

impl TrayManager {
    pub fn setup_tray(app: &AppHandle) -> Result<(), String> {
        Self::build_or_rebuild_tray(app)?;
        Self::start_taskbar_restart_watcher(app);
        Ok(())
    }

    /// 构建或重建系统托盘图标（右键弹出原生 Win11 Fluent 自绘菜单）
    pub fn build_or_rebuild_tray(app: &AppHandle) -> Result<(), String> {
        // 若存在失效的旧托盘实例（如 explorer.exe 重启后），先将其移除再重建
        let _ = app.remove_tray_by_id(MAIN_TRAY_ID);

        let mut builder =
            TrayIconBuilder::with_id(MAIN_TRAY_ID).tooltip("酷音 KuYin - 超低内存音乐播放器");

        if let Ok(tray_img) = Image::from_bytes(include_bytes!("../../icons/tray-icon.png")) {
            builder = builder.icon(tray_img);
        } else if let Some(icon) = app.default_window_icon().cloned() {
            builder = builder.icon(icon);
        }

        builder
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    match button {
                        MouseButton::Left => Self::restore_or_create_window(tray.app_handle()),
                        MouseButton::Right => Self::popup_fluent_menu(tray.app_handle()),
                        _ => {}
                    }
                }
            })
            .build(app)
            .map_err(|e| format!("构建系统托盘图标失败: {e:?}"))?;

        log_info("系统托盘初始化就绪");
        Ok(())
    }

    /// 读取当前播放快照后，在独立线程弹出原生 Fluent 菜单；选中项回到主线程执行
    fn popup_fluent_menu(app: &AppHandle) {
        #[cfg(windows)]
        {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let snap = state.coordinator.get_snapshot().await;
                let cover_bgra = state.coordinator.get_tray_cover_bgra().await;
                let is_dark = state
                    .db
                    .get_setting("theme")
                    .map(|t| t == "dark")
                    .unwrap_or(false);
                let params = super::tray_menu::TrayMenuParams {
                    is_dark,
                    title: snap.current_track.as_ref().map(|t| t.name.clone()),
                    subtitle: snap.current_track.as_ref().map(|t| t.singer.clone()),
                    is_playing: snap.is_playing,
                    mode: snap.mode.as_str().to_string(),
                    cover_bgra,
                };
                let menu_handle = handle.clone();
                std::thread::spawn(move || {
                    if let Some(action) = super::tray_menu::show_tray_menu(params) {
                        let h = menu_handle.clone();
                        let _ = menu_handle.run_on_main_thread(move || {
                            Self::handle_menu_action(&h, action);
                        });
                    }
                });
            });
        }
        #[cfg(not(windows))]
        {
            let _ = app;
        }
    }

    /// 执行托盘菜单项动作
    fn handle_menu_action(app: &AppHandle, action: &str) {
        match action {
            "show" => {
                Self::restore_or_create_window(app);
            }
            "settings" => {
                {
                    let state = app.state::<AppState>();
                    if let Ok(mut guard) = state.pending_nav_tab.lock() {
                        *guard = Some("settings".to_string());
                    };
                }
                Self::restore_or_create_window(app);
                let _ = app.emit("tray-navigate", "settings");
            }
            "play_pause" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let snap = state.coordinator.get_snapshot().await;
                    if snap.is_playing {
                        let _ = state.coordinator.pause().await;
                    } else {
                        let _ = state.coordinator.resume_or_play().await;
                    }
                });
            }
            "prev" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let _ = state.coordinator.previous().await;
                });
            }
            "next" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let _ = state.coordinator.next().await;
                });
            }
            "mode_sequence" | "mode_loop" | "mode_single" | "mode_shuffle" => {
                let mode = match action {
                    "mode_loop" => PlaybackMode::Loop,
                    "mode_single" => PlaybackMode::Single,
                    "mode_shuffle" => PlaybackMode::Shuffle,
                    _ => PlaybackMode::Sequence,
                };
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    state.coordinator.set_mode(mode).await;
                });
            }
            "quit" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    if let Some(win) = handle.get_webview_window("main") {
                        WindowManager::save_window_bounds(&win, &state.db);
                    }
                    state.coordinator.persist_snapshot_now().await;
                    crate::ALLOW_APP_EXIT.store(true, Ordering::SeqCst);
                    handle.exit(0);
                });
            }
            _ => {}
        }
    }

    /// 双级休眠唤醒：
    /// - 若处于第 1 级休眠（最小化在任务栏），0ms 瞬时恢复窗口
    /// - 若处于第 2 级休眠（WebView2 已销毁），秒级重建窗口、应用 Mica 并恢复上次显示器坐标
    pub fn restore_or_create_window(app: &AppHandle) {
        let state = app.state::<AppState>();
        let is_dark = state
            .db
            .get_setting("theme")
            .map(|t| t == "dark")
            .unwrap_or(false);

        if let Some(win) = app.get_webview_window("main") {
            WindowManager::resume_webview(&win);
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
            WindowManager::apply_win11_mica(&win, is_dark);
        } else {
            log_info("从第 2 级托盘休眠状态重建 WebView2 主窗口...");
            let builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("酷音 KuYin")
                    .inner_size(1020.0, 690.0)
                    .min_inner_size(780.0, 540.0)
                    .decorations(false)
                    .transparent(false)
                    .shadow(true)
                    .visible(false)
                    .center();

            if let Ok(win) = builder.build() {
                WindowManager::restore_window_bounds(&win, &state.db);
                WindowManager::apply_win11_mica(&win, is_dark);
            }
        }
    }

    /// 架构红线（四.2）：监听 Windows `WM_TASKBARCREATED` 广播消息，在 `explorer.exe` 重启后自动重建托盘图标
    fn start_taskbar_restart_watcher(app: &AppHandle) {
        #[cfg(windows)]
        {
            if WATCHER_APP_HANDLE.set(app.clone()).is_err() {
                return;
            }

            std::thread::spawn(|| unsafe {
                use windows::core::w;
                use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
                use windows::Win32::System::LibraryLoader::GetModuleHandleW;
                use windows::Win32::UI::WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
                    RegisterClassW, RegisterWindowMessageW, TranslateMessage, MSG, WINDOW_EX_STYLE,
                    WINDOW_STYLE, WNDCLASSW, WS_EX_TOOLWINDOW,
                };

                unsafe extern "system" fn taskbar_wnd_proc(
                    hwnd: HWND,
                    msg: u32,
                    wparam: WPARAM,
                    lparam: LPARAM,
                ) -> LRESULT {
                    let target_msg = WM_TASKBAR_CREATED_ID.load(Ordering::Relaxed);
                    if target_msg != 0 && msg == target_msg {
                        log_warn("检测到资源管理器 explorer.exe 重启 (WM_TASKBARCREATED)，正在自动重建系统托盘...");
                        if let Some(app_handle) = WATCHER_APP_HANDLE.get() {
                            let h = app_handle.clone();
                            let _ = app_handle.run_on_main_thread(move || {
                                let _ = TrayManager::build_or_rebuild_tray(&h);
                            });
                        }
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }

                let wm_id = RegisterWindowMessageW(w!("TaskbarCreated"));
                WM_TASKBAR_CREATED_ID.store(wm_id, Ordering::Relaxed);

                let hinstance = GetModuleHandleW(None).unwrap_or_default();
                let class_name = w!("KuYinTaskbarCreatedWatcher");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(taskbar_wnd_proc),
                    hInstance: hinstance.into(),
                    lpszClassName: class_name,
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);

                // 创建不可见的 0x0 顶层工具窗口接收 HWND_BROADCAST 消息（零任务栏图标、零内存负担）
                let _hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WINDOW_EX_STYLE(0),
                    class_name,
                    w!(""),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    hinstance,
                    None,
                );

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            });
        }
        #[cfg(not(windows))]
        {
            let _ = app;
        }
    }
}
