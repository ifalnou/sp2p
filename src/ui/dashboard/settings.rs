pub fn is_auto_start_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(path) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
            if let Ok(val) = path.get_value::<String, _>("sp2p") {
                return val.contains("sp2p.exe");
            }
        }
    }
    false
}

pub fn set_auto_start(enable: bool) {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_str) = exe_path.to_str() {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                if let Ok((key, _)) =
                    hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
                {
                    if enable {
                        let val = format!("\"{}\"", exe_str);
                        let _ = key.set_value("sp2p", &val);
                    } else {
                        let _ = key.delete_value("sp2p");
                    }
                }
            }
        }
    }
}
