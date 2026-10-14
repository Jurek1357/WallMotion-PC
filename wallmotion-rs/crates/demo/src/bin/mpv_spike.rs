//! libmpv spike: play a real video file behind the desktop icons.
//!
//! Temporary verification binary (not shipped):
//! `mpv-spike <video>` creates the wallpaper canvas, renders the video
//! into it via in-process libmpv (`wid` embed), prints mpv events,
//! unfreezes on the first frame and cleans up after ~15 s or end-of-file.
//! Needs `libmpv-2.dll` on PATH (see `wallmotion-mpv` docs). Windows only.

#[cfg(windows)]
fn main() {
    use wallmotion_win::canvas::sys as canvas;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let file = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: mpv-spike <video>");
        std::process::exit(2);
    });
    assert!(std::path::Path::new(&file).is_file(), "video missing");

    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let wc = canvas::setup_wallpaper_canvas(0, 0, w, h)
        .expect("wallpaper canvas - run on a real Windows desktop");
    println!(
        "canvas={} workerw={} raised={}",
        wc.canvas, wc.workerw, wc.raised
    );

    let player = wallmotion_mpv::Player::open(wc.canvas).expect("mpv open");
    player.set_muted(true).expect("mute (spike runs silent)");
    player
        .load_file(std::path::Path::new(&file), true)
        .expect("loadfile");
    println!("playing {file}");
    canvas::ensure_below_defview(wc.canvas);

    let start = std::time::Instant::now();
    let mut unfrozen = false;
    let mut diagnosed = false;
    let mut last_watchdog = std::time::Instant::now();
    loop {
        if let Some(ev) = player.wait_event(200) {
            println!("event: {ev:?} at {:.1}s", start.elapsed().as_secs_f32());
            match ev {
                wallmotion_mpv::LibEvent::PlaybackRestart if !unfrozen => {
                    player.set_paused(false).expect("unpause");
                    unfrozen = true;
                    println!("unfrozen on first frame");
                }
                wallmotion_mpv::LibEvent::EndOfFile
                | wallmotion_mpv::LibEvent::FileError
                | wallmotion_mpv::LibEvent::Shutdown => break,
                _ => {}
            }
            if unfrozen && !diagnosed {
                diagnosed = true;
                println!(
                    "vo-configured={:?} vo={:?} video-codec={:?} hwdec-current={:?}",
                    player.prop_bool("vo-configured"),
                    player.prop_str("vo"),
                    player.prop_str("video-codec"),
                    player.prop_str("hwdec-current"),
                );
            }
        }
        // Same watchdog the real app runs: keep the canvas below the icons.
        if last_watchdog.elapsed() > std::time::Duration::from_secs(1) {
            last_watchdog = std::time::Instant::now();
            canvas::ensure_below_defview(wc.canvas);
        }
        if start.elapsed() > std::time::Duration::from_secs(20) {
            println!("time's up, stopping");
            break;
        }
    }
    canvas::destroy_canvas(wc.canvas);
    println!("spike done, canvas removed");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mpv-spike runs on Windows only");
}
