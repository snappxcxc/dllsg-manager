//! Game icons for the auto-detected games list.
//!
//! The icon embedded in a game exe is read through the Win32 shell/GDI layer
//! (no extra crates, no external processes) and cached as plain RGBA bytes in
//! `%LOCALAPPDATA%\DLSSG Manager\icons`, so the same exe is decoded once.
//! File layout: `width: u32 LE`, `height: u32 LE`, then `w * h * 4` RGBA bytes.

use crate::util;
use std::fs;
use std::path::{Path, PathBuf};

/// Size requested from Windows; rendered at ~40 px, so 64 px stays crisp.
const ICON_PX: i32 = 64;

#[derive(Debug, Clone)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Returns the cached `.rgba` file for `exe`, extracting the icon on first use.
pub fn ensure_icon(exe: &Path) -> Option<PathBuf> {
    let meta = fs::metadata(exe).ok()?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!(
        "{}\n{}\n{}",
        exe.to_string_lossy().to_ascii_lowercase(),
        meta.len(),
        stamp
    );
    let out = util::icons_dir().join(format!("{:016x}.rgba", hash64(key.as_bytes())));
    if out.exists() {
        return Some(out);
    }
    let icon = extract(exe)?;
    if icon.width == 0 || icon.height == 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(8 + icon.rgba.len());
    bytes.extend_from_slice(&icon.width.to_le_bytes());
    bytes.extend_from_slice(&icon.height.to_le_bytes());
    bytes.extend_from_slice(&icon.rgba);
    let part = out.with_extension("part");
    fs::write(&part, &bytes).ok()?;
    fs::rename(&part, &out).ok()?;
    Some(out)
}

pub fn load(path: &Path) -> Option<Icon> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let width = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let pixels = &bytes[8..];
    if width == 0 || height == 0 || pixels.len() != (width as usize * height as usize * 4) {
        return None;
    }
    Some(Icon {
        width,
        height,
        rgba: pixels.to_vec(),
    })
}

/// FNV-1a, only used to name cache files.
fn hash64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[cfg(windows)]
fn extract(exe: &Path) -> Option<Icon> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, PrivateExtractIconsW, HICON,
    };

    let wide: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut hicon: HICON = 0;
        let mut id = 0u32;
        let count = PrivateExtractIconsW(
            wide.as_ptr(),
            0,
            ICON_PX,
            ICON_PX,
            &mut hicon,
            &mut id,
            1,
            0,
        );
        if count > 0 && hicon != 0 {
            let icon = icon_to_rgba(hicon);
            DestroyIcon(hicon);
            if icon.is_some() {
                return icon;
            }
        }
        // Older/corrupt icon groups: fall back to the shell extractor.
        let mut large: HICON = 0;
        let mut small: HICON = 0;
        let count = ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1);
        let mut icon = None;
        if count > 0 {
            for handle in [large, small] {
                if handle != 0 && icon.is_none() {
                    icon = icon_to_rgba(handle);
                }
            }
        }
        for handle in [large, small] {
            if handle != 0 {
                DestroyIcon(handle);
            }
        }
        icon
    }
}

#[cfg(not(windows))]
fn extract(_exe: &Path) -> Option<Icon> {
    None
}

#[cfg(windows)]
unsafe fn icon_to_rgba(icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) -> Option<Icon> {
    use windows_sys::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(icon, &mut info) == 0 {
        return None;
    }
    let mut icon = None;
    if info.hbmColor != 0 {
        icon = bitmap_to_rgba(info.hbmColor, info.hbmMask);
    }
    if info.hbmColor != 0 {
        DeleteObject(info.hbmColor as HGDIOBJ);
    }
    if info.hbmMask != 0 {
        DeleteObject(info.hbmMask as HGDIOBJ);
    }
    icon
}

/// Reads a 32-bpp icon bitmap (+ its 1-bpp AND mask for icons without an alpha
/// channel) into straight-alpha RGBA, top-down.
#[cfg(windows)]
unsafe fn bitmap_to_rgba(
    hbm: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    hbm_mask: windows_sys::Win32::Graphics::Gdi::HBITMAP,
) -> Option<Icon> {
    use std::ffi::c_void;
    use windows_sys::Win32::Graphics::Gdi::{
        GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };

    let mut bm: BITMAP = std::mem::zeroed();
    if GetObjectW(
        hbm as HGDIOBJ,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bm as *mut BITMAP as *mut c_void,
    ) == 0
    {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 512 || h > 512 {
        return None;
    }

    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = h; // positive: bottom-up rows
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let hdc = GetDC(0);
    if hdc == 0 {
        return None;
    }
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h as u32,
        buf.as_mut_ptr() as *mut c_void,
        &mut bi,
        DIB_RGB_COLORS,
    );
    if lines == 0 {
        ReleaseDC(0, hdc);
        return None;
    }

    let (wu, hu) = (w as usize, h as usize);
    let mut rgba = vec![0u8; wu * hu * 4];
    let mut has_alpha = false;
    for y in 0..hu {
        let src = (hu - 1 - y) * wu * 4;
        let dst = y * wu * 4;
        for x in 0..wu {
            let s = src + x * 4;
            let d = dst + x * 4;
            rgba[d] = buf[s + 2];
            rgba[d + 1] = buf[s + 1];
            rgba[d + 2] = buf[s];
            rgba[d + 3] = buf[s + 3];
            has_alpha |= buf[s + 3] != 0;
        }
    }

    // 24-bpp icons come back with alpha == 0 everywhere; the transparency lives
    // in the AND mask instead.
    if !has_alpha && hbm_mask != 0 {
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let stride = ((wu + 31) / 32) * 4;
        let mut mask = vec![0u8; stride * hu];
        let mut bm_info: BITMAPINFO = std::mem::zeroed();
        bm_info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bm_info.bmiHeader.biWidth = w;
        bm_info.bmiHeader.biHeight = h;
        bm_info.bmiHeader.biPlanes = 1;
        bm_info.bmiHeader.biBitCount = 1;
        bm_info.bmiHeader.biCompression = BI_RGB;
        let mask_lines = GetDIBits(
            hdc,
            hbm_mask,
            0,
            h as u32,
            mask.as_mut_ptr() as *mut c_void,
            &mut bm_info,
            DIB_RGB_COLORS,
        );
        if mask_lines > 0 {
            for y in 0..hu {
                let row = hu - 1 - y;
                for x in 0..wu {
                    let bit = (mask[row * stride + x / 8] >> (7 - (x % 8))) & 1;
                    if bit == 1 {
                        rgba[(y * wu + x) * 4 + 3] = 0;
                    }
                }
            }
        }
    }
    ReleaseDC(0, hdc);

    Some(Icon {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_icon_from_system_exe() {
        let path = Path::new("C:/Windows/System32/notepad.exe");
        if !path.exists() {
            return;
        }
        let Some(icon) = extract(path) else {
            return;
        };
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(
            icon.rgba.len(),
            icon.width as usize * icon.height as usize * 4
        );
    }

    #[test]
    fn round_trips_cache_file() {
        let dir = std::env::temp_dir().join("dlssgm-icon-test");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("x.rgba");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[1u8; 8]);
        fs::write(&file, bytes).unwrap();
        let icon = load(&file).unwrap();
        assert_eq!((icon.width, icon.height), (2, 1));
        assert_eq!(icon.rgba.len(), 8);
    }
}
