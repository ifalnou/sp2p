mod paint;
mod settings;
mod window;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows_sys::Win32::UI::WindowsAndMessaging::*;

static DASHBOARD_RUNNING: AtomicBool = AtomicBool::new(false);
static DASHBOARD_HWND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

pub fn spawn_dashboard() {
    let hwnd = DASHBOARD_HWND.load(Ordering::SeqCst);
    if !hwnd.is_null() {
        unsafe {
            if IsWindow(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
        return;
    }
    if DASHBOARD_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        window::run_dashboard();
        DASHBOARD_HWND.store(std::ptr::null_mut(), Ordering::SeqCst);
        DASHBOARD_RUNNING.store(false, Ordering::SeqCst);
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
