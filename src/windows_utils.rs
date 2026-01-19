use std::{
    env,
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder};
use log::warn;
use sha1::{Digest, Sha1};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, LoadKeyboardLayoutW, KLF_ACTIVATE, KLF_SETFORPROCESS,
};
use windows::{
    core::{Error, Interface, Result, PCWSTR},
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
        },
        Storage::FileSystem::WIN32_FIND_DATAW,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
            },
            Environment::ExpandEnvironmentStringsW,
        },
        UI::{
            Shell::{ExtractIconExW, IShellLinkW, ShellLink, SLGP_RAWPATH, SLGP_UNCPRIORITY},
            WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
        },
    },
};
#[cfg(target_os = "windows")]
use winreg::{enums::*, RegKey};

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

#[derive(Debug, Clone)]
pub(crate) struct ShortcutInfo {
    pub target_path: Option<String>,
    pub arguments: Option<String>,
    pub working_directory: Option<String>,
    pub description: Option<String>,
    pub icon_path: Option<String>,
    pub icon_index: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct InternetShortcutInfo {
    pub url: String,
    pub icon_path: Option<String>,
    pub icon_index: i32,
    pub description: Option<String>,
}

/// Resolves `.lnk` shortcuts and extracts metadata such as target executable, arguments and icon info.
pub(crate) fn resolve_shell_link(path: &Path) -> Option<ShortcutInfo> {
    #[cfg(target_os = "windows")]
    unsafe {
        let _guard = ComGuard::new().ok()?;
        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = shell_link.cast().ok()?;
        let wide_path = os_str_to_wide(path.as_os_str());
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ).ok()?;

        const BUFFER_LEN: usize = 1024;
        let mut shortcut = ShortcutInfo {
            target_path: None,
            arguments: None,
            working_directory: None,
            description: None,
            icon_path: None,
            icon_index: 0,
        };

        let mut target_buffer = vec![0u16; BUFFER_LEN];
        let path_flags = (SLGP_UNCPRIORITY.0 | SLGP_RAWPATH.0) as u32;
        if shell_link
            .GetPath(
                target_buffer.as_mut_slice(),
                ptr::null_mut::<WIN32_FIND_DATAW>(),
                path_flags,
            )
            .is_ok()
        {
            shortcut.target_path = wide_to_string(&target_buffer);
        }

        let mut arg_buffer = vec![0u16; BUFFER_LEN];
        if shell_link.GetArguments(arg_buffer.as_mut_slice()).is_ok() {
            shortcut.arguments = wide_to_string(&arg_buffer).filter(|value| !value.is_empty());
        }

        let mut working_dir_buffer = vec![0u16; BUFFER_LEN];
        if shell_link
            .GetWorkingDirectory(working_dir_buffer.as_mut_slice())
            .is_ok()
        {
            shortcut.working_directory =
                wide_to_string(&working_dir_buffer).filter(|value| !value.is_empty());
        }

        let mut desc_buffer = vec![0u16; BUFFER_LEN];
        if shell_link
            .GetDescription(desc_buffer.as_mut_slice())
            .is_ok()
        {
            shortcut.description =
                wide_to_string(&desc_buffer).filter(|value| !value.trim().is_empty());
        }

        let mut icon_buffer = vec![0u16; BUFFER_LEN];
        let mut icon_index = 0i32;
        if shell_link
            .GetIconLocation(icon_buffer.as_mut_slice(), &mut icon_index)
            .is_ok()
        {
            shortcut.icon_path =
                wide_to_string(&icon_buffer).filter(|value| !value.trim().is_empty());
            shortcut.icon_index = icon_index;
        }

        Some(shortcut)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        None
    }
}

/// Parses a `.url` Internet shortcut and extracts the target URL plus optional icon metadata.
pub(crate) fn parse_internet_shortcut(path: &Path) -> Option<InternetShortcutInfo> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }

    let content = decode_shortcut_contents(&bytes)?;
    let mut in_section = false;
    let mut url = None;
    let mut icon_path = None;
    let mut icon_index = 0i32;
    let mut description = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_section = line.eq_ignore_ascii_case("[internetshortcut]");
            continue;
        }

        if !in_section {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key_lower = key.trim().to_ascii_lowercase();
        let cleaned_value = value.trim().trim_matches(|c| c == '\'' || c == '"');
        if cleaned_value.is_empty() {
            continue;
        }

        match key_lower.as_str() {
            "url" => url = Some(cleaned_value.to_string()),
            "iconfile" => icon_path = Some(cleaned_value.to_string()),
            "iconindex" => {
                if let Ok(parsed) = cleaned_value.parse::<i32>() {
                    icon_index = parsed;
                }
            }
            "description" | "comment" => {
                description = Some(cleaned_value.to_string());
            }
            _ => {}
        }
    }

    let url = url?;
    Some(InternetShortcutInfo {
        url,
        icon_path,
        icon_index,
        description,
    })
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

