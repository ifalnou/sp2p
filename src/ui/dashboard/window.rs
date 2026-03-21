use std::sync::atomic::Ordering;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::paint;
use super::settings;
use super::wide;
use super::DASHBOARD_HWND;

// ── Constants ──────────────────────────────────────────────────────────────────

const IDC_TAB: usize = 100;
const IDC_CHECKBOX: usize = 101;
const TIMER_REFRESH: usize = 1;
const TAB_HEIGHT: i32 = 30;
const MARGIN: i32 = 15;
const FW_BOLD: i32 = 700;

// Tab control messages / flags
const TCM_FIRST: u32 = 0x1300;
const TCM_INSERTITEMW: u32 = TCM_FIRST + 62;
const TCM_GETCURSEL: u32 = TCM_FIRST + 11;
const TCIF_TEXT: u32 = 0x0001;
const TCN_SELCHANGE: u32 = (-551i32) as u32;

// Button messages
const BN_CLICKED: u16 = 0;
const BM_SETCHECK: u32 = 0x00F1;
const BM_GETCHECK: u32 = 0x00F0;
const BST_CHECKED: usize = 1;

// ── FFI layout structs ────────────────────────────────────────────────────────

#[repr(C)]
struct TabItem {
    mask: u32,
    state: u32,
    state_mask: u32,
    text: *mut u16,
    text_max: i32,
    image: i32,
    lparam: isize,
}

#[repr(C)]
struct NotifyHeader {
    hwnd_from: HWND,
    id_from: usize,
    code: u32,
}

// ── Per-window state ───────────────────────────────────────────────────────────

struct DashboardState {
    tab_hwnd: HWND,
    checkbox_hwnd: HWND,
    active_tab: i32,
    bold_font: HGDIOBJ,
}

// ── Window creation + message loop ─────────────────────────────────────────────

pub fn run_dashboard() {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = wide("sp2p_dashboard");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);

        let title = wide("sp2p Dashboard");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            550,
            450,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );

        if hwnd.is_null() {
            tracing::error!("Failed to create dashboard window");
            return;
        }

        DASHBOARD_HWND.store(hwnd, Ordering::SeqCst);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

// ── Window procedure ───────────────────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => on_create(hwnd),
        WM_DESTROY => on_destroy(hwnd),
        _ => on_message(hwnd, msg, wparam, lparam),
    }
}

