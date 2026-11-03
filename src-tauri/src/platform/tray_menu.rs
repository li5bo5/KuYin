//! 原生 Win11 Fluent 风格托盘弹出菜单（1.0.4 重构版）
//!
//! 纯 Win32 弹出窗口 + DWM 原生圆角/阴影 + GDI 双缓冲自绘：
//! - 从上到下顺序：歌曲信息（带 40x40 圆角歌曲封面） -> 横向播放控制（上一首 / 暂停或播放 / 下一首）
//!   -> 显示主页面 -> 播放模式选择（向右自动展开二级子菜单，当前模式采用圆角背景+主题色加粗高亮，无勾号）
//!   -> 设置 -> 退出酷音；
//! - 菜单仅在弹出期间存在于独立线程，点击/失焦/Esc 后立即销毁，不驻留任何 WebView2 或常驻内存。

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetDC, GetMonitorInfoW, GetStockObject, GetTextFaceW, InvalidateRect, MonitorFromPoint,
    ReleaseDC, RoundRect, SelectClipRgn, SelectObject, SetBkMode, SetStretchBltMode, SetTextColor,
    StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY, DEFAULT_CHARSET,
    DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT,
    DT_SINGLELINE, DT_VCENTER, HALFTONE, HDC, HFONT, LOGFONTW, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, NULL_PEN, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_LBUTTON,
    VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetWindowRect, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    SetTimer, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST,
    IDC_ARROW, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE,
    WM_ACTIVATE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WM_MOUSELEAVE_ID: u32 = 0x02A3;
const OUTSIDE_CHECK_TIMER: usize = 1;

/// 防止重复右键弹出多个菜单
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// 弹出菜单所需的当前播放器状态
pub struct TrayMenuParams {
    pub is_dark: bool,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub is_playing: bool,
    pub mode: String,
    /// 当前曲目的 40x40 BGRA8 缩略图像素（长度 40 * 40 * 4 = 6400 字节）
    pub cover_bgra: Option<Vec<u8>>,
}

enum ItemKind {
    Header,
    TransportRow {
        is_playing: bool,
    },
    Separator,
    Action {
        id: &'static str,
        label: String,
        glyph: char,
    },
    SubMenuTrigger {
        label: String,
        glyph: char,
        mode_text: String,
    },
}

