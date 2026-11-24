use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// Deterministic FNV-1a 64-bit hash used for stable icon cache filenames.
pub fn icon_filename_for_path(exe_path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in exe_path.to_lowercase().as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.png")
}

/// Extract an executable's Windows icon to a cached 32x32 RGBA PNG inside `cache_dir`.
/// Returns `Some("<hash>.png")` on success, `None` on any error (never panics or blocks tracking).
pub fn ensure_cached_icon(exe_path: &str, cache_dir: &Path) -> Option<String> {
    if exe_path.trim().is_empty() || !Path::new(exe_path).exists() {
        return None;
    }

    let filename = icon_filename_for_path(exe_path);
    let out_path = cache_dir.join(&filename);
    if out_path.exists() {
        return Some(filename);
    }

    let (w, h, rgba) = extract_icon_rgba(exe_path)?;
    let _ = fs::create_dir_all(cache_dir);
    write_png_rgba(&out_path, w, h, &rgba)?;
    Some(filename)
}

fn write_png_rgba(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Option<()> {
    let file = File::create(path).ok()?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header().ok()?;
    png_writer.write_image_data(rgba).ok()?;
    Some(())
}

#[cfg(not(windows))]
fn extract_icon_rgba(_exe_path: &str) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(windows)]
fn extract_icon_rgba(exe_path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let wide: Vec<u16> = OsStr::new(exe_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut shfi: SHFILEINFOW = std::mem::zeroed();
        let res = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut shfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.hIcon.is_null() {
            return None;
        }

        let hicon = shfi.hIcon;
        let mut icon_info: ICONINFO = std::mem::zeroed();
        let ok_info = GetIconInfo(hicon, &mut icon_info);
        DestroyIcon(hicon);
        if ok_info == 0 {
            return None;
        }

        let result = read_icon_bitmaps(&icon_info);

        if !icon_info.hbmColor.is_null() {
            DeleteObject(icon_info.hbmColor as _);
        }
        if !icon_info.hbmMask.is_null() {
            DeleteObject(icon_info.hbmMask as _);
        }
        result
    }
}

#[cfg(windows)]
unsafe fn read_icon_bitmaps(info: &ICONINFO) -> Option<(u32, u32, Vec<u8>)> {
    if info.hbmColor.is_null() {
        return None;
    }

    let mut bmp: BITMAP = std::mem::zeroed();
    if GetObjectW(
        info.hbmColor as _,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bmp as *mut _ as *mut _,
    ) == 0
    {
        return None;
    }

    let w = bmp.bmWidth;
    let h = bmp.bmHeight.abs();
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }

    let hdc = CreateCompatibleDC(std::ptr::null_mut());
    if hdc.is_null() {
        return None;
    }

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let byte_len = (w as usize) * (h as usize) * 4;
    let mut color_bgra = vec![0u8; byte_len];
    let got_color = GetDIBits(
        hdc,
        info.hbmColor,
        0,
        h as u32,
        color_bgra.as_mut_ptr() as *mut _,
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Check if color bitmap actually contains non-zero alpha channel
    let has_alpha = color_bgra.chunks_exact(4).any(|px| px[3] != 0);
    let mut mask_bgra = Vec::new();
    if !has_alpha && !info.hbmMask.is_null() {
        mask_bgra.resize(byte_len, 0u8);
        let _ = GetDIBits(
            hdc,
            info.hbmMask,
            0,
            h as u32,
            mask_bgra.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );
    }

    DeleteDC(hdc);
    if got_color == 0 {
        return None;
    }

    let mut rgba = vec![0u8; byte_len];
    for i in (0..byte_len).step_by(4) {
        let b = color_bgra[i];
        let g = color_bgra[i + 1];
        let r = color_bgra[i + 2];
        let a = if has_alpha {
            color_bgra[i + 3]
        } else if !mask_bgra.is_empty() && mask_bgra[i] > 127 {
            0
        } else {
            255
        };
        rgba[i] = r;
        rgba[i + 1] = g;
        rgba[i + 2] = b;
        rgba[i + 3] = a;
    }

    Some((w as u32, h as u32, rgba))
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::database::{Database, PendingBucketEntry, UsageBucketKey};
    use std::collections::HashMap;

    #[test]
    fn extracts_real_windows_exe_icon_and_serves_data_url_with_fallback() {
        let dir = std::env::temp_dir().join(format!("piku_icon_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cache_dir = dir.join("icon-cache");
        let _ = fs::create_dir_all(&cache_dir);

        // Nonexistent exe falls back cleanly to None
        assert!(ensure_cached_icon(r"C:\NonExistent\ghost.exe", &cache_dir).is_none());

        // Real Windows executable extracts cleanly into a valid PNG and round-trips through Database
        let notepad = r"C:\Windows\System32\notepad.exe";
        if Path::new(notepad).exists() {
            let hash = ensure_cached_icon(notepad, &cache_dir).expect("should extract notepad.exe icon");
            let png_path = cache_dir.join(&hash);
            assert!(png_path.exists());
            let bytes = fs::read(&png_path).unwrap();
            assert!(bytes.starts_with(&[137, 80, 78, 71]), "cached icon must be valid PNG");

            let db = Database::open(&dir).unwrap();
            let mut buckets = HashMap::new();
            buckets.insert(
                UsageBucketKey {
                    local_date: "2026-11-23".to_string(),
                    local_hour: 12,
                    exe_name: "notepad.exe".to_string(),
                },
                PendingBucketEntry {
                    exe_path: Some(notepad.to_string()),
                    display_name: "Notepad".to_string(),
                    active_secs: 42,
                },
            );
            db.flush_usage_buckets(&buckets, 1_700_000_000).unwrap();
            db.set_app_icon_hash("notepad.exe", &hash).unwrap();

            let day = db.get_day_summary("2026-11-23").unwrap();
            let data_url = day.apps[0].icon_data_url.as_deref().unwrap_or("");
            assert!(data_url.starts_with("data:image/png;base64,"), "expected base64 PNG data URL");
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
