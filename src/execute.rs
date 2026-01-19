use std::{
    ffi::{OsStr, OsString},
    ptr,
};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::HWND,
        UI::Shell::ShellExecuteW,
        UI::WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::{
    models::ApplicationInfo,
    state::PendingAction,
    windows_utils::os_str_to_wide,
};

/// Execute a pending action (launch app, open URL, etc.)
pub fn execute_action(action: &PendingAction, run_as_admin: bool) -> Result<(), String> {
    match action {
        PendingAction::Application(app) => launch_application(app, run_as_admin),
        PendingAction::Bookmark(entry) => open_url(&entry.url),
        PendingAction::Url(url) | PendingAction::Search(url) => open_url(url),
    }
}

fn open_url(target: &str) -> Result<(), String> {
    open::that(target).map_err(|err| err.to_string())
}

fn launch_application(app: &ApplicationInfo, run_as_admin: bool) -> Result<(), String> {
    let target = app.path.trim();
    if target.is_empty() {
        return Err("目标程序无效".into());
    }

    let arguments = app.arguments.as_deref();
    let working_directory = app.working_directory.as_deref();
    let allow_runas = run_as_admin && should_use_runas(target);

    match shell_execute_raw(target, arguments, working_directory, allow_runas) {
        Ok(_) => Ok(()),
        Err(err) => {
            if let Some(source) = app.source_path.as_deref() {
                shell_execute_raw(source, arguments, working_directory, allow_runas).or(Err(err))
            } else {
                Err(err)
            }
        }
    }
}

fn should_use_runas(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    !(lower.starts_with("shell:") || lower.contains("://"))
}

fn shell_execute_raw(
    target: &str,
    arguments: Option<&str>,
    working_directory: Option<&str>,
    run_as_admin: bool,
) -> Result<(), String> {
    let target_os = OsString::from(target);
    let argument_os = arguments
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(OsString::from);
    let working_dir_os = working_directory
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(OsString::from);

    let verb = if run_as_admin {
        OsStr::new("runas")
    } else {
        OsStr::new("open")
    };

    shell_execute_internal(
        target_os.as_os_str(),
        argument_os.as_deref(),
        working_dir_os.as_deref(),
        verb,
    )
}

fn shell_execute_internal(
    target: &OsStr,
    arguments: Option<&OsStr>,
    working_directory: Option<&OsStr>,
    verb: &OsStr,
) -> Result<(), String> {
    let file_buffer = os_str_to_wide(target);
    let arg_buffer = arguments.map(os_str_to_wide);
    let dir_buffer = working_directory.map(os_str_to_wide);
    let verb_buffer = os_str_to_wide(verb);

    let arg_ptr = arg_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let dir_ptr = dir_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let verb_ptr = PCWSTR(verb_buffer.as_ptr());

    let result = unsafe {
        ShellExecuteW(
            HWND(ptr::null_mut()),
            verb_ptr,
            PCWSTR(file_buffer.as_ptr()),
            arg_ptr,
            dir_ptr,
            SW_SHOWNORMAL,
        )
    };

    if result.0 as isize <= 32 {
        Err(format!(
            "无法启动程序 (ShellExecute 错误码 {})",
            result.0 as isize
        ))
    } else {
        Ok(())
    }
}
