use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

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
