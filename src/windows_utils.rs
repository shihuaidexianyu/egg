use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

/// Converts an [`OsStr`] into a null-terminated wide string buffer suitable for Win32 APIs.
pub(crate) fn os_str_to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