fn decode_shortcut_contents(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(decode_utf16(&bytes[2..], true))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(decode_utf16(&bytes[2..], false))
    } else {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        if let Some(stripped) = text.strip_prefix('\u{feff}') {
            text = stripped.to_string();
        }
        Some(text)
    }
}

fn decode_utf16(data: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let value = if little_endian {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], chunk[1]])
        };
        units.push(value);
    }
    String::from_utf16_lossy(&units)
}

fn icon_cache_dir() -> Option<PathBuf> {
    let base = env::var("LOCALAPPDATA").ok()?;
    Some(Path::new(&base).join("egg").join("icons"))
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

/// Switches the current keyboard layout to English (US) so the search框默认使用英文输入法。
pub(crate) fn switch_to_english_input_method() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyboardLayout, ActivateKeyboardLayout,
        };
        use windows::core::w;
        
        log::info!("=== Switching to English IME ===");
        
        // Get current layout before switching
        let current_layout = GetKeyboardLayout(0);
        log::info!("Current layout before switch: 0x{:x}", current_layout.0 as isize);
        
        // First, get or load the English layout handle
        let en_us_layout = match LoadKeyboardLayoutW(w!("00000409"), KLF_ACTIVATE) {
            Ok(value) => {
                log::info!("English layout handle: 0x{:x}", value.0 as isize);
                value
            }
            Err(error) => {
                warn!("failed to load EN-US keyboard layout: {error:?}");
                return;
            }
        };

        // Check if already English
        if current_layout == en_us_layout {
            log::info!("Already using English layout, no switch needed");
        } else {
            // Use KLF_ACTIVATE to switch
            if let Err(error) = ActivateKeyboardLayout(en_us_layout, KLF_ACTIVATE) {
                warn!("failed to activate EN-US keyboard layout: {error:?}");
            } else {
                log::info!("Successfully switched to English");
            }
        }
    }
}

/// Gets the current input method (keyboard layout) for the current thread.
pub(crate) fn get_current_input_method() -> Option<isize> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
        let layout = GetKeyboardLayout(0);
        if layout.is_invalid() {
            log::warn!("Failed to get current keyboard layout");
            None
        } else {
            let layout_id = layout.0 as isize;
            log::info!("Saving IME layout: 0x{:x}", layout_id);
            Some(layout_id)
        }
    }

    #[cfg(not(target_os = "windows"))]
    None
}

/// Restores a previously saved input method.
pub(crate) fn restore_input_method(layout_id: isize) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
        log::info!("=== Restoring IME layout: 0x{:x} ===", layout_id);
        let layout = HKL(layout_id as *mut _);
        // Use KLF_ACTIVATE to restore the layout
        if let Err(error) = ActivateKeyboardLayout(layout, KLF_ACTIVATE) {
            warn!("failed to restore keyboard layout: {error:?}");
        } else {
            log::info!("Successfully restored IME layout");
        }
    }
}

/// Enables or disables Windows auto-start via the "Run" registry key.
pub(crate) fn configure_launch_on_startup(enable: bool) -> std::result::Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        const VALUE_NAME: &str = "egg";

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(RUN_KEY).map_err(|err| err.to_string())?;

        if enable {
            let exe_path = env::current_exe().map_err(|err| err.to_string())?;
            let exe_value = {
                let raw = exe_path.as_os_str().to_string_lossy();
                if raw.contains(' ') {
                    format!("\"{raw}\"")
                } else {
                    raw.into_owned()
                }
            };
            key.set_value(VALUE_NAME, &exe_value)
                .map_err(|err| err.to_string())
        } else {
            match key.delete_value(VALUE_NAME) {
                Ok(_) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Ok(())
    }
}