struct SubMenuItem {
    id: &'static str,
    label: &'static str,
    glyph: char,
    active: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverTarget {
    Item(usize),
    Transport(usize), // 0 = prev, 1 = play_pause, 2 = next
}

struct LayoutItem {
    kind: ItemKind,
    top: i32,
    height: i32,
}

struct Palette {
    bg: COLORREF,
    text: COLORREF,
    sub: COLORREF,
    hover: COLORREF,
    active_bg: COLORREF,
    transport_bg: COLORREF,
    sep: COLORREF,
    accent: COLORREF,
}

struct MenuState {
    hwnd_main: HWND,
    hwnd_sub: HWND,
    sub_visible: bool,
    items: Vec<LayoutItem>,
    sub_items: Vec<SubMenuItem>,
    sub_item_height: i32,
    sub_pad: i32,
    sub_width: i32,
    sub_height: i32,
    width: i32,
    height: i32,
    scale: f32,
    hover_main: Option<HoverTarget>,
    hover_sub: Option<usize>,
    tracking_main: bool,
    tracking_sub: bool,
    selected: Option<&'static str>,
    palette: Palette,
    title: String,
    subtitle: String,
    cover_bgra: Option<Vec<u8>>,
    font_text: HFONT,
    font_text_bold: HFONT,
    font_title: HFONT,
    font_sub: HFONT,
    font_icon: HFONT,
    font_icon_large: HFONT,
}

thread_local! {
    static MENU: RefCell<Option<MenuState>> = const { RefCell::new(None) };
    static CLOSING: Cell<bool> = const { Cell::new(false) };
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

unsafe fn create_font(face: &str, height_px: i32, weight: i32) -> HFONT {
    let mut lf = LOGFONTW {
        lfHeight: -height_px,
        lfWeight: weight,
        lfCharSet: DEFAULT_CHARSET,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (i, ch) in face.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = ch;
    }
    CreateFontIndirectW(&lf)
}

/// 优先使用 Win11 内置 Segoe Fluent Icons，不存在时回退 Win10 的 Segoe MDL2 Assets
unsafe fn create_icon_font(height_px: i32) -> HFONT {
    let fluent = create_font("Segoe Fluent Icons", height_px, 400);
    let hdc = GetDC(None);
    let prev = SelectObject(hdc, fluent);
    let mut face_buf = [0u16; 64];
    let len = GetTextFaceW(hdc, Some(&mut face_buf)).max(0) as usize;
    SelectObject(hdc, prev);
    ReleaseDC(None, hdc);
    let face = String::from_utf16_lossy(&face_buf[..len.min(64)]);
    if face
        .trim_end_matches('\0')
        .eq_ignore_ascii_case("Segoe Fluent Icons")
    {
        fluent
    } else {
        let _ = DeleteObject(fluent);
        create_font("Segoe MDL2 Assets", height_px, 400)
    }
}

fn mode_info(mode: &str) -> (char, &'static str) {
    match mode {
        "loop" => ('\u{E8EE}', "列表循环"),
        "single" => ('\u{E8ED}', "单曲循环"),
        "shuffle" => ('\u{E8B1}', "随机播放"),
        _ => ('\u{E8FD}', "顺序播放"),
    }
}

fn build_items(params: &TrayMenuParams) -> Vec<ItemKind> {
    let (m_glyph, m_label) = mode_info(&params.mode);
    vec![
        ItemKind::Header,
        ItemKind::TransportRow {
            is_playing: params.is_playing,
        },
        ItemKind::Separator,
        ItemKind::Action {
            id: "show",
            label: "显示主页面".to_string(),
            glyph: '\u{E8A7}',
        },
        ItemKind::SubMenuTrigger {
            label: "播放模式".to_string(),
            glyph: m_glyph,
            mode_text: m_label.to_string(),
        },
        ItemKind::Action {
            id: "settings",
            label: "设置".to_string(),
            glyph: '\u{E713}',
        },
        ItemKind::Separator,
        ItemKind::Action {
            id: "quit",
            label: "退出酷音".to_string(),
            glyph: '\u{E7E8}',
        },
    ]
}

fn build_sub_items(mode: &str) -> Vec<SubMenuItem> {
    vec![
        SubMenuItem {
            id: "mode_sequence",
            label: "顺序播放",
            glyph: '\u{E8FD}',
            active: mode == "sequence",
        },
        SubMenuItem {
            id: "mode_loop",
            label: "列表循环",
            glyph: '\u{E8EE}',
            active: mode == "loop",
        },
        SubMenuItem {
            id: "mode_single",
            label: "单曲循环",
            glyph: '\u{E8ED}',
            active: mode == "single",
        },
        SubMenuItem {
            id: "mode_shuffle",
            label: "随机播放",
            glyph: '\u{E8B1}',
            active: mode == "shuffle",
        },
    ]
}

fn transport_btn_rects(state: &MenuState, top: i32, height: i32) -> [RECT; 3] {
    let px = |v: i32| ((v as f32) * state.scale).round() as i32;
    let margin_x = px(8);
    let gap = px(5);
    let avail = state.width - margin_x * 2 - gap * 2;
    let btn_w = avail / 3;
    let btn_top = top + px(2);
    let btn_bottom = top + height - px(2);

    let l0 = margin_x;
    let l1 = l0 + btn_w + gap;
    let l2 = l1 + btn_w + gap;
    [
        RECT {
            left: l0,
            top: btn_top,
            right: l0 + btn_w,
            bottom: btn_bottom,
        },
        RECT {
            left: l1,
            top: btn_top,
            right: l1 + btn_w,
            bottom: btn_bottom,
        },
        RECT {
            left: l2,
            top: btn_top,
            right: state.width - margin_x,
            bottom: btn_bottom,
        },
    ]
}

fn hit_test_main(state: &MenuState, x: i32, y: i32) -> Option<HoverTarget> {
    for (idx, it) in state.items.iter().enumerate() {
        if y >= it.top && y < it.top + it.height {
            return match it.kind {
                ItemKind::TransportRow { .. } => {
                    let rects = transport_btn_rects(state, it.top, it.height);
                    for (b_idx, rc) in rects.iter().enumerate() {
                        if x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom {
                            return Some(HoverTarget::Transport(b_idx));
                        }
                    }
                    None
                }
                ItemKind::Action { .. } | ItemKind::SubMenuTrigger { .. } => {
                    Some(HoverTarget::Item(idx))
                }
                _ => None,
            };
        }
    }
    None
}

fn hit_test_sub(state: &MenuState, y: i32) -> Option<usize> {
    let rel_y = y - state.sub_pad;
    if rel_y < 0 {
        return None;
    }
    let idx = (rel_y / state.sub_item_height) as usize;
    if idx < state.sub_items.len() {
        Some(idx)
    } else {
        None
    }
}

unsafe fn show_submenu_next_to_trigger(state: &mut MenuState, trigger_idx: usize) {
    if state.hwnd_sub.0.is_null() {
        return;
    }
    let px = |v: i32| ((v as f32) * state.scale).round() as i32;
    let mut main_rc = RECT::default();
    if GetWindowRect(state.hwnd_main, &mut main_rc).is_err() {
        return;
    }
    let trigger_top = state
        .items
        .get(trigger_idx)
        .map(|it| it.top)
        .unwrap_or(0);

    let mut sub_x = main_rc.right + px(4);
    let mut sub_y = main_rc.top + trigger_top - state.sub_pad;

    let hmon = MonitorFromPoint(
        POINT {
            x: main_rc.left,
            y: main_rc.top,
        },
        MONITOR_DEFAULTTONEAREST,
    );
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let work = mi.rcWork;
        if sub_x + state.sub_width > work.right {
            sub_x = main_rc.left - state.sub_width - px(4);
        }
        if sub_y + state.sub_height > work.bottom {
            sub_y = work.bottom - state.sub_height;
        }
        if sub_y < work.top {
            sub_y = work.top;
        }
    }

    let _ = SetWindowPos(
        state.hwnd_sub,
        HWND_TOPMOST,
        sub_x,
        sub_y,
        state.sub_width,
        state.sub_height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(state.hwnd_sub, SW_SHOWNOACTIVATE);
    let _ = InvalidateRect(state.hwnd_sub, None, false);
    state.sub_visible = true;
}

unsafe fn hide_submenu(state: &mut MenuState) {
    if state.sub_visible && !state.hwnd_sub.0.is_null() {
        let _ = ShowWindow(state.hwnd_sub, SW_HIDE);
        state.sub_visible = false;
        state.hover_sub = None;
    }
}

unsafe fn close_menu() {
    if CLOSING.with(|c| c.replace(true)) {
        return;
    }
    let (hwnd_main, hwnd_sub) = MENU.with(|m| {
        if let Some(st) = m.borrow().as_ref() {
            (st.hwnd_main, st.hwnd_sub)
        } else {
            (HWND::default(), HWND::default())
        }
    });
    if !hwnd_sub.0.is_null() {
        let _ = DestroyWindow(hwnd_sub);
    }
    if !hwnd_main.0.is_null() {
        let _ = KillTimer(hwnd_main, OUTSIDE_CHECK_TIMER);
        let _ = DestroyWindow(hwnd_main);
    }
}

unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    rc: &mut RECT,
    font: HFONT,
    color: COLORREF,
    align_mode: DRAW_TEXT_FORMAT,
) {
    let prev = SelectObject(hdc, font);
    SetTextColor(hdc, color);
    let mut buf = to_wide(text);
    DrawTextW(
        hdc,
        &mut buf,
        rc,
        align_mode | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(hdc, prev);
}

unsafe fn paint_cover_bgra(
    hdc: HDC,
    x: i32,
    y: i32,
    size: i32,
    radius: i32,
    bgra: &[u8],
) -> bool {
    if bgra.len() < 40 * 40 * 4 {
        return false;
    }
    let rgn = CreateRoundRectRgn(x, y, x + size + 1, y + size + 1, radius, radius);
    SelectClipRgn(hdc, rgn);
    let _ = SetStretchBltMode(hdc, HALFTONE);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 40,
            biHeight: -40, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let copied = StretchDIBits(
        hdc,
        x,
        y,
        size,
        size,
        0,
        0,
        40,
        40,
        Some(bgra.as_ptr() as *const _),
        &bmi,
        DIB_RGB_COLORS,
        SRCCOPY,
    );

    SelectClipRgn(hdc, None);
    let _ = DeleteObject(rgn);
    copied > 0
}

unsafe fn paint_main(hwnd: HWND, state: &MenuState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let w = state.width;
    let h = state.height;
    let px = |v: i32| ((v as f32) * state.scale).round() as i32;

    let mem_dc = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem_dc, bmp);
    SetBkMode(mem_dc, TRANSPARENT);

    let p = &state.palette;
    let bg_brush = CreateSolidBrush(p.bg);
    FillRect(
        mem_dc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        },
        bg_brush,
    );
    let _ = DeleteObject(bg_brush);

