//! Single instance: only one settings app may run.
//!
//! A lock file with our PID in the temp dir; a live owner means "exit
//! quietly", a dead one means stale (take over). On Unix the PID is
//! checked against `/proc` — a crashed instance used to leave a lock
//! behind that silently blocked every later launch until reboot.

use std::path::PathBuf;

/// True when another live instance owns the app.
pub fn another_instance_running() -> bool {
    let path = lock_path();
    let pid = match std::fs::read_to_string(&path) {
        Ok(text) => match text.trim().parse::<u32>() {
            Ok(pid) => pid,
            Err(_) => return take_over(&path),
        },
        Err(_) => return take_over(&path),
    };
    if pid == std::process::id() {
        return take_over(&path);
    }
    if process_alive(pid) {
        return true;
    }
    take_over(&path)
}

fn lock_path() -> PathBuf {
    std::env::temp_dir().join("wallmotion-settings.lock")
}

fn take_over(path: &PathBuf) -> bool {
    match std::fs::write(path, std::process::id().to_string()) {
        Ok(()) => false,
        Err(_) => true, // cannot write: assume someone is there
    }
}

/// Remove the lock if it still belongs to this process. Called on a
/// clean exit so the next start does not depend on the liveness check.
pub fn release_lock() {
    let path = lock_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if text.trim().parse::<u32>() == Ok(std::process::id()) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(handle);
        alive
    }
}

#[cfg(not(windows))]
fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No cheap portable liveness check: treat the lock as stale
        // rather than blocking every later launch. The TCP command
        // port below is still an authoritative second gate.
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_lock_takeover_stale_and_release() {
        let _ = std::fs::remove_file(lock_path());
        // Takeover writes our PID; a second check in the same process
        // sees itself and takes over again (no false positive).
        assert!(!another_instance_running());
        assert!(!another_instance_running());
        // A lock left by a dead PID must not block the next start.
        std::fs::write(lock_path(), u32::MAX.to_string()).unwrap();
        assert!(!another_instance_running());
        assert_eq!(
            std::fs::read_to_string(lock_path()).unwrap().trim(),
            std::process::id().to_string()
        );
        release_lock();
        assert!(!lock_path().exists());
    }
}
