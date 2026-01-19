use std::{
    env,
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
};

use log::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, LoadKeyboardLayoutW, KLF_ACTIVATE,
};
use windows::{
    core::{Error, Interface, Result, PCWSTR},
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Storage::FileSystem::WIN32_FIND_DATAW,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
            },
            Environment::ExpandEnvironmentStringsW,
        },
        UI::{
            Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH, SLGP_UNCPRIORITY},
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
}

#[derive(Debug, Clone)]
pub(crate) struct InternetShortcutInfo {
    pub url: String,
    pub description: Option<String>,
}

/// Resolves `.lnk` shortcuts and extracts metadata such as target executable and arguments.
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

        Some(shortcut)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        None
    }
}

/// Parses a `.url` Internet shortcut and extracts the target URL plus metadata.
pub(crate) fn parse_internet_shortcut(path: &Path) -> Option<InternetShortcutInfo> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }

    let content = decode_shortcut_contents(&bytes)?;
    let mut in_section = false;
    let mut url = None;
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
            "description" | "comment" => {
                description = Some(cleaned_value.to_string());
            }
            _ => {}
        }
    }

    let url = url?;
    Some(InternetShortcutInfo {
        url,
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