    let null_pen = GetStockObject(NULL_PEN);
    let old_pen = SelectObject(mem_dc, null_pen);

    for (idx, item) in state.items.iter().enumerate() {
        let top = item.top;
        let bottom = item.top + item.height;
        match &item.kind {
            ItemKind::Header => {
                let badge = px(34);
                let bx = px(10);
                let by = top + (item.height - badge) / 2;

                let drew_cover = state
                    .cover_bgra
                    .as_deref()
                    .map(|bytes| paint_cover_bgra(mem_dc, bx, by, badge, px(8), bytes))
                    .unwrap_or(false);

                if !drew_cover {
                    let accent_brush = CreateSolidBrush(p.accent);
                    let old_brush = SelectObject(mem_dc, accent_brush);
                    let _ = RoundRect(mem_dc, bx, by, bx + badge, by + badge, px(8), px(8));
                    SelectObject(mem_dc, old_brush);
                    let _ = DeleteObject(accent_brush);
                    let mut icon_rc = RECT {
                        left: bx,
                        top: by,
                        right: bx + badge,
                        bottom: by + badge,
                    };
                    draw_text(
                        mem_dc,
                        "\u{EC4F}",
                        &mut icon_rc,
                        state.font_icon_large,
                        rgb(255, 255, 255),
                        DT_CENTER,
                    );
                }

                let text_left = bx + badge + px(9);
                let mid = top + item.height / 2;
                let mut title_rc = RECT {
                    left: text_left,
                    top: mid - px(18),
                    right: w - px(10),
                    bottom: mid,
                };
                draw_text(
                    mem_dc,
                    &state.title,
                    &mut title_rc,
                    state.font_title,
                    p.text,
                    DT_LEFT,
                );
                let mut sub_rc = RECT {
                    left: text_left,
                    top: mid,
                    right: w - px(10),
                    bottom: mid + px(16),
                };
                draw_text(
                    mem_dc,
                    &state.subtitle,
                    &mut sub_rc,
                    state.font_sub,
                    p.sub,
                    DT_LEFT,
                );
            }
            ItemKind::TransportRow { is_playing } => {
                let rects = transport_btn_rects(state, top, item.height);
                let btns: [(char, &str); 3] = [
                    ('\u{E892}', "上一首"),
                    (
                        if *is_playing { '\u{E769}' } else { '\u{E768}' },
                        if *is_playing { "暂停" } else { "播放" },
                    ),
                    ('\u{E893}', "下一首"),
                ];

                for (b_idx, (glyph, label)) in btns.iter().enumerate() {
                    let rc = rects[b_idx];
                    let is_hovered = state.hover_main == Some(HoverTarget::Transport(b_idx));
                    let fill_col = if is_hovered {
                        p.hover
                    } else {
                        p.transport_bg
                    };
                    let btn_brush = CreateSolidBrush(fill_col);
                    let old_brush = SelectObject(mem_dc, btn_brush);
                    let _ = RoundRect(mem_dc, rc.left, rc.top, rc.right, rc.bottom, px(7), px(7));
                    SelectObject(mem_dc, old_brush);
                    let _ = DeleteObject(btn_brush);

                    let fg = if b_idx == 1 { p.accent } else { p.text };
                    let mut icon_rc = RECT {
                        left: rc.left + px(5),
                        top: rc.top,
                        right: rc.left + px(21),
                        bottom: rc.bottom,
                    };
                    let mut glyph_buf = [0u8; 4];
                    draw_text(
                        mem_dc,
                        glyph.encode_utf8(&mut glyph_buf),
                        &mut icon_rc,
                        state.font_icon,
                        fg,
                        DT_CENTER,
                    );

                    let mut lbl_rc = RECT {
                        left: rc.left + px(21),
                        top: rc.top,
                        right: rc.right - px(3),
                        bottom: rc.bottom,
                    };
                    draw_text(
                        mem_dc,
                        label,
                        &mut lbl_rc,
                        if b_idx == 1 {
                            state.font_text_bold
                        } else {
                            state.font_text
                        },
                        fg,
                        DT_CENTER,
                    );
                }
            }
            ItemKind::Separator => {
                let sep_brush = CreateSolidBrush(p.sep);
                let y = top + item.height / 2;
                FillRect(
                    mem_dc,
                    &RECT {
                        left: px(10),
                        top: y,
                        right: w - px(10),
                        bottom: y + px(1).max(1),
                    },
                    sep_brush,
                );
                let _ = DeleteObject(sep_brush);
            }
            ItemKind::Action { label, glyph, .. } => {
                if state.hover_main == Some(HoverTarget::Item(idx)) {
                    let hover_brush = CreateSolidBrush(p.hover);
                    let old_brush = SelectObject(mem_dc, hover_brush);
                    let _ = RoundRect(
                        mem_dc,
                        px(5),
                        top + px(1),
                        w - px(5),
                        bottom - px(1),
                        px(7),
                        px(7),
                    );
                    SelectObject(mem_dc, old_brush);
                    let _ = DeleteObject(hover_brush);
                }
                let mut icon_rc = RECT {
                    left: px(12),
                    top,
                    right: px(34),
                    bottom,
                };
                let mut glyph_buf = [0u8; 4];
                draw_text(
                    mem_dc,
                    glyph.encode_utf8(&mut glyph_buf),
                    &mut icon_rc,
                    state.font_icon,
                    p.text,
                    DT_CENTER,
                );
                let mut label_rc = RECT {
                    left: px(38),
                    top,
                    right: w - px(12),
                    bottom,
                };
                draw_text(
                    mem_dc,
                    label,
                    &mut label_rc,
                    state.font_text,
                    p.text,
                    DT_LEFT,
                );
            }
            ItemKind::SubMenuTrigger {
                label,
                glyph,
                mode_text,
            } => {
                if state.hover_main == Some(HoverTarget::Item(idx)) || state.sub_visible {
                    let hover_brush = CreateSolidBrush(p.hover);
                    let old_brush = SelectObject(mem_dc, hover_brush);
                    let _ = RoundRect(
                        mem_dc,
                        px(5),
                        top + px(1),
                        w - px(5),
                        bottom - px(1),
                        px(7),
                        px(7),
                    );
                    SelectObject(mem_dc, old_brush);
                    let _ = DeleteObject(hover_brush);
                }
                let mut icon_rc = RECT {
                    left: px(12),
                    top,
                    right: px(34),
                    bottom,
                };
                let mut glyph_buf = [0u8; 4];
                draw_text(
                    mem_dc,
                    glyph.encode_utf8(&mut glyph_buf),
                    &mut icon_rc,
                    state.font_icon,
                    p.text,
                    DT_CENTER,
                );
                let mut label_rc = RECT {
                    left: px(38),
                    top,
                    right: w - px(94),
                    bottom,
                };
                draw_text(
                    mem_dc,
                    label,
                    &mut label_rc,
                    state.font_text,
                    p.text,
                    DT_LEFT,
                );

                let mut mode_rc = RECT {
                    left: w - px(96),
                    top,
                    right: w - px(26),
                    bottom,
                };
                draw_text(
                    mem_dc,
                    mode_text,
                    &mut mode_rc,
                    state.font_sub,
                    p.sub,
                    DT_RIGHT,
                );

                let mut arrow_rc = RECT {
                    left: w - px(25),
                    top,
                    right: w - px(9),
                    bottom,
                };
                draw_text(
                    mem_dc,
                    "\u{E76C}",
                    &mut arrow_rc,
                    state.font_icon,
                    p.sub,
                    DT_CENTER,
                );
            }
        }
    }

