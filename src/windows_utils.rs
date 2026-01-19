use std::{env, ffi::OsStr, os::windows::ffi::OsStrExt};

use log::warn;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, LoadKeyboardLayoutW, KLF_ACTIVATE,
};
#[cfg(target_os = "windows")]
use winreg::{enums::*, RegKey};

/// Converts an [`OsStr`] into a null-terminated wide string buffer suitable for Win32 APIs.
pub(crate) fn os_str_to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

/// Centers and focuses the current console window.
pub(crate) fn focus_and_center_console_window() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::{
            Foundation::RECT,
            System::Console::GetConsoleWindow,
            UI::WindowsAndMessaging::{
                GetSystemMetrics, GetWindowRect, MoveWindow, SetForegroundWindow, ShowWindow,
                SM_CXSCREEN, SM_CYSCREEN, SW_RESTORE,
            },
        };

        let hwnd = GetConsoleWindow();
        if hwnd.0.is_null() {
            return;
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return;
        }

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let x = ((screen_width - width) / 2).max(0);
        let y = ((screen_height - height) / 2).max(0);

        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = MoveWindow(hwnd, x, y, width, height, true);
        let _ = SetForegroundWindow(hwnd);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // no-op
    }
}

/// Switches the current keyboard layout to English (US) so the search框默认使用英文输入法。
#[allow(dead_code)]
pub(crate) fn switch_to_english_input_method() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::core::w;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            ActivateKeyboardLayout, GetKeyboardLayout,
        };

        log::info!("=== Switching to English IME ===");

        let current_layout = GetKeyboardLayout(0);
        log::info!(
            "Current layout before switch: 0x{:x}",
            current_layout.0 as isize
        );

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

        if current_layout == en_us_layout {
            log::info!("Already using English layout, no switch needed");
        } else if let Err(error) = ActivateKeyboardLayout(en_us_layout, KLF_ACTIVATE) {
            warn!("failed to activate EN-US keyboard layout: {error:?}");
        } else {
            log::info!("Successfully switched to English");
        }
    }
}

/// Gets the current input method (keyboard layout) for the current thread.
#[allow(dead_code)]
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
#[allow(dead_code)]
pub(crate) fn restore_input_method(layout_id: isize) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
        log::info!("=== Restoring IME layout: 0x{:x} ===", layout_id);
        let layout = HKL(layout_id as *mut _);
        if let Err(error) = ActivateKeyboardLayout(layout, KLF_ACTIVATE) {
            warn!("failed to restore keyboard layout: {error:?}");
        } else {
            log::info!("Successfully restored IME layout");
        }
    }
}

/// Enables or disables Windows auto-start via the "Run" registry key.
#[allow(dead_code)]
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
                let base = if raw.contains(' ') {
                    format!("\"{raw}\"")
                } else {
                    raw.into_owned()
                };
                format!("{base} --daemon")
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
