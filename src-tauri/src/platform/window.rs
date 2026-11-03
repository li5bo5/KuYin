use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::storage::{log_warn, Database};

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWCP_ROUND, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE,
};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONULL,
};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
    LR_DEFAULTCOLOR, LR_SHARED, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SavedWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub is_maximized: bool,
}

#[cfg(windows)]
mod wv2_suspend {
    use core::ffi::c_void;
    use windows::core::GUID;

    const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
    const IID_TRY_SUSPEND_HANDLER: GUID = GUID::from_u128(0x00f206a7_9d17_4605_91f6_4e8e4de192e3);
    const IID_ICOREWEBVIEW2_3: GUID = GUID::from_u128(0xa0d6df20_3b92_416d_aa0c_437a9c727857);

    #[repr(C)]
    struct HandlerVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        invoke: unsafe extern "system" fn(*mut c_void, i32, i32) -> i32,
    }

    #[repr(C)]
    struct StaticHandler {
        vtbl: *const HandlerVtbl,
    }

    unsafe impl Sync for StaticHandler {}

    unsafe extern "system" fn noop_qi(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> i32 {
        if ppv.is_null() {
            return -2147467261i32; // E_POINTER
        }
        if !riid.is_null() && (*riid == IID_IUNKNOWN || *riid == IID_TRY_SUSPEND_HANDLER) {
            *ppv = this;
            0
        } else {
            *ppv = core::ptr::null_mut();
            -2147467262i32 // E_NOINTERFACE
        }
    }

    unsafe extern "system" fn noop_ref(_this: *mut c_void) -> u32 {
        1
    }

    unsafe extern "system" fn noop_invoke(_this: *mut c_void, _err: i32, _ok: i32) -> i32 {
        0
    }

    static HANDLER_VTBL: HandlerVtbl = HandlerVtbl {
        query_interface: noop_qi,
        add_ref: noop_ref,
        release: noop_ref,
        invoke: noop_invoke,
    };

    static NOOP_HANDLER: StaticHandler = StaticHandler {
        vtbl: &HANDLER_VTBL,
    };

    pub unsafe fn try_suspend_or_resume(raw_core_wv2: *mut c_void, suspend: bool) {
        if raw_core_wv2.is_null() {
            return;
        }
        let vtbl = *(raw_core_wv2 as *const *const usize);
        let qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32 =
            core::mem::transmute(*vtbl.add(0));

        let mut wv3_ptr: *mut c_void = core::ptr::null_mut();
        if qi(raw_core_wv2, &IID_ICOREWEBVIEW2_3, &mut wv3_ptr) >= 0 && !wv3_ptr.is_null() {
            let vtbl3 = *(wv3_ptr as *const *const usize);
            if suspend {
                // ICoreWebView2_3::TrySuspend 位于 COM 虚函数表槽位 68
                let try_suspend_fn: unsafe extern "system" fn(*mut c_void, *const StaticHandler) -> i32 =
                    core::mem::transmute(*vtbl3.add(68));
                let _ = try_suspend_fn(wv3_ptr, &NOOP_HANDLER);
            } else {
                // ICoreWebView2_3::Resume 位于 COM 虚函数表槽位 69
                let resume_fn: unsafe extern "system" fn(*mut c_void) -> i32 =
                    core::mem::transmute(*vtbl3.add(69));
                let _ = resume_fn(wv3_ptr);
            }
            let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
                core::mem::transmute(*vtbl3.add(2));
            let _ = release_fn(wv3_ptr);
        }
    }
}

pub struct WindowManager;

