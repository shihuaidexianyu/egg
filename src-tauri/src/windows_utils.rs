use std::{
    env,
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use sha1::{Digest, Sha1};
use windows::{
    core::{Error, Result, PCWSTR},
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
        },
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            Environment::ExpandEnvironmentStringsW,
        },
        UI::{
            Shell::ExtractIconExW,
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
        },
    },
};

/// RAII guard for COM initialization on the current thread.
pub(crate) struct ComGuard {
    initialized: bool,
}

impl ComGuard {
    /// Initializes COM in STA mode if needed.
    pub(crate) unsafe fn new() -> Result<Self> {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_ok() {
            Ok(Self { initialized: true })
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self { initialized: false })
        } else {
            Err(Error::from(hr))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Converts an [`OsStr`] into a null-terminated wide string buffer suitable for Win32 APIs.
pub(crate) fn os_str_to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

/// Trims trailing null terminators and converts a UTF-16 buffer into a [`String`].
pub(crate) fn wide_to_string(buffer: &[u16]) -> Option<String> {
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    if end == 0 {
        return None;
    }

    String::from_utf16(&buffer[..end]).ok()
}

/// Expands Windows environment variables (e.g. `%SystemRoot%`).
pub(crate) fn expand_env_vars(value: &str) -> Option<String> {
    if !value.contains('%') {
        return Some(value.to_string());
    }

    let wide_input = os_str_to_wide(OsStr::new(value));
    unsafe {
        let required = ExpandEnvironmentStringsW(PCWSTR(wide_input.as_ptr()), None);
        if required == 0 {
            return None;
        }

        let mut buffer = vec![0u16; required as usize];
        let written = ExpandEnvironmentStringsW(PCWSTR(wide_input.as_ptr()), Some(&mut buffer));
        if written == 0 {
            return None;
        }

        wide_to_string(&buffer)
    }
}

/// Extracts a large application icon and returns it as PNG encoded base64.
pub(crate) fn extract_icon_from_path(path: &str, icon_index: i32) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let resolved = if path.contains('%') {
        expand_env_vars(path).unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };

    if !Path::new(&resolved).exists() {
        return None;
    }

    let wide_path = os_str_to_wide(OsStr::new(&resolved));
    let mut icon = HICON::default();
    let icon_index = icon_index.max(0);
    let cache_key = icon_cache_key(&resolved, icon_index);

    if let Some(encoded) = load_cached_icon(&cache_key) {
        return Some(encoded);
    }

    unsafe {
        let extracted = ExtractIconExW(
            PCWSTR(wide_path.as_ptr()),
            icon_index,
            Some(&mut icon),
            None,
            1,
        );
        if extracted == 0 || icon.is_invalid() {
            return None;
        }

        let encoded = icon_to_base64(icon);
        // icon_to_base64 handles destroying the icon.
        if let Some(ref data) = encoded {
            store_cached_icon(&cache_key, data);
        }
        encoded
    }
}

fn icon_cache_key(path: &str, icon_index: i32) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.to_lowercase().as_bytes());
    hasher.update(icon_index.to_le_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    const LUT: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        hex.push(LUT[(byte >> 4) as usize] as char);
        hex.push(LUT[(byte & 0x0f) as usize] as char);
    }
    hex
}

fn load_cached_icon(key: &str) -> Option<String> {
    let path = cache_file_path(key)?;
    fs::read_to_string(path).ok()
}

fn store_cached_icon(key: &str, data: &str) {
    if let Some(path) = cache_file_path(key) {
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let _ = fs::write(path, data);
    }
}

fn cache_file_path(key: &str) -> Option<PathBuf> {
    let mut dir = icon_cache_dir()?;
    dir.push(format!("{key}.b64"));
    Some(dir)
}

fn icon_cache_dir() -> Option<PathBuf> {
    let base = env::var("LOCALAPPDATA").ok()?;
    Some(Path::new(&base).join("RustLauncher").join("icons"))
}

unsafe fn icon_to_base64(icon: HICON) -> Option<String> {
    let mut icon_info: ICONINFO = std::mem::zeroed();
    if GetIconInfo(icon, &mut icon_info).is_err() {
        let _ = DestroyIcon(icon);
        return None;
    }

    let color_bitmap = if !icon_info.hbmColor.is_invalid() {
        icon_info.hbmColor
    } else {
        icon_info.hbmMask
    };

    if color_bitmap.is_invalid() {
        cleanup_icon(&icon_info);
        let _ = DestroyIcon(icon);
        return None;
    }

    let mut bitmap: BITMAP = std::mem::zeroed();
    if GetObjectW(
        color_bitmap,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bitmap as *mut _ as *mut _),
    ) == 0
    {
        cleanup_icon(&icon_info);
        let _ = DestroyIcon(icon);
        return None;
    }

    let width = bitmap.bmWidth as i32;
    let mut height = bitmap.bmHeight as i32;
    if icon_info.hbmColor.is_invalid() {
        height /= 2;
    }

    if width <= 0 || height <= 0 {
        cleanup_icon(&icon_info);
        let _ = DestroyIcon(icon);
        return None;
    }

    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height; // top-down DIB
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let dc = CreateCompatibleDC(HDC::default());
    if dc.is_invalid() {
        cleanup_icon(&icon_info);
        let _ = DestroyIcon(icon);
        return None;
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    if GetDIBits(
        dc,
        color_bitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut info,
        DIB_RGB_COLORS,
    ) == 0
    {
        let _ = DeleteDC(dc);
        cleanup_icon(&icon_info);
        let _ = DestroyIcon(icon);
        return None;
    }

    let _ = DeleteDC(dc);

    // Convert BGRA -> RGBA
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    cleanup_icon(&icon_info);
    let _ = DestroyIcon(icon);

    let mut png = Vec::new();
    {
        let encoder = PngEncoder::new(&mut png);
        if encoder
            .write_image(&pixels, width as u32, height as u32, ColorType::Rgba8)
            .is_err()
        {
            return None;
        }
    }

    Some(BASE64.encode(png))
}

unsafe fn cleanup_icon(info: &ICONINFO) {
    if !info.hbmColor.is_invalid() {
        let _ = DeleteObject(info.hbmColor);
    }
    if !info.hbmMask.is_invalid() {
        let _ = DeleteObject(info.hbmMask);
    }
}
