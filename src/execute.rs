use std::{
    ffi::{OsStr, OsString},
    path::Path,
    ptr,
};

use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
        UI::Shell::{
            ApplicationActivationManager, IApplicationActivationManager, ShellExecuteW,
            ACTIVATEOPTIONS,
        },
        UI::WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::{
    models::{AppType, ApplicationInfo},
    state::PendingAction,
    windows_utils::{os_str_to_wide, ComGuard},
};

/// Execute a pending action (launch app, open URL, etc.)
pub fn execute_action(action: &PendingAction, run_as_admin: bool) -> Result<(), String> {
    match action {
        PendingAction::Application(app) => match app.app_type {
            AppType::Win32 => launch_win32_app(app, run_as_admin),
            AppType::Uwp => launch_uwp_app(&app.path),
        },
        PendingAction::Bookmark(entry) => open_url(&entry.url),
        PendingAction::Url(url) | PendingAction::Search(url) => open_url(url),
    }
}

fn open_url(target: &str) -> Result<(), String> {
    open::that(target).map_err(|err| err.to_string())
}

fn launch_win32_app(app: &ApplicationInfo, run_as_admin: bool) -> Result<(), String> {
    let primary = Path::new(&app.path);
    match shell_execute_path(primary, run_as_admin) {
        Ok(_) => Ok(()),
        Err(primary_err) => {
            if let Some(source) = &app.source_path {
                launch_from_source(
                    source,
                    app.arguments.as_deref(),
                    app.working_directory.as_deref(),
                    run_as_admin,
                )
                .or(Err(primary_err))
            } else {
                Err(primary_err)
            }
        }
    }
}

fn shell_execute_path(path: &Path, run_as_admin: bool) -> Result<(), String> {
    if !path.exists() {
        return Err("目标程序不存在或已被移动".into());
    }

    let verb = if run_as_admin {
        Some(OsStr::new("runas"))
    } else {
        None
    };
    shell_execute_internal(path.as_os_str(), None, None, verb)
}

fn launch_uwp_app(app_id: &str) -> Result<(), String> {
    unsafe {
        let _guard = ComGuard::new().map_err(|err| err.to_string())?;

        let manager: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                .map_err(|err| err.to_string())?;

        let app_id = HSTRING::from(app_id);
        let _process_id = manager
            .ActivateApplication(&app_id, PCWSTR::null(), ACTIVATEOPTIONS::default())
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn launch_from_source(
    source: &str,
    arguments: Option<&str>,
    working_directory: Option<&str>,
    run_as_admin: bool,
) -> Result<(), String> {
    let normalized = source.trim().trim_matches(|c| c == '"' || c == '\'');
    if normalized.is_empty() {
        return Err("备用路径无效".into());
    }

    if normalized.contains("://") && !Path::new(normalized).exists() {
        return shell_execute_uri(normalized);
    }

    shell_execute_raw(normalized, arguments, working_directory, run_as_admin)
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
        Some(OsStr::new("runas"))
    } else {
        None
    };

    shell_execute_internal(
        target_os.as_os_str(),
        argument_os.as_deref(),
        working_dir_os.as_deref(),
        verb,
    )
}

fn shell_execute_uri(uri: &str) -> Result<(), String> {
    let uri_os = OsString::from(uri);
    shell_execute_internal(uri_os.as_os_str(), None, None, None)
}

fn shell_execute_internal(
    target: &OsStr,
    arguments: Option<&OsStr>,
    working_directory: Option<&OsStr>,
    verb: Option<&OsStr>,
) -> Result<(), String> {
    let file_buffer = os_str_to_wide(target);
    let arg_buffer = arguments.map(os_str_to_wide);
    let dir_buffer = working_directory.map(os_str_to_wide);
    let verb_buffer = verb.map(os_str_to_wide);

    let arg_ptr = arg_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let dir_ptr = dir_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let verb_ptr = verb_buffer
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or(PCWSTR::null());

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
