#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};

use windows::Win32::Foundation::{
    COLORREF, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    FillRect, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SRCCOPY,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, AppendMenuW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, HCURSOR, HICON, HMENU, IDC_ARROW, IDC_HAND, IMAGE_ICON,
    KillTimer, LR_SHARED, LoadCursorW, LoadImageW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    MB_SETFOREGROUND, MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassExW,
    SM_CXICON, SM_CXSCREEN, SM_CXSMICON, SM_CYSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SETICON, WM_TIMER,
    WNDCLASS_STYLES, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{PCWSTR, w};

use crate::LanLink;

const CLASS_NAME: PCWSTR = w!("PhoneMouseWindow");
const WM_PHONE: u32 = 0x8000 + 1;
const WM_TRAY: u32 = 0x8000 + 2;
const WM_MOUSELEAVE: u32 = 675;
const WM_DPICHANGED: u32 = 0x02E0;
const WM_SYSCOMMAND: u32 = 0x0112;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_ENDSESSION: u32 = 0x0016;
const SC_MINIMIZE: usize = 0xF020;
const SC_CLOSE: usize = 0xF060;
const ID_TIMER: usize = 1;
const CF_UNICODETEXT: u32 = 13;
const TRAY_ID: u32 = 1;
const NIM_ADD: u32 = 0;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;

const BG: u32 = 0x001E1F20;
const FIELD: u32 = 0x00181A1C;
const FIELD_BORDER: u32 = 0x00303C44;
const TEXT: u32 = 0x00E6EFF4;
const MUTED: u32 = 0x008A949A;
const AMBER: u32 = 0x002AA2E8;
const AMBER_HOVER: u32 = 0x0040B0F2;
const AMBER_PRESS: u32 = 0x001C92D6;
const INK: u32 = 0x000A1820;
const QUIT: u32 = 0x0027292A;
const QUIT_BORDER: u32 = 0x00565A5C;
const GREEN: u32 = 0x0068BA78;
const GREEN_DIM: u32 = 0x0058A86C;

static PHONES: AtomicI32 = AtomicI32::new(0);
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static TRAY: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct NotifyIconData {
    cb_size: u32,
    hwnd: HWND,
    id: u32,
    flags: u32,
    callback: u32,
    icon: HICON,
    tip: [u16; 128],
}

const _: () = assert!(size_of::<NotifyIconData>() == 296);

#[link(name = "shell32")]
unsafe extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIconData) -> i32;
}

unsafe extern "system" {
    fn TrackPopupMenu(
        menu: HMENU,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: HWND,
        rect: *const RECT,
    ) -> i32;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hot {
    None,
    Copy,
    Quit,
}

struct Ui {
    pin: String,
    lines: Vec<String>,
    copy_text: String,
    phones: i32,
    copied: bool,
    hover: Hot,
    pressed: Hot,
    tracking: bool,
    dpi: u32,
    font: HFONT,
    font_small: HFONT,
    font_pin: HFONT,
    font_button: HFONT,
}

struct Layout {
    width: i32,
    height: i32,
    address: RECT,
    fields: Vec<RECT>,
    code: RECT,
    pin: RECT,
    status: RECT,
    copy: RECT,
    quit: RECT,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

pub fn enable_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        message_box(&format!("PhoneMouse s'est arrêté.\n\n{info}"), true);
    }));
}

pub fn message_box(text: &str, error: bool) {
    let text = wide(text);
    let title = wide("PhoneMouse");
    let flags = MB_OK
        | MB_SETFOREGROUND
        | if error {
            MB_ICONERROR
        } else {
            MB_ICONINFORMATION
        };
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(text.as_ptr()),
            PCWSTR::from_raw(title.as_ptr()),
            flags,
        );
    }
}

