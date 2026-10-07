//! Native demo: animated color bars behind the desktop icons.
//!
//! Exercises the real path - WorkerW discovery, canvas setup, z-order,
//! GDI painting - with no video decoding yet. Runs ~10 seconds, then
//! removes the canvas. Windows only.

#[cfg(windows)]
fn main() {
    use wallmotion_win::canvas::sys as canvas;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    println!("primary screen: {w}x{h}");

    let wc = canvas::setup_wallpaper_canvas(0, 0, w, h)
        .expect("wallpaper canvas - run on a real Windows desktop");
    println!(
        "canvas={} workerw={} raised={}",
        wc.canvas, wc.workerw, wc.raised
    );
    for frame in 0..300u32 {
        if !canvas::paint_test_pattern(wc.canvas, w, h, frame) {
            eprintln!("paint failed at frame {frame}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
    canvas::destroy_canvas(wc.canvas);
    println!("demo done, canvas removed");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("wallmotion-demo runs on Windows only");
}