    SelectObject(mem_dc, old_pen);
    let _ = BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem_dc);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn paint_sub(hwnd: HWND, state: &MenuState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let w = state.sub_width;
    let h = state.sub_height;
    let px = |v: i32| ((v as f32) * state.scale).round() as i32;

    let mem_dc = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem_dc, bmp);
    SetBkMode(mem_dc, TRANSPARENT);

    let p = &state.palette;
    let bg_brush = CreateSolidBrush(p.bg);
    FillRect(
        mem_dc,
        &RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        },
        bg_brush,
    );
    let _ = DeleteObject(bg_brush);

    let null_pen = GetStockObject(NULL_PEN);
    let old_pen = SelectObject(mem_dc, null_pen);

    for (idx, sub_item) in state.sub_items.iter().enumerate() {
        let top = state.sub_pad + (idx as i32) * state.sub_item_height;
        let bottom = top + state.sub_item_height;
        let is_hover = state.hover_sub == Some(idx);

        // 与主界面下拉框保持一致：当前选中模式采用圆角背景高亮 + 主题色加粗文字（无勾号）
        if sub_item.active || is_hover {
            let bg_col = if sub_item.active {
                p.active_bg
            } else {
                p.hover
            };
            let brush = CreateSolidBrush(bg_col);
            let old_brush = SelectObject(mem_dc, brush);
            let _ = RoundRect(
                mem_dc,
                px(6),
                top + px(2),
                w - px(6),
                bottom - px(2),
                px(8),
                px(8),
            );
            SelectObject(mem_dc, old_brush);
            let _ = DeleteObject(brush);
        }

        let fg = if sub_item.active { p.accent } else { p.text };
        let font = if sub_item.active {
            state.font_text_bold
        } else {
            state.font_text
        };

        let mut icon_rc = RECT {
            left: px(10),
            top,
            right: px(32),
            bottom,
        };
        let mut glyph_buf = [0u8; 4];
        draw_text(
            mem_dc,
            sub_item.glyph.encode_utf8(&mut glyph_buf),
            &mut icon_rc,
            state.font_icon,
            fg,
            DT_CENTER,
        );

        let mut label_rc = RECT {
            left: px(36),
            top,
            right: w - px(10),
            bottom,
        };
        draw_text(mem_dc, sub_item.label, &mut label_rc, font, fg, DT_LEFT);
    }

    SelectObject(mem_dc, old_pen);
    let _ = BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem_dc);
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn main_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            MENU.with(|m| {
                if let Some(state) = m.borrow().as_ref() {
                    paint_main(hwnd, state);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let (changed, need_track) = MENU.with(|m| {
                let mut guard = m.borrow_mut();
                let Some(state) = guard.as_mut() else {
                    return (false, false);
                };
                let hit = hit_test_main(state, x, y);
                let changed = hit != state.hover_main;
                state.hover_main = hit;

                if let Some(HoverTarget::Item(idx)) = hit {
                    if matches!(state.items[idx].kind, ItemKind::SubMenuTrigger { .. }) {
                        if !state.sub_visible {
                            show_submenu_next_to_trigger(state, idx);
                        }
                    } else if state.sub_visible {
                        hide_submenu(state);
                    }
                } else if hit.is_some() && state.sub_visible {
                    hide_submenu(state);
                }

                let need_track = !state.tracking_main;
                state.tracking_main = true;
                (changed, need_track)
            });
            if need_track {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            if changed {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE_ID => {
            MENU.with(|m| {
                if let Some(state) = m.borrow_mut().as_mut() {
                    state.hover_main = None;
                    state.tracking_main = false;
                }
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let picked = MENU.with(|m| {
                let mut guard = m.borrow_mut();
                let state = guard.as_mut()?;
                match hit_test_main(state, x, y)? {
                    HoverTarget::Transport(0) => {
                        state.selected = Some("prev");
                        Some("prev")
                    }
                    HoverTarget::Transport(1) => {
                        state.selected = Some("play_pause");
                        Some("play_pause")
                    }
                    HoverTarget::Transport(2) => {
                        state.selected = Some("next");
                        Some("next")
                    }
                    HoverTarget::Item(idx) => {
                        if let ItemKind::Action { id, .. } = state.items[idx].kind {
                            state.selected = Some(id);
                            Some(id)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            });
            if picked.is_some() {
                close_menu();
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u16;
            if key == VK_ESCAPE.0 {
                close_menu();
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) == 0 {
                let other = HWND(lparam.0 as *mut _);
                let is_sub = MENU.with(|m| {
                    m.borrow()
                        .as_ref()
                        .map(|st| !st.hwnd_sub.0.is_null() && st.hwnd_sub == other)
                        .unwrap_or(false)
                });
                if !is_sub {
                    close_menu();
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let pressed = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0;
            if pressed {
                let mut pt = POINT::default();
                let mut rc_main = RECT::default();
                if GetCursorPos(&mut pt).is_ok() && GetWindowRect(hwnd, &mut rc_main).is_ok() {
                    let in_main = pt.x >= rc_main.left
                        && pt.x < rc_main.right
                        && pt.y >= rc_main.top
                        && pt.y < rc_main.bottom;
                    let in_sub = MENU.with(|m| {
                        let borrow = m.borrow();
                        let Some(st) = borrow.as_ref() else {
                            return false;
                        };
                        if !st.sub_visible || st.hwnd_sub.0.is_null() {
                            return false;
                        }
                        let mut rc_sub = RECT::default();
                        if GetWindowRect(st.hwnd_sub, &mut rc_sub).is_ok() {
                            pt.x >= rc_sub.left
                                && pt.x < rc_sub.right
                                && pt.y >= rc_sub.top
                                && pt.y < rc_sub.bottom
                        } else {
                            false
                        }
                    });
                    if !in_main && !in_sub {
                        close_menu();
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn sub_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            MENU.with(|m| {
                if let Some(state) = m.borrow().as_ref() {
                    paint_sub(hwnd, state);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let (changed, need_track) = MENU.with(|m| {
                let mut guard = m.borrow_mut();
                let Some(state) = guard.as_mut() else {
                    return (false, false);
                };
                let hit = hit_test_sub(state, y);
                let changed = hit != state.hover_sub;
                state.hover_sub = hit;
                let need_track = !state.tracking_sub;
                state.tracking_sub = true;
                (changed, need_track)
            });
            if need_track {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            if changed {
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE_ID => {
            MENU.with(|m| {
                if let Some(state) = m.borrow_mut().as_mut() {
                    state.hover_sub = None;
                    state.tracking_sub = false;
                }
            });
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let picked = MENU.with(|m| {
                let mut guard = m.borrow_mut();
                let state = guard.as_mut()?;
                let idx = hit_test_sub(state, y)?;
                let id = state.sub_items.get(idx)?.id;
                state.selected = Some(id);
                Some(id)
            });
            if picked.is_some() {
                close_menu();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 在当前线程阻塞弹出托盘菜单，返回用户选中的菜单项 id（未选择返回 `None`）
pub fn show_tray_menu(params: TrayMenuParams) -> Option<&'static str> {
    if MENU_OPEN.swap(true, Ordering::SeqCst) {
        return None;
    }
    let result = unsafe { run_menu(params) };
    MENU_OPEN.store(false, Ordering::SeqCst);
    result
}

unsafe fn run_menu(params: TrayMenuParams) -> Option<&'static str> {
    let scale = (GetDpiForSystem().max(96) as f32) / 96.0;
    let px = |v: i32| ((v as f32) * scale).round() as i32;

    let width = px(220);
    let pad = px(5);
    let mut y = pad;
    let mut items = Vec::new();
    for kind in build_items(&params) {
        let height = match kind {
            ItemKind::Header => px(52),
            ItemKind::TransportRow { .. } => px(36),
            ItemKind::Separator => px(7),
            ItemKind::Action { .. } | ItemKind::SubMenuTrigger { .. } => px(32),
        };
        items.push(LayoutItem {
            kind,
            top: y,
            height,
        });
        y += height;
    }
    let height = y + pad;

    let sub_items = build_sub_items(&params.mode);
    let sub_item_height = px(32);
    let sub_pad = px(5);
    let sub_width = px(128);
    let sub_height = sub_pad * 2 + (sub_items.len() as i32) * sub_item_height;

    let palette = if params.is_dark {
        Palette {
            bg: rgb(0x2B, 0x2B, 0x2B),
            text: rgb(0xFF, 0xFF, 0xFF),
            sub: rgb(0xC5, 0xC5, 0xC5),
            hover: rgb(0x3A, 0x3A, 0x3A),
            active_bg: rgb(0x22, 0x3D, 0x54),
            transport_bg: rgb(0x33, 0x33, 0x33),
            sep: rgb(0x40, 0x40, 0x40),
            accent: rgb(0x4C, 0xC2, 0xFF),
        }
    } else {
        Palette {
            bg: rgb(0xF9, 0xF9, 0xF9),
            text: rgb(0x1A, 0x1A, 0x1A),
            sub: rgb(0x60, 0x60, 0x60),
            hover: rgb(0xEA, 0xEA, 0xEA),
            active_bg: rgb(0xE1, 0xF0, 0xFF),
            transport_bg: rgb(0xF0, 0xF0, 0xF0),
            sep: rgb(0xE3, 0xE3, 0xE3),
            accent: rgb(0x00, 0x67, 0xC0),
        }
    };

    let title = params
        .title
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "酷音 KuYin".to_string());
    let subtitle = match params.subtitle.clone().filter(|s| !s.is_empty()) {
        Some(singer) => format!(
            "{singer} · {}",
            if params.is_playing {
                "正在播放"
            } else {
                "已暂停"
            }
        ),
        None => "暂无播放曲目".to_string(),
    };

    let hinstance = GetModuleHandleW(None).unwrap_or_default();
    let main_class = w!("KuYinFluentTrayMenu");
    let sub_class = w!("KuYinFluentTraySubMenu");

    let wc_main = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(main_wnd_proc),
        hInstance: hinstance.into(),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        lpszClassName: main_class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc_main);

    let wc_sub = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(sub_wnd_proc),
        hInstance: hinstance.into(),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        lpszClassName: sub_class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc_sub);

    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);
    let mut x = cursor.x - width;
    let mut top = cursor.y - height;
    let hmon = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let work = mi.rcWork;
        if x < work.left {
            x = cursor.x.max(work.left);
        }
        if x + width > work.right {
            x = work.right - width;
        }
        if top < work.top {
            top = cursor.y.max(work.top);
        }
        if top + height > work.bottom {
            top = work.bottom - height;
        }
    }

    let state = MenuState {
        hwnd_main: HWND::default(),
        hwnd_sub: HWND::default(),
        sub_visible: false,
        items,
        sub_items,
        sub_item_height,
        sub_pad,
        sub_width,
        sub_height,
        width,
        height,
        scale,
        hover_main: None,
        hover_sub: None,
        tracking_main: false,
        tracking_sub: false,
        selected: None,
        palette,
        title,
        subtitle,
        cover_bgra: params.cover_bgra,
        font_text: create_font("Microsoft YaHei UI", px(13), 400),
        font_text_bold: create_font("Microsoft YaHei UI", px(13), 600),
        font_title: create_font("Microsoft YaHei UI", px(14), 600),
        font_sub: create_font("Microsoft YaHei UI", px(11), 400),
        font_icon: create_icon_font(px(13)),
        font_icon_large: create_icon_font(px(16)),
    };
    MENU.with(|m| *m.borrow_mut() = Some(state));
    CLOSING.with(|c| c.set(false));

    let hwnd_main = match CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        main_class,
        PCWSTR::null(),
        WS_POPUP,
        x,
        top,
        width,
        height,
        None,
        None,
        hinstance,
        None,
    ) {
        Ok(h) => h,
        Err(_) => {
            cleanup_state();
            return None;
        }
    };

    let hwnd_sub = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        sub_class,
        PCWSTR::null(),
        WS_POPUP,
        0,
        0,
        sub_width,
        sub_height,
        hwnd_main,
        None,
        hinstance,
        None,
    )
    .unwrap_or_default();

    MENU.with(|m| {
        if let Some(st) = m.borrow_mut().as_mut() {
            st.hwnd_main = hwnd_main;
            st.hwnd_sub = hwnd_sub;
        }
    });

    let corner = DWMWCP_ROUND;
    let dark: u32 = u32::from(params.is_dark);
    for h in [hwnd_main, hwnd_sub] {
        if !h.0.is_null() {
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as _,
                std::mem::size_of_val(&corner) as u32,
            );
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    let _ = ShowWindow(hwnd_main, SW_SHOW);
    let _ = SetForegroundWindow(hwnd_main);
    SetTimer(hwnd_main, OUTSIDE_CHECK_TIMER, 80, None);

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    cleanup_state()
}

unsafe fn cleanup_state() -> Option<&'static str> {
    let state = MENU.with(|m| m.borrow_mut().take())?;
    for font in [
        state.font_text,
        state.font_text_bold,
        state.font_title,
        state.font_sub,
        state.font_icon,
        state.font_icon_large,
    ] {
        let _ = DeleteObject(font);
    }
    state.selected
}