impl WindowManager {
    /// 启用 Win11 DWM 原生 Mica 云母材质并同步深浅色沉浸式边框
    pub fn apply_win11_mica(window: &WebviewWindow, is_dark: bool) {
        #[cfg(windows)]
        {
            if let Ok(hwnd) = window.hwnd() {
                unsafe {
                    let raw_hwnd = HWND(hwnd.0 as _);
                    let dark_mode: u32 = u32::from(is_dark);
                    let _ = DwmSetWindowAttribute(
                        raw_hwnd,
                        DWMWA_USE_IMMERSIVE_DARK_MODE,
                        &dark_mode as *const _ as _,
                        std::mem::size_of::<u32>() as u32,
                    );

                    // 启用 Win11 DWM 原生无锯齿窗口圆角与阴影（无需开启 WebView2 透明通道，杜绝失焦透底）
                    let corner_pref = DWMWCP_ROUND;
                    let _ = DwmSetWindowAttribute(
                        raw_hwnd,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &corner_pref as *const _ as _,
                        std::mem::size_of_val(&corner_pref) as u32,
                    );

                    // 2 = DWMSBT_MAINWINDOW (Mica)
                    let backdrop_type: u32 = 2;
                    let _ = DwmSetWindowAttribute(
                        raw_hwnd,
                        DWMWA_SYSTEMBACKDROP_TYPE,
                        &backdrop_type as *const _ as _,
                        std::mem::size_of::<u32>() as u32,
                    );

                    // 直接从可执行文件内嵌资源 (ID 32512) 加载原生大小图标并绑定至窗口 HWND，
                    // 绕过 tao RgbaIcon::into_windows_icon 中 CreateIcon 1bpp AND 掩码失真导致的任务管理器子进程图标花屏问题
                    if let Ok(hmodule) = GetModuleHandleW(None) {
                        let hinstance = HINSTANCE(hmodule.0);
                        let res_id = PCWSTR(32512usize as *const u16);
                        let cx_sm = GetSystemMetrics(SM_CXSMICON);
                        let cy_sm = GetSystemMetrics(SM_CYSMICON);
                        if let Ok(hicon_sm) = LoadImageW(
                            hinstance,
                            res_id,
                            IMAGE_ICON,
                            cx_sm,
                            cy_sm,
                            LR_DEFAULTCOLOR | LR_SHARED,
                        ) {
                            let _ = SendMessageW(
                                raw_hwnd,
                                WM_SETICON,
                                WPARAM(ICON_SMALL as usize),
                                LPARAM(hicon_sm.0 as isize),
                            );
                        }

                        let cx_big = GetSystemMetrics(SM_CXICON);
                        let cy_big = GetSystemMetrics(SM_CYICON);
                        if let Ok(hicon_big) = LoadImageW(
                            hinstance,
                            res_id,
                            IMAGE_ICON,
                            cx_big,
                            cy_big,
                            LR_DEFAULTCOLOR | LR_SHARED,
                        ) {
                            let _ = SendMessageW(
                                raw_hwnd,
                                WM_SETICON,
                                WPARAM(ICON_BIG as usize),
                                LPARAM(hicon_big.0 as isize),
                            );
                        }
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (window, is_dark);
        }
    }

    /// 保存当前标准窗口坐标与尺寸至 SQLite（忽略最小化或紧凑 Mini 状态，保留最大化标记）
    pub fn save_window_bounds(window: &WebviewWindow, db: &Database) {
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        if window.is_maximized().unwrap_or(false) {
            let mut prev = db
                .get_setting("window_bounds")
                .and_then(|s| serde_json::from_str::<SavedWindowBounds>(&s).ok())
                .unwrap_or(SavedWindowBounds {
                    x: 160,
                    y: 100,
                    width: 1020,
                    height: 690,
                    is_maximized: true,
                });
            prev.is_maximized = true;
            if let Ok(json_str) = serde_json::to_string(&prev) {
                let _ = db.set_setting("window_bounds", &json_str);
            }
            return;
        }

        let Ok(pos) = window.outer_position() else {
            return;
        };
        let Ok(size) = window.inner_size() else {
            return;
        };

        // Mini 模式尺寸不覆盖主窗口标准尺寸记忆
        if size.width < 600 || size.height < 400 {
            return;
        }

        let bounds = SavedWindowBounds {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            is_maximized: false,
        };
        if let Ok(json_str) = serde_json::to_string(&bounds) {
            let _ = db.set_setting("window_bounds", &json_str);
        }
    }

    /// 恢复窗口坐标并校验多显示器与高 DPI 边界（架构红线四.2：拔出副屏越界时自动居中主屏）
    pub fn restore_window_bounds(window: &WebviewWindow, db: &Database) {
        let Some(raw_json) = db.get_setting("window_bounds") else {
            return;
        };
        let Ok(bounds) = serde_json::from_str::<SavedWindowBounds>(&raw_json) else {
            return;
        };

        if bounds.width < 600 || bounds.height < 400 {
            return;
        }

        if Self::is_bounds_on_valid_workarea(&bounds) {
            let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
            let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
        } else {
            log_warn("检测到历史窗口坐标超出当前有效显示器工作区，已自动居中回主屏幕");
            let _ = window.set_size(PhysicalSize::new(1020, 690));
            let _ = window.center();
        }

        if bounds.is_maximized {
            let _ = window.maximize();
        }
    }

    /// 第 1 级休眠：隐藏渲染可见性并调用 WebView2 原生 `TrySuspend()` 挂起内存页（支持任务栏 0ms 秒开）
    pub fn suspend_webview(window: &WebviewWindow) {
        #[cfg(windows)]
        {
            let _ = window.with_webview(|wv| unsafe {
                let controller = wv.controller();
                let _ = controller.SetIsVisible(false);
                if let Ok(core) = controller.CoreWebView2() {
                    let raw_ptr: *mut core::ffi::c_void = core::mem::transmute_copy(&core);
                    wv2_suspend::try_suspend_or_resume(raw_ptr, true);
                }
            });
        }
        #[cfg(not(windows))]
        {
            let _ = window;
        }
    }

    /// 从第 1 级休眠瞬时唤醒 WebView2 渲染页
    pub fn resume_webview(window: &WebviewWindow) {
        #[cfg(windows)]
        {
            let _ = window.with_webview(|wv| unsafe {
                let controller = wv.controller();
                if let Ok(core) = controller.CoreWebView2() {
                    let raw_ptr: *mut core::ffi::c_void = core::mem::transmute_copy(&core);
                    wv2_suspend::try_suspend_or_resume(raw_ptr, false);
                }
                let _ = controller.SetIsVisible(true);
            });
        }
        #[cfg(not(windows))]
        {
            let _ = window;
        }
    }

    fn is_bounds_on_valid_workarea(bounds: &SavedWindowBounds) -> bool {
        #[cfg(windows)]
        {
            // 取标题栏中点校验是否落在有效显示器的工作区 rcWork 内
            let anchor = POINT {
                x: bounds.x + (bounds.width as i32) / 2,
                y: bounds.y + 24,
            };
            unsafe {
                let hmon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONULL);
                if hmon.0.is_null() {
                    return false;
                }
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
                    return false;
                }
                let work = mi.rcWork;
                anchor.x >= work.left
                    && anchor.x <= work.right
                    && anchor.y >= work.top
                    && anchor.y <= work.bottom
            }
        }
        #[cfg(not(windows))]
        {
            let _ = bounds;
            true
        }
    }
}
