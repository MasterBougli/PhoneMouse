#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET,
    DeleteObject, GetSysColorBrush, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetMessageW, GetSystemMetrics, HICON, HMENU, IDC_ARROW, IDI_APPLICATION,
    KillTimer, LoadCursorW, LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
    MSG, MessageBoxW, PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
    SWP_NOZORDER, SendMessageW, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_TIMER,
    WNDCLASS_STYLES, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use crate::LanLink;

const CLASS_NAME: PCWSTR = w!("PhoneMouseWindow");
const ID_COPY: usize = 101;
const ID_QUIT: usize = 102;
const ID_TIMER: usize = 1;
const SS_CENTER: u32 = 0x0001;
const SS_CENTERIMAGE: u32 = 0x0200;
const ES_CENTER: u32 = 0x0001;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_READONLY: u32 = 0x0800;
const CF_UNICODETEXT: u32 = 13;

struct Ui {
    copy_text: String,
    copy_button: HWND,
    copy_label: String,
    window: HWND,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static FONTS: RefCell<Vec<HFONT>> = const { RefCell::new(Vec::new()) };
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
    let icon = LoadIconW(None, IDI_APPLICATION).unwrap_or(HICON(std::ptr::null_mut()));
    let cursor = match LoadCursorW(None, IDC_ARROW) {
        Ok(cursor) => cursor,
        Err(_) => {
            message_box("Impossible d'ouvrir la fenêtre.", true);
            return;
        }
    };
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: WNDCLASS_STYLES(0),
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: icon,
        hCursor: cursor,
        hbrBackground: GetSysColorBrush(COLOR_WINDOW),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: CLASS_NAME,
        hIconSm: HICON(std::ptr::null_mut()),
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
        480,
        420,
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

    let dpi = GetDpiForSystem().max(96);
    let scale = |px: i32| px * dpi as i32 / 96;
    let font = make_font(-scale(16), 400);
    let small = make_font(-scale(13), 400);
    let big = make_font(-scale(34), 600);
    remember_font(font);
    remember_font(small);
    remember_font(big);

    let pad = scale(20);
    let width = scale(440);
    let inner = width - pad * 2;
    let mut y = scale(16);

    text_control(
        instance,
        hwnd,
        "Ouvre cette adresse sur le téléphone",
        pad,
        y,
        inner,
        scale(22),
        font,
        0,
    );
    y += scale(28);

    let copy_text = if links.is_empty() {
        text_control(
            instance,
            hwnd,
            "Aucune adresse locale. Vérifie que le PC est connecté au Wi-Fi.",
            pad,
            y,
            inner,
            scale(44),
            font,
            0,
        );
        y += scale(52);
        String::new()
    } else {
        for link in links {
            edit_control(instance, hwnd, &link.url, pad, y, inner, scale(30), font);
            y += scale(34);
            text_control(
                instance,
                hwnd,
                &link.name,
                pad,
                y,
                inner,
                scale(18),
                small,
                0,
            );
            y += scale(26);
        }
        links
            .iter()
            .map(|link| link.url.clone())
            .collect::<Vec<_>>()
            .join("\r\n")
    };

    y += scale(6);
    text_control(
        instance,
        hwnd,
        "Code",
        pad,
        y,
        inner,
        scale(20),
        font,
        SS_CENTER,
    );
    y += scale(22);
    text_control(
        instance,
        hwnd,
        pin,
        pad,
        y,
        inner,
        scale(52),
        big,
        SS_CENTER | SS_CENTERIMAGE,
    );
    y += scale(60);
    text_control(
        instance,
        hwnd,
        "Tu peux réduire cette fenêtre. Ferme-la pour quitter. Si le téléphone ne s'ouvre pas, autorise PhoneMouse dans le pare-feu, sur le réseau privé.",
        pad,
        y,
        inner,
        scale(72),
        small,
        0,
    );
    y += scale(84);

    let copy_label = if links.len() > 1 {
        "Copier les liens"
    } else {
        "Copier le lien"
    };
    let copy_button = button(
        instance,
        hwnd,
        copy_label,
        pad,
        y,
        scale(170),
        scale(36),
        font,
        ID_COPY as isize,
    );
    button(
        instance,
        hwnd,
        "Quitter",
        width - pad - scale(110),
        y,
        scale(110),
        scale(36),
        font,
        ID_QUIT as isize,
    );
    y += scale(36 + 18);

    UI.with(|ui| {
        *ui.borrow_mut() = Some(Ui {
            copy_text,
            copy_button,
            copy_label: copy_label.to_string(),
            window: hwnd,
        });
    });

    fit_window(hwnd, width, y, style);
    let _ = ShowWindow(hwnd, SW_SHOW);

    let mut message = MSG::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
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
            SWP_NOZORDER,
        );
    }
}

fn text_control(
    instance: HINSTANCE,
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    font: HFONT,
    extra: u32,
) {
    let _ = child(
        w!("STATIC"),
        text,
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(extra),
        WINDOW_EX_STYLE(0),
        x,
        y,
        width,
        height,
        parent,
        0,
        instance,
        font,
    );
}

fn edit_control(
    instance: HINSTANCE,
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    font: HFONT,
) {
    let _ = child(
        w!("EDIT"),
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_CENTER | ES_AUTOHSCROLL | ES_READONLY),
        WS_EX_CLIENTEDGE,
        x,
        y,
        width,
        height,
        parent,
        0,
        instance,
        font,
    );
}

fn button(
    instance: HINSTANCE,
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    font: HFONT,
    id: isize,
) -> HWND {
    child(
        w!("BUTTON"),
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE(0),
        x,
        y,
        width,
        height,
        parent,
        id,
        instance,
        font,
    )
    .unwrap_or(HWND(std::ptr::null_mut()))
}

fn child(
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    ex: WINDOW_EX_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: isize,
    instance: HINSTANCE,
    font: HFONT,
) -> windows::core::Result<HWND> {
    let text = wide(text);
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class,
            PCWSTR::from_raw(text.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(instance),
            None,
        )?
    };
    if !font.0.is_null() {
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
    }
    Ok(hwnd)
}

fn make_font(height: i32, weight: i32) -> HFONT {
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

fn remember_font(font: HFONT) {
    if !font.0.is_null() {
        FONTS.with(|fonts| fonts.borrow_mut().push(font));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            if id == ID_COPY {
                copy_clicked();
            } else if id == ID_QUIT {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == ID_TIMER {
                let _ = KillTimer(Some(hwnd), ID_TIMER);
                restore_copy_label();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let fonts = FONTS.with(|fonts| std::mem::take(&mut *fonts.borrow_mut()));
            for font in fonts {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
            UI.with(|ui| *ui.borrow_mut() = None);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn copy_clicked() {
    let Some((text, button, window)) = UI.with(|ui| {
        ui.borrow()
            .as_ref()
            .map(|ui| (ui.copy_text.clone(), ui.copy_button, ui.window))
    }) else {
        return;
    };
    if text.is_empty() || !copy_to_clipboard(&text) {
        return;
    }
    let copied = wide("Copié");
    unsafe {
        let _ = SetWindowTextW(button, PCWSTR::from_raw(copied.as_ptr()));
        SetTimer(Some(window), ID_TIMER, 1400, None);
    }
}

fn restore_copy_label() {
    let Some((button, label)) = UI.with(|ui| {
        ui.borrow()
            .as_ref()
            .map(|ui| (ui.copy_button, wide(&ui.copy_label)))
    }) else {
        return;
    };
    unsafe {
        let _ = SetWindowTextW(button, PCWSTR::from_raw(label.as_ptr()));
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
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
