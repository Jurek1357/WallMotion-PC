//! Windows autostart: an `HKCU\...\Run` value so the app launches at
//! logon, hidden to the tray, resuming the last wallpaper.
//!
//! The registry is the source of truth (no config key): the settings
//! checkbox shows [`is_enabled`] scanned at startup, toggling writes or
//! deletes the value. The stored command launches the exe with
//! `--minimized` (tray + autoplay), so a logon start never pops a window.
//!
//! Only std + the `windows` crate (Registry feature, already a dependency).
//! Off Windows everything is a stub (`false` / `Err`).

/// Value name under the Run key.
#[cfg(windows)]
pub const RUN_VALUE: &str = "WallMotion";

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Command line stored in the Run value: quoted exe + `--minimized`.
pub fn run_command(exe: &str) -> String {
    format!("\"{exe}\" --minimized")
}

/// Current exe path for the Run value (`None` when it cannot be known).
pub fn current_exe_string() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// True when the Run value exists and points at this exe with `--minimized`.
#[cfg(windows)]
pub fn is_enabled() -> bool {
    let expected = current_exe_string().map(|e| run_command(&e));
    match (read_run_value(), expected) {
        (Some(stored), Some(expected)) => stored.trim() == expected,
        _ => false,
    }
}

/// Stub off Windows.
#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

/// Create (`on = true`) or delete the Run value.
#[cfg(windows)]
pub fn set_enabled(on: bool) -> Result<(), String> {
    if on {
        let exe = current_exe_string().ok_or("unknown exe path".to_string())?;
        write_run_value(&run_command(&exe))
    } else {
        delete_run_value()
    }
}

/// Stub off Windows.
#[cfg(not(windows))]
pub fn set_enabled(_on: bool) -> Result<(), String> {
    Err("autostart needs Windows".to_string())
}

#[cfg(windows)]
fn wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

#[cfg(windows)]
fn read_run_value() -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    unsafe {
        let subkey = wide_nul(RUN_KEY);
        let value = wide_nul(RUN_VALUE);
        let mut len: u32 = 0;
        // Size probe first (missing value -> ERROR_FILE_NOT_FOUND).
        if RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize / 2).max(1)];
        let mut got = (buf.len() * 2) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut got),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let used = (got as usize / 2).min(buf.len());
        Some(
            String::from_utf16_lossy(&buf[..used])
                .trim_matches('\0')
                .to_string(),
        )
    }
}

#[cfg(windows)]
fn write_run_value(cmd: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    unsafe {
        let subkey = wide_nul(RUN_KEY);
        let value = wide_nul(RUN_VALUE);
        let mut hkey = HKEY::default();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot open Run key ({rc:?})"));
        }
        let data: Vec<u16> = cmd.encode_utf16().chain([0]).collect();
        let bytes: &[u8] = std::slice::from_raw_parts(
            data.as_ptr().cast(),
            data.len() * std::mem::size_of::<u16>(),
        );
        let rc = RegSetValueExW(hkey, PCWSTR(value.as_ptr()), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot write Run value ({rc:?})"));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn delete_run_value() -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::*;
    unsafe {
        let subkey = wide_nul(RUN_KEY);
        let value = wide_nul(RUN_VALUE);
        let mut hkey = HKEY::default();
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_WRITE,
            &mut hkey,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot open Run key ({rc:?})"));
        }
        let rc = RegDeleteValueW(hkey, PCWSTR(value.as_ptr()));
        let _ = RegCloseKey(hkey);
        // Already gone counts as disabled, not as an error.
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("cannot delete Run value ({rc:?})"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_shape() {
        assert_eq!(
            run_command(r"C:\app\wallmotion-settings.exe"),
            r#""C:\app\wallmotion-settings.exe" --minimized"#.to_string()
        );
    }

    #[test]
    fn exe_string_sane() {
        // Best effort: in tests current_exe is the test binary itself.
        let exe = current_exe_string().expect("test binary has an exe path");
        assert!(!exe.is_empty());
        assert!(run_command(&exe).ends_with("--minimized"));
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_off_windows() {
        assert!(!is_enabled());
        assert!(set_enabled(true).is_err());
    }
}