pub fn phone_connected(delta: i32) {
    let mut current = PHONES.load(Ordering::Relaxed);
    let next = loop {
        let next = (current + delta).max(0);
        match PHONES.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => break next,
            Err(actual) => current = actual,
        }
    };
    let raw = WINDOW.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as *mut core::ffi::c_void)),
                WM_PHONE,
                WPARAM(next as usize),
                LPARAM(0),
            );
        }
    }
}

pub fn run(pin: &str, links: &[LanLink]) {
    unsafe { show(pin, links) }
}

unsafe fn show(pin: &str, links: &[LanLink]) {
    let instance = match GetModuleHandleW(None) {
        Ok(module) => HINSTANCE::from(module),
        Err(_) => {
            message_box("Impossible d'ouvrir la fenêtre.", true);
            return;
        }
    };
    let big = load_icon(instance, GetSystemMetrics(SM_CXICON).max(32));
    let small = load_icon(instance, GetSystemMetrics(SM_CXSMICON).max(16));
    let cursor = match LoadCursorW(None, IDC_ARROW) {
        Ok(cursor) => cursor,
        Err(_) => {
            message_box("Impossible d'ouvrir la fenêtre.", true);
            return;
        }
    };
    let background = CreateSolidBrush(COLORREF(BG));
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_REDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: big,
        hCursor: cursor,
        hbrBackground: background,
        lpszMenuName: PCWSTR::null(),
        lpszClassName: CLASS_NAME,
        hIconSm: small,
    };
    if RegisterClassExW(&class) == 0 {
        message_box("Impossible d'ouvrir la fenêtre.", true);
        return;
    }

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE(0),
        CLASS_NAME,
        w!("PhoneMouse"),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        520,
        460,
        None,
        None,
        Some(instance),
        None,
    ) {
        Ok(hwnd) => hwnd,
        Err(_) => {
            message_box("Impossible d'ouvrir la fenêtre.", true);
            return;
        }
    };

    let dark = 1i32;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const i32 as *const _,
        size_of::<i32>() as u32,
    );
    if !big.0.is_null() {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(1)),
            Some(LPARAM(big.0 as isize)),
        );
    }
    if !small.0.is_null() {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(0)),
            Some(LPARAM(small.0 as isize)),
        );
    }

    let dpi = GetDpiForWindow(hwnd).max(GetDpiForSystem()).max(96);
    let lines: Vec<String> = if links.is_empty() {
        vec!["Aucune adresse locale. Vérifie le Wi-Fi.".into()]
    } else if links.len() == 1 {
        vec![links[0].url.clone()]
    } else {
        links
            .iter()
            .map(|link| format!("{}   {}", link.url, link.name))
            .collect()
    };
    let copy_text = links
        .iter()
        .map(|link| link.url.clone())
        .collect::<Vec<_>>()
        .join("\r\n");
    let fonts = make_fonts(dpi);
    UI.with(|ui| {
        *ui.borrow_mut() = Some(Ui {
            pin: pin.to_string(),
            lines,
            copy_text,
            phones: PHONES.load(Ordering::Acquire),
            copied: false,
            hover: Hot::None,
            pressed: Hot::None,
            tracking: false,
            dpi,
            font: fonts.0,
            font_small: fonts.1,
            font_pin: fonts.2,
            font_button: fonts.3,
        });
    });
    WINDOW.store(hwnd.0 as isize, Ordering::Release);
    add_tray(hwnd, if small.0.is_null() { big } else { small });

    let placed = place(dpi, links.len().max(1));
    fit_window(hwnd, placed.width, placed.height, style);
    let _ = ShowWindow(hwnd, SW_SHOW);

    let mut message = MSG::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

fn tray_data(hwnd: HWND, icon: HICON) -> NotifyIconData {
    let mut tip = [0u16; 128];
    for (slot, unit) in "PhoneMouse".encode_utf16().enumerate() {
        tip[slot] = unit;
    }
    NotifyIconData {
        cb_size: size_of::<NotifyIconData>() as u32,
        hwnd,
        id: TRAY_ID,
        flags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        callback: WM_TRAY,
        icon,
        tip,
    }
}

