//! Static image wallpaper: cover-fit to the screen, set via the system.
//!
//! Mirrors `fit_image_to_screen()` + `set_static_wallpaper()` in
//! `wallmotion/wallpaper.py`. Cover math is pure and unit-tested.

use std::path::{Path, PathBuf};

/// Cover-fit source rectangle: (x, y, w, h) of the crop in a
/// `src_w` x `src_h` image that exactly fills `dst_w` x `dst_h`.
/// Pure, unit-tested.
pub fn cover_crop_rect(
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    let scale = (dst_w as f64 / src_w as f64).max(dst_h as f64 / src_h as f64);
    let scaled_w = (src_w as f64 * scale).round() as u32;
    let scaled_h = (src_h as f64 * scale).round() as u32;
    // Crop the scaled image centered to the exact target size.
    let crop_w = scaled_w.min(dst_w.max(1));
    let crop_h = scaled_h.min(dst_h.max(1));
    let x = scaled_w.saturating_sub(crop_w) / 2;
    let y = scaled_h.saturating_sub(crop_h) / 2;
    // Map the crop back into source pixels for the resizer.
    let inv = 1.0 / scale;
    let sx = (x as f64 * inv).round() as u32;
    let sy = (y as f64 * inv).round() as u32;
    let sw = ((crop_w as f64 * inv).round() as u32).max(1).min(src_w);
    let sh = ((crop_h as f64 * inv).round() as u32).max(1).min(src_h);
    Some((sx.min(src_w - sw), sy.min(src_h - sh), sw, sh))
}

/// Resize `src` with cover-fit to exactly `dst_w` x `dst_h`.
/// Returns the resized image, or None on degenerate input.
pub fn cover_fit(
    img: &image::DynamicImage,
    dst_w: u32,
    dst_h: u32,
) -> Option<image::DynamicImage> {
    let (sw, sh) = (img.width(), img.height());
    let (sx, sy, cw, ch) = cover_crop_rect(sw, sh, dst_w, dst_h)?;
    let cropped = img.crop_imm(sx, sy, cw, ch);
    Some(cropped.resize_exact(
        dst_w.max(1),
        dst_h.max(1),
        image::imageops::FilterType::Lanczos3,
    ))
}

/// Fit an image file to the screen and save as BMP in the temp dir.
/// Returns the fitted path (or the original on any failure), mirroring
/// the Python fallback behavior.
pub fn fit_image_to_screen(image_path: &Path, width: u32, height: u32) -> PathBuf {
    let fallback = || image_path.to_path_buf();
    if width == 0 || height == 0 {
        return fallback();
    }
    let img = match image::open(image_path) {
        Ok(img) => img,
        Err(_) => return fallback(),
    };
    let fitted = match cover_fit(&img, width, height) {
        Some(img) => img,
        None => return fallback(),
    };
    let mut out = std::env::temp_dir();
    out.push("wallmotion-fitted.bmp");
    match fitted.save(&out) {
        Ok(()) => out,
        Err(_) => fallback(),
    }
}

/// Set a static image as the desktop wallpaper (Windows only).
#[cfg(windows)]
pub fn set_static_wallpaper(image_path: &Path) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
        SPI_SETDESKWALLPAPER,
    };
    use windows::core::HSTRING;
    let wide: HSTRING = image_path.to_string_lossy().into_owned().into();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
        .is_ok()
    }
}

/// Current desktop wallpaper path, if readable.
#[cfg(windows)]
pub fn get_current_wallpaper() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETDESKWALLPAPER,
    };
    unsafe {
        let mut buf = [0u16; 260];
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            Default::default(),
        )
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16(&buf[..len]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_exact_size() {
        assert_eq!(cover_crop_rect(1920, 1080, 1920, 1080), Some((0, 0, 1920, 1080)));
    }

    #[test]
    fn cover_wide_screen_crops_top_bottom() {
        // 16:9 image onto 32:9 canvas: full width, center crop vertically.
        assert_eq!(
            cover_crop_rect(1920, 1080, 3840, 1080),
            Some((0, 270, 1920, 540))
        );
    }

    #[test]
    fn cover_tall_screen_crops_top_bottom() {
        // Square image onto 16:9: full width, center crop vertically.
        assert_eq!(
            cover_crop_rect(1080, 1080, 1920, 1080),
            Some((0, 236, 1080, 608))
        );
    }

    #[test]
    fn cover_degenerate() {
        assert_eq!(cover_crop_rect(0, 1080, 1920, 1080), None);
        assert_eq!(cover_crop_rect(1920, 1080, 0, 1080), None);
    }

    #[test]
    fn cover_fit_exact_pixels() {
        let img = image::DynamicImage::new_rgb8(64, 32);
        let out = cover_fit(&img, 128, 64).unwrap();
        assert_eq!((out.width(), out.height()), (128, 64));
    }

    #[test]
    fn fit_missing_file_falls_back() {
        let orig = Path::new("/definitely/not/here.png");
        assert_eq!(fit_image_to_screen(orig, 1920, 1080), orig);
    }
}
