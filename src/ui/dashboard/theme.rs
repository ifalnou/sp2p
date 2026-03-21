use std::ffi::c_void;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::*;

use super::wide;

// COLORREF format: 0x00BBGGRR

pub struct ThemeColors {
    pub background: u32,
    pub text: u32,
    pub separator: u32,
    pub progress_bg: u32,
}

pub fn is_dark_mode() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
    ) {
        if let Ok(val) = key.get_value::<u32, _>("AppsUseLightTheme") {
            return val == 0;
        }
    }
    false
}

pub fn colors(dark: bool) -> ThemeColors {
    if dark {
        ThemeColors {
            background: 0x001E1E1E, // RGB(30,30,30)
            text: 0x00E0E0E0,       // RGB(224,224,224)
            separator: 0x00555555,   // RGB(85,85,85)
            progress_bg: 0x00333333, // RGB(51,51,51)
        }
    } else {
        unsafe {
            ThemeColors {
                background: GetSysColor(COLOR_WINDOW),
                text: GetSysColor(COLOR_WINDOWTEXT),
                separator: GetSysColor(COLOR_GRAYTEXT),
                progress_bg: 0x00E0E0E0,
            }
        }
    }
}

/// Immersive dark mode title bar via DwmSetWindowAttribute (Windows 10 1809+).
pub fn apply_dark_title_bar(hwnd: HWND, dark: bool) {
    unsafe {
        let value: i32 = if dark { 1 } else { 0 };
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &value as *const _ as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// SetPreferredAppMode(AllowDark) — uxtheme ordinal 135.
/// Call once before creating windows.
pub fn init_dark_mode_support() {
    unsafe {
        let dll = wide("uxtheme.dll");
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return;
        }
        type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
        if let Some(f) = GetProcAddress(module, 135usize as *const u8) {
            let f: SetPreferredAppMode = std::mem::transmute(f);
            f(1); // AllowDark
        }
    }
}

/// AllowDarkModeForWindow — uxtheme ordinal 133.
pub fn allow_dark_mode_for_window(hwnd: HWND, dark: bool) {
    unsafe {
        let dll = wide("uxtheme.dll");
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return;
        }
        type AllowDarkMode = unsafe extern "system" fn(HWND, i32) -> i32;
        if let Some(f) = GetProcAddress(module, 133usize as *const u8) {
            let f: AllowDarkMode = std::mem::transmute(f);
            f(hwnd, if dark { 1 } else { 0 });
        }
    }
}

/// Apply DarkMode_Explorer or default theme to a common control.
pub fn apply_control_theme(hwnd: HWND, dark: bool) {
    unsafe {
        let theme = if dark {
            wide("DarkMode_Explorer")
        } else {
            wide("")
        };
        windows_sys::Win32::UI::Controls::SetWindowTheme(
            hwnd,
            theme.as_ptr(),
            std::ptr::null(),
        );
    }
}

/// RefreshImmersiveColorPolicyState — uxtheme ordinal 104.
pub fn refresh_theme() {
    unsafe {
        let dll = wide("uxtheme.dll");
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return;
        }
        type Refresh = unsafe extern "system" fn();
        if let Some(f) = GetProcAddress(module, 104usize as *const u8) {
            let f: Refresh = std::mem::transmute(f);
            f();
        }
    }
}