fn on_create(hwnd: HWND) -> LRESULT {
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let instance = GetModuleHandleW(std::ptr::null());

        // ── Tab control ────────────────────────────────────────────────────
        let tab_class = wide("SysTabControl32");
        let tab_hwnd = CreateWindowExW(
            0,
            tab_class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            550,
            TAB_HEIGHT,
            hwnd,
            IDC_TAB as HMENU,
            instance,
            std::ptr::null(),
        );

        let gui_font = GetStockObject(DEFAULT_GUI_FONT);
        SendMessageW(tab_hwnd, WM_SETFONT, gui_font as WPARAM, 1);

        for (i, name) in ["Transfer Progress", "Transfer History", "Settings"]
            .iter()
            .enumerate()
        {
            let mut text = wide(name);
            let item = TabItem {
                mask: TCIF_TEXT,
                state: 0,
                state_mask: 0,
                text: text.as_mut_ptr(),
                text_max: 0,
                image: -1,
                lparam: 0,
            };
            SendMessageW(tab_hwnd, TCM_INSERTITEMW, i, &item as *const _ as isize);
        }

        // ── Checkbox (hidden until Settings tab) ───────────────────────────
        let btn_class = wide("BUTTON");
        let cb_text = wide("Run sp2p on Windows startup");
        let checkbox_hwnd = CreateWindowExW(
            0,
            btn_class.as_ptr(),
            cb_text.as_ptr(),
            WS_CHILD | BS_AUTOCHECKBOX as u32,
            20,
            TAB_HEIGHT + 55,
            300,
            24,
            hwnd,
            IDC_CHECKBOX as HMENU,
            instance,
            std::ptr::null(),
        );
        SendMessageW(checkbox_hwnd, WM_SETFONT, gui_font as WPARAM, 1);

        if settings::is_auto_start_enabled() {
            SendMessageW(checkbox_hwnd, BM_SETCHECK, BST_CHECKED, 0);
        }

        // ── Bold font for headings ─────────────────────────────────────────
        let mut lf: LOGFONTW = std::mem::zeroed();
        GetObjectW(
            gui_font,
            std::mem::size_of::<LOGFONTW>() as i32,
            &mut lf as *mut _ as *mut _,
        );
        lf.lfWeight = FW_BOLD;
        let bold_font = CreateFontIndirectW(&lf);

        // ── Store per-window state ─────────────────────────────────────────
        let state = Box::new(DashboardState {
            tab_hwnd,
            checkbox_hwnd,
            active_tab: 0,
            bold_font,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(hwnd, TIMER_REFRESH, 500, None);
    }
    0
}

fn on_destroy(hwnd: HWND) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DashboardState;
        if !ptr.is_null() {
            let state = Box::from_raw(ptr);
            if !state.bold_font.is_null() {
                DeleteObject(state.bold_font);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        KillTimer(hwnd, TIMER_REFRESH);
        DASHBOARD_HWND.store(std::ptr::null_mut(), Ordering::SeqCst);
        PostQuitMessage(0);
    }
    0
}

fn on_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DashboardState;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = &mut *ptr;

        match msg {
            WM_SIZE => {
                let width = (lparam as u32 & 0xFFFF) as i32;
                MoveWindow(state.tab_hwnd, 0, 0, width, TAB_HEIGHT, 1);
                InvalidateRect(hwnd, std::ptr::null(), 1);
                0
            }

            WM_TIMER if wparam == TIMER_REFRESH => {
                if state.active_tab < 2 {
                    invalidate_content(hwnd);
                }
                0
            }

            WM_NOTIFY => {
                let hdr = &*(lparam as *const NotifyHeader);
                if hdr.id_from == IDC_TAB && hdr.code == TCN_SELCHANGE {
                    let sel = SendMessageW(state.tab_hwnd, TCM_GETCURSEL, 0, 0) as i32;
                    state.active_tab = sel;
                    ShowWindow(
                        state.checkbox_hwnd,
                        if sel == 2 { SW_SHOW } else { SW_HIDE },
                    );
                    invalidate_content(hwnd);
                }
                0
            }

            WM_COMMAND => {
                let id = (wparam & 0xFFFF) as usize;
                let code = ((wparam >> 16) & 0xFFFF) as u16;
                if id == IDC_CHECKBOX && code == BN_CLICKED {
                    let checked =
                        SendMessageW(state.checkbox_hwnd, BM_GETCHECK, 0, 0) as usize;
                    settings::set_auto_start(checked == BST_CHECKED);
                }
                0
            }

            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                let mut client: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut client);

                let content = RECT {
                    left: 0,
                    top: TAB_HEIGHT,
                    right: client.right,
                    bottom: client.bottom,
                };
                FillRect(hdc, &content, (COLOR_WINDOW + 1) as usize as HBRUSH);

                let old_font = SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT));
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, GetSysColor(COLOR_WINDOWTEXT));

                let left = MARGIN;
                let right = client.right - MARGIN;
                let mut y = TAB_HEIGHT + 10;

                match state.active_tab {
                    0 => paint::paint_progress(hdc, state.bold_font, left, right, &mut y),
                    1 => paint::paint_history(hdc, state.bold_font, left, right, &mut y),
                    2 => paint::paint_settings_heading(hdc, state.bold_font, left, right, &mut y),
                    _ => {}
                }

                SelectObject(hdc, old_font);
                EndPaint(hwnd, &ps);
                0
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn invalidate_content(hwnd: HWND) {
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        rc.top = TAB_HEIGHT;
        InvalidateRect(hwnd, &rc, 1);
    }
}
