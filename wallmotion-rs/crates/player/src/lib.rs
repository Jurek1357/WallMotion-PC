//! Video playback: mpv sidecar embedded in our window.
//!
//! mpv renders straight into the canvas (`--wid`), loops the file and
//! exposes JSON IPC for pause/mute/volume - the same architecture as
//! the Python Linux backend, minus the X11/Wayland glue. Pure command
//! builders are unit-tested everywhere; spawning needs a real mpv.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// mpv IPC socket: named pipe on Windows, unix socket elsewhere.
pub const MPV_IPC_NAME: &str = "wallmotion-mpv";

/// A JSON value we send over mpv IPC (no serde dependency).
#[derive(Debug, Clone, PartialEq)]
pub enum IpcValue {
    Bool(bool),
    Int(i64),
    Str(String),
}

/// One `set_property` command line for mpv (trailing newline included).
pub fn ipc_message(prop: &str, value: &IpcValue) -> String {
    let val = match value {
        IpcValue::Bool(true) => "true".to_string(),
        IpcValue::Bool(false) => "false".to_string(),
        IpcValue::Int(n) => n.to_string(),
        IpcValue::Str(s) => {
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        }
    };
    format!("{{\"command\":[\"set_property\",\"{prop}\",{val}]}}\n")
}

/// Volume 0.0-1.0 mapped to mpv 0-100.
pub fn volume_to_mpv(volume: f32) -> i64 {
    ((volume.clamp(0.0, 1.0) * 100.0).round() as i64).clamp(0, 100)
}

/// Find mpv: explicit path, exe-adjacent dir, well-known install
/// locations, then `$PATH`.
pub fn find_mpv(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        if is_executable_file(p) {
            return Some(p.to_path_buf());
        }
    }
    for dir in candidate_dirs() {
        for name in exe_names("mpv") {
            let cand = dir.join(&name);
            if is_executable_file(&cand) {
                return Some(cand);
            }
        }
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in exe_names("mpv") {
            let cand = dir.join(&name);
            if is_executable_file(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

/// Directories searched before `$PATH`.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
        }
    }
    #[cfg(windows)]
    {
        for base in [
            std::env::var_os("ProgramFiles").map(PathBuf::from),
            std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs")),
        ]
        .into_iter()
        .flatten()
        {
            dirs.push(base.join("mpv"));
        }
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            dirs.push(home.join("scoop").join("apps").join("mpv").join("current"));
        }
    }
    dirs
}

#[cfg(windows)]
fn exe_names(base: &str) -> Vec<String> {
    vec![format!("{base}.exe"), base.to_string()]
}

#[cfg(not(windows))]
fn exe_names(base: &str) -> Vec<String> {
    vec![base.to_string()]
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Options for one wallpaper playback process.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    pub mpv: PathBuf,
    pub wid: isize,
    pub file: PathBuf,
    pub muted: bool,
    pub volume: f32,
    /// Full IPC endpoint (`\\.\pipe\…` on Windows, socket path elsewhere).
    pub ipc_endpoint: String,
}

/// Build the mpv command line. Pure, unit-tested.
pub fn mpv_args(opts: &SpawnOptions) -> Vec<String> {
    let mut args = vec![
        format!("--wid={}", opts.wid),
        "--no-osc".to_string(),
        "--no-input-default-bindings".to_string(),
        "--loop-file=inf".to_string(),
        format!("--input-ipc-server={}", opts.ipc_endpoint),
    ];
    if opts.muted {
        args.push("--mute=yes".to_string());
    } else {
        args.push(format!("--volume={}", volume_to_mpv(opts.volume)));
    }
    args.push(opts.file.to_string_lossy().into_owned());
    args
}

/// Spawn mpv detached from our console. Caller owns the [`Child`].
/// No console window pops up (CREATE_NO_WINDOW on Windows).
pub fn spawn_mpv(opts: &SpawnOptions) -> io::Result<Child> {
    let mut cmd = Command::new(&opts.mpv);
    cmd.args(mpv_args(opts))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
}

/// Default IPC endpoint for this platform.
pub fn default_ipc_endpoint() -> String {
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\{MPV_IPC_NAME}")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
        format!("{base}/{MPV_IPC_NAME}.sock")
    }
}

/// Send one IPC command. Never panics; false when mpv is unreachable.
pub fn ipc_set(endpoint: &str, prop: &str, value: &IpcValue) -> bool {
    send_line(endpoint, &ipc_message(prop, value))
}

