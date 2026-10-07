//! Single instance: only one settings app may run.
//!
//! A lock file with our PID in the temp dir; a live owner (checked via
//! OpenProcess) means "exit quietly", a dead one means stale (take over).

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

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
        else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok()
            && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(handle);
        alive
    }
}

#[cfg(not(windows))]
fn process_alive(pid: u32) -> bool {
    // Best effort: a live PID file means someone is home. Stale locks
    // clear on the next successful start after a reboot (temp dir).
    let _ = pid;
    std::fs::metadata(lock_path()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_lock_then_release() {
        // Takeover writes our PID; a second check in the same process
        // sees itself and takes over again (no false positive).
        assert!(!another_instance_running());
        assert!(!another_instance_running());
        let _ = std::fs::remove_file(lock_path());
    }
}