fn add_tray(hwnd: HWND, icon: HICON) {
    if icon.0.is_null() {
        return;
    }
    let data = tray_data(hwnd, icon);
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) != 0 };
    TRAY.store(added, Ordering::Release);
}

fn remove_tray(hwnd: HWND) {
    if !TRAY.swap(false, Ordering::AcqRel) {
        return;
    }
    let data = tray_data(hwnd, HICON(std::ptr::null_mut()));
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn hide_to_tray(hwnd: HWND) -> bool {
    if !TRAY.load(Ordering::Acquire) {
        return false;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    true
}

fn reveal_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let open = wide("Ouvrir");
        let quit = wide("Quitter");
        let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR::from_raw(open.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, 2, PCWSTR::from_raw(quit.as_ptr()));
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = SetForegroundWindow(hwnd);
        let choice = TrackPopupMenu(menu, 0x0180, cursor.x, cursor.y, 0, hwnd, std::ptr::null());
        let _ = DestroyMenu(menu);
        match choice {
            1 => reveal_window(hwnd),
            2 => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

fn load_icon(instance: HINSTANCE, size: i32) -> HICON {
    unsafe {
        match LoadImageW(
            Some(instance),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            size,
            size,
            LR_SHARED,
        ) {
            Ok(handle) => HICON(handle.0),
            Err(_) => HICON(std::ptr::null_mut()),
        }
    }
}

fn make_fonts(dpi: u32) -> (HFONT, HFONT, HFONT, HFONT) {
    (
        font(-px(dpi, 15), 400),
        font(-px(dpi, 13), 400),
        font(-px(dpi, 40), 600),
        font(-px(dpi, 15), 600),
    )
}

fn font(height: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    }
}

fn px(dpi: u32, value: i32) -> i32 {
    value * dpi as i32 / 96
}

fn place(dpi: u32, fields: usize) -> Layout {
    let width = px(dpi, 480);
    let pad = px(dpi, 28);
    let mut y = px(dpi, 22);
    let address = RECT {
        left: pad,
        top: y,
        right: width - pad,
        bottom: y + px(dpi, 20),
    };
    y += px(dpi, 26);
    let mut field_rects = Vec::with_capacity(fields);
    for _ in 0..fields {
        field_rects.push(RECT {
            left: pad,
            top: y,
            right: width - pad,
            bottom: y + px(dpi, 44),
        });
        y += px(dpi, 52);
    }
    y += px(dpi, 8);
    let code = RECT {
        left: pad,
        top: y,
        right: width - pad,
        bottom: y + px(dpi, 18),
    };
    y += px(dpi, 20);
    let pin = RECT {
        left: pad,
        top: y,
        right: width - pad,
        bottom: y + px(dpi, 58),
    };
    y += px(dpi, 60);
    let status = RECT {
        left: pad,
        top: y,
        right: width - pad,
        bottom: y + px(dpi, 24),
    };
    y += px(dpi, 36);
    let button_w = (width - pad * 2 - px(dpi, 12)) / 2;
    let copy = RECT {
        left: pad,
        top: y,
        right: pad + button_w,
        bottom: y + px(dpi, 46),
    };
    let quit = RECT {
        left: copy.right + px(dpi, 12),
        top: y,
        right: width - pad,
        bottom: y + px(dpi, 46),
    };
    y += px(dpi, 46 + 24);
    Layout {
        width,
        height: y,
        address,
        fields: field_rects,
        code,
        pin,
        status,
        copy,
        quit,
    }
}

fn fit_window(hwnd: HWND, client_w: i32, client_h: i32, style: WINDOW_STYLE) {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        if AdjustWindowRectEx(&raw mut rect, style, false, WINDOW_EX_STYLE(0)).is_err() {
            return;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let _ = SetWindowPos(
            hwnd,
            None,
            (screen_w - width) / 2,
            (screen_h - height) / 2,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_PHONE => {
            UI.with(|ui| {
                if let Some(ui) = ui.borrow_mut().as_mut() {
                    ui.phones = wparam.0 as i32;
                }
            });
            let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            LRESULT(0)
        }
        WM_SYSCOMMAND => {
            let command = wparam.0 & 0xFFF0;
            if (command == SC_MINIMIZE || command == SC_CLOSE) && hide_to_tray(hwnd) {
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            if hide_to_tray(hwnd) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_TRAY => match lparam.0 as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                reveal_window(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                tray_menu(hwnd);
                LRESULT(0)
            }
            _ => LRESULT(0),
        },
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                remove_tray(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            let mut changed = false;
            UI.with(|slot| {
                let mut ui = slot.borrow_mut();
                let Some(ui) = ui.as_mut() else {
                    return;
                };
                if !ui.tracking {
                    track_leave(hwnd);
                    ui.tracking = true;
                }
                let hot = hit(ui, x, y);
                if hot != ui.hover {
                    ui.hover = hot;
                    changed = true;
                }
            });
            if changed {
                let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            UI.with(|slot| {
                if let Some(ui) = slot.borrow_mut().as_mut() {
                    ui.tracking = false;
                    ui.hover = Hot::None;
                    ui.pressed = Hot::None;
                }
            });
            let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point(lparam);
            UI.with(|slot| {
                if let Some(ui) = slot.borrow_mut().as_mut() {
                    ui.pressed = hit(ui, x, y);
                }
            });
            let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point(lparam);
            let mut action = Hot::None;
            UI.with(|slot| {
                if let Some(ui) = slot.borrow_mut().as_mut() {
                    let hot = hit(ui, x, y);
                    if hot == ui.pressed {
                        action = hot;
                    }
                    ui.pressed = Hot::None;
                }
            });
            match action {
                Hot::Copy => copy_clicked(hwnd),
                Hot::Quit => {
                    let _ = DestroyWindow(hwnd);
                }
                Hot::None => {}
            }
            let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let hot = UI.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map(|ui| ui.hover)
                    .unwrap_or(Hot::None)
            });
            if hot != Hot::None {
                SetCursor(Some(cursor(IDC_HAND)));
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_TIMER => {
            if wparam.0 == ID_TIMER {
                let _ = KillTimer(Some(hwnd), ID_TIMER);
                UI.with(|slot| {
                    if let Some(ui) = slot.borrow_mut().as_mut() {
                        ui.copied = false;
                    }
                });
                let _ = InvalidateRect(hwnd_opt(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = ((wparam.0 >> 16) & 0xffff) as u32;
            let suggested = lparam.0 as *const RECT;
            if dpi >= 96 && !suggested.is_null() {
                let fonts = make_fonts(dpi);
                UI.with(|slot| {
                    if let Some(ui) = slot.borrow_mut().as_mut() {
                        delete_font(ui.font);
                        delete_font(ui.font_small);
                        delete_font(ui.font_pin);
                        delete_font(ui.font_button);
                        ui.dpi = dpi;
                        ui.font = fonts.0;
                        ui.font_small = fonts.1;
                        ui.font_pin = fonts.2;
                        ui.font_button = fonts.3;
                    }
                });
                let rect = unsafe { *suggested };
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray(hwnd);
            if WINDOW.load(Ordering::Acquire) == hwnd.0 as isize {
                WINDOW.store(0, Ordering::Release);
            }
            UI.with(|slot| {
                if let Some(ui) = slot.borrow_mut().take() {
                    delete_font(ui.font);
                    delete_font(ui.font_small);
                    delete_font(ui.font_pin);
                    delete_font(ui.font_button);
                }
            });
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn paint(hwnd: HWND) {
    let snap = UI.with(|slot| {
        slot.borrow().as_ref().map(|ui| Snapshot {
            pin: ui.pin.clone(),
            lines: ui.lines.clone(),
            phones: ui.phones,
            copied: ui.copied,
            hover: ui.hover,
            pressed: ui.pressed,
            dpi: ui.dpi,
            font: ui.font,
            font_small: ui.font_small,
            font_pin: ui.font_pin,
            font_button: ui.font_button,
        })
    });
    let Some(snap) = snap else {
        return;
    };
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let width = (client.right - client.left).max(1);
        let height = (client.bottom - client.top).max(1);
        let memory = CreateCompatibleDC(Some(hdc));
        let bitmap = CreateCompatibleBitmap(hdc, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        draw(&snap, memory, width, height);
        let _ = BitBlt(hdc, 0, 0, width, height, Some(memory), 0, 0, SRCCOPY);
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        let _ = EndPaint(hwnd, &paint);
    }
}

struct Snapshot {
    pin: String,
    lines: Vec<String>,
    phones: i32,
    copied: bool,
    hover: Hot,
    pressed: Hot,
    dpi: u32,
    font: HFONT,
    font_small: HFONT,
    font_pin: HFONT,
    font_button: HFONT,
}

fn draw(snap: &Snapshot, hdc: HDC, width: i32, height: i32) {
    let placed = place(snap.dpi, snap.lines.len().max(1));
    fill(
        hdc,
        RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        COLORREF(BG),
    );
    text(
        hdc,
        "Adresse",
        placed.address,
        snap.font_small,
        COLORREF(MUTED),
        DT_SINGLELINE | DT_VCENTER,
    );
    for (index, line) in snap.lines.iter().enumerate() {
        let Some(field) = placed.fields.get(index) else {
            break;
        };
        round(
            hdc,
            *field,
            px(snap.dpi, 8),
            COLORREF(FIELD),
            COLORREF(FIELD_BORDER),
        );
        text(
            hdc,
            line,
            inset(*field, px(snap.dpi, 14), 0),
            snap.font,
            COLORREF(TEXT),
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
    }
    text(
        hdc,
        "CODE",
        placed.code,
        snap.font_small,
        COLORREF(MUTED),
        DT_SINGLELINE | DT_VCENTER,
    );
    text(
        hdc,
        &snap.pin,
        placed.pin,
        snap.font_pin,
        COLORREF(AMBER),
        DT_SINGLELINE | DT_VCENTER,
    );
    let (status, color) = if snap.phones > 0 {
        (
            if snap.phones == 1 {
                "Téléphone relié".to_string()
            } else {
                format!("{} téléphones reliés", snap.phones)
            },
            GREEN,
        )
    } else {
        ("Téléphone en attente".to_string(), GREEN_DIM)
    };
    text(
        hdc,
        &status,
        placed.status,
        snap.font_small,
        COLORREF(color),
        DT_SINGLELINE | DT_VCENTER,
    );

    let copy_fill = button_fill(Hot::Copy, snap, AMBER, AMBER_HOVER, AMBER_PRESS);
    round(
        hdc,
        placed.copy,
        px(snap.dpi, 8),
        COLORREF(copy_fill),
        COLORREF(copy_fill),
    );
    let copy_label = if snap.copied { "Copié" } else { "Copier" };
    text(
        hdc,
        copy_label,
        placed.copy,
        snap.font_button,
        COLORREF(INK),
        DT_SINGLELINE | DT_CENTER | DT_VCENTER,
    );

    let quit_hot = snap.hover == Hot::Quit || snap.pressed == Hot::Quit;
    let quit_fill = if quit_hot { 0x00322F2C } else { QUIT };
    round(
        hdc,
        placed.quit,
        px(snap.dpi, 8),
        COLORREF(quit_fill),
        COLORREF(QUIT_BORDER),
    );
    text(
        hdc,
        "Quitter",
        placed.quit,
        snap.font_button,
        COLORREF(TEXT),
        DT_SINGLELINE | DT_CENTER | DT_VCENTER,
    );
}

fn button_fill(which: Hot, snap: &Snapshot, base: u32, hover: u32, pressed: u32) -> u32 {
    if snap.pressed == which && snap.hover == which {
        pressed
    } else if snap.hover == which {
        hover
    } else {
        base
    }
}

fn fill(hdc: HDC, rect: RECT, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

fn round(hdc: HDC, rect: RECT, radius: i32, fill_color: COLORREF, border: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(fill_color);
        let pen = CreatePen(PS_SOLID, 1, border);
        let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
        let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius * 2,
            radius * 2,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

fn text(
    hdc: HDC,
    value: &str,
    mut rect: RECT,
    font: HFONT,
    color: COLORREF,
    format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    if value.is_empty() || font.0.is_null() {
        return;
    }
    let mut chars: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        SetTextColor(hdc, color);
        SetBkMode(hdc, TRANSPARENT);
        DrawTextW(hdc, &mut chars, &mut rect, format);
        SelectObject(hdc, old);
    }
}

fn inset(rect: RECT, dx: i32, dy: i32) -> RECT {
    RECT {
        left: rect.left + dx,
        top: rect.top + dy,
        right: rect.right - dx,
        bottom: rect.bottom - dy,
    }
}

fn hit(ui: &Ui, x: i32, y: i32) -> Hot {
    let placed = place(ui.dpi, ui.lines.len().max(1));
    if contains(placed.copy, x, y) {
        Hot::Copy
    } else if contains(placed.quit, x, y) {
        Hot::Quit
    } else {
        Hot::None
    }
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 as i16) as i32;
    let y = ((lparam.0 >> 16) as i16) as i32;
    (x, y)
}

fn copy_clicked(hwnd: HWND) {
    let text = UI.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|ui| ui.copy_text.clone())
            .unwrap_or_default()
    });
    if text.is_empty() || !copy_to_clipboard(&text) {
        return;
    }
    UI.with(|slot| {
        if let Some(ui) = slot.borrow_mut().as_mut() {
            ui.copied = true;
        }
    });
    unsafe {
        SetTimer(Some(hwnd), ID_TIMER, 1400, None);
    }
}

fn track_leave(hwnd: HWND) {
    unsafe {
        let mut event = TrackMouse {
            cb_size: size_of::<TrackMouse>() as u32,
            flags: 2,
            hwnd,
            hover_time: 0,
        };
        TrackMouseEvent(&mut event);
    }
}

fn cursor(id: PCWSTR) -> HCURSOR {
    unsafe { LoadCursorW(None, id).unwrap_or(HCURSOR(std::ptr::null_mut())) }
}

fn hwnd_opt(hwnd: HWND) -> Option<HWND> {
    Some(hwnd)
}

fn delete_font(font: HFONT) {
    if !font.0.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

fn copy_to_clipboard(text: &str) -> bool {
    let wide_text = wide(text);
    let bytes = wide_text.len() * size_of::<u16>();
    unsafe {
        let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
            return false;
        };
        let ptr = GlobalLock(memory);
        if ptr.is_null() {
            free_global(memory);
            return false;
        }
        std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr.cast::<u16>(), wide_text.len());
        let _ = GlobalUnlock(memory);
        if OpenClipboard(None).is_err() {
            free_global(memory);
            return false;
        }
        let _ = EmptyClipboard();
        let copied = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))).is_ok();
        let _ = CloseClipboard();
        if !copied {
            free_global(memory);
        }
        copied
    }
}

unsafe fn free_global(memory: HGLOBAL) {
    unsafe { GlobalFree(memory.0) };
}

unsafe extern "system" {
    fn GlobalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn TrackMouseEvent(event: *mut TrackMouse) -> i32;
}

#[repr(C)]
struct TrackMouse {
    cb_size: u32,
    flags: u32,
    hwnd: HWND,
    hover_time: u32,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;

const CS_REDRAW: WNDCLASS_STYLES = WNDCLASS_STYLES(CS_HREDRAW.0 | CS_VREDRAW.0);

use windows::Win32::Graphics::Gdi::InvalidateRect;