#[cfg(windows)]
fn send_line(endpoint: &str, line: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, WriteFile, OPEN_EXISTING};
    // mpv serves one IPC client at a time; retry briefly when busy
    // (volume slider drags fire many sets per second).
    for _ in 0..8 {
        unsafe {
            let handle: HANDLE = match CreateFileW(
                &HSTRING::from(endpoint),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            ) {
                Ok(h) => h,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                    continue;
                }
            };
            if handle.is_invalid() {
                std::thread::sleep(std::time::Duration::from_millis(25));
                continue;
            }
            let bytes = line.as_bytes();
            let mut written = 0u32;
            let ok = WriteFile(handle, Some(bytes), Some(&mut written), None).is_ok()
                && written as usize == bytes.len();
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            if ok {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

#[cfg(not(windows))]
fn send_line(endpoint: &str, line: &str) -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    for _ in 0..8 {
        match UnixStream::connect(endpoint) {
            Ok(mut sock) => {
                if sock.write_all(line.as_bytes()).is_ok() {
                    return true;
                }
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> SpawnOptions {
        SpawnOptions {
            mpv: PathBuf::from("mpv"),
            wid: 12345,
            file: PathBuf::from("/tmp/v.mp4"),
            muted: false,
            volume: 0.6,
            ipc_endpoint: default_ipc_endpoint(),
        }
    }

    #[test]
    fn ipc_message_shapes() {
        assert_eq!(
            ipc_message("pause", &IpcValue::Bool(true)),
            "{\"command\":[\"set_property\",\"pause\",true]}\n"
        );
        assert_eq!(
            ipc_message("volume", &IpcValue::Int(60)),
            "{\"command\":[\"set_property\",\"volume\",60]}\n"
        );
        assert_eq!(
            ipc_message("vid", &IpcValue::Str("a\"b".into())),
            "{\"command\":[\"set_property\",\"vid\",\"a\\\"b\"]}\n"
        );
    }

    #[test]
    fn volume_mapping() {
        assert_eq!(volume_to_mpv(0.6), 60);
        assert_eq!(volume_to_mpv(5.0), 100);
        assert_eq!(volume_to_mpv(-1.0), 0);
    }

    #[test]
    fn args_muted() {
        let mut o = opts();
        o.muted = true;
        let args = mpv_args(&o);
        assert!(args.contains(&"--mute=yes".to_string()));
        assert!(args.iter().any(|a| a.starts_with("--wid=")));
        assert!(args.iter().any(|a| a.starts_with("--input-ipc-server=")));
        assert!(args.iter().any(|a| a.starts_with("--loop-file=")));
        assert_eq!(args.last().unwrap(), "/tmp/v.mp4");
    }

    #[test]
    fn args_volume() {
        let args = mpv_args(&opts());
        assert!(args.contains(&"--volume=60".to_string()));
        assert!(!args.iter().any(|a| a == "--mute=yes"));
    }

    #[test]
    fn find_explicit_file_wins() {
        // Explicit existing file wins.
        let tmp = std::env::temp_dir().join("wallmotion-mpv-probe");
        std::fs::write(&tmp, b"x").unwrap();
        assert_eq!(find_mpv(Some(&tmp)), Some(tmp.clone()));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn find_well_known_dirs() {
        // Well-known locations are searched (count + mpv suffix only).
        let found = find_mpv(None);
        if let Some(p) = found {
            let name = p.file_name().unwrap().to_string_lossy().to_lowercase();
            assert!(name.starts_with("mpv"), "{name}");
        }
    }

    #[test]
    fn ipc_to_nowhere_fails_quietly() {
        assert!(!ipc_set(
            "/definitely/not/here.sock",
            "pause",
            &IpcValue::Bool(true)
        ));
    }

    #[test]
    fn default_endpoint_shaped() {
        let ep = default_ipc_endpoint();
        #[cfg(windows)]
        assert!(ep.starts_with(r"\\.\pipe\"));
        #[cfg(not(windows))]
        assert!(ep.ends_with(".sock"));
    }

    /// Live test, runs only with WALLMOTION_TEST_MPV + WALLMOTION_TEST_VIDEO
    /// set (local runs with a display, never CI): plain canvas on WorkerW
    /// (legacy path, no manifest needed), mpv inside it, pause/volume
    /// over IPC, then cleanup.
    #[test]
    #[cfg(windows)]
    fn live_mpv_in_canvas() {
        use wallmotion_win::canvas::sys as canvas;
        use wallmotion_win::workerw::sys as ww;

        let mpv = match std::env::var("WALLMOTION_TEST_MPV") {
            Ok(v) => PathBuf::from(v),
            Err(_) => return,
        };
        let video = match std::env::var("WALLMOTION_TEST_VIDEO") {
            Ok(v) => PathBuf::from(v),
            Err(_) => return,
        };
        assert!(mpv.is_file(), "mpv binary missing");
        assert!(video.is_file(), "test video missing");

        let workerw = ww::get_workerw_handle().expect("workerw");
        let hwnd = canvas::create_canvas(&wallmotion_win::canvas::CanvasSpec {
            x: 0,
            y: 0,
            w: 640,
            h: 480,
            parent: workerw,
            layered: false,
        })
        .expect("canvas");
        let opts = SpawnOptions {
            mpv,
            wid: hwnd,
            file: video,
            muted: true,
            volume: 0.3,
            ipc_endpoint: default_ipc_endpoint(),
        };
        let mut child = spawn_mpv(&opts).expect("mpv spawned");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert!(child.try_wait().expect("poll").is_none(), "mpv alive");
        assert!(ipc_set(&opts.ipc_endpoint, "pause", &IpcValue::Bool(true)));
        assert!(ipc_set(&opts.ipc_endpoint, "volume", &IpcValue::Int(20)));
        assert!(ipc_set(&opts.ipc_endpoint, "pause", &IpcValue::Bool(false)));
        let _ = child.kill();
        let _ = child.wait();
        assert!(canvas::destroy_canvas(hwnd));
    }
}
