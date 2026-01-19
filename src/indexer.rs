use std::collections::HashSet;

use log::{debug, warn};
use windows::{
    core::{Error as WinError, Result as WinResult, PWSTR},
    Win32::{
        Foundation::{HANDLE, RPC_E_CHANGED_MODE},
        System::{
            Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
            SystemServices::SFGAO_HIDDEN,
        },
        UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
            KF_FLAG_DEFAULT, SIGDN, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
        },
    },
};

use crate::{
    models::{AppType, ApplicationInfo},
    text_utils::build_pinyin_index,
};

/// Build the application index by enumerating the AppsFolder shell items.
pub async fn build_index(exclusion_paths: Vec<String>) -> Vec<ApplicationInfo> {
    let shell_task = tokio::task::spawn_blocking(enumerate_shell_apps);
    let mut results = match shell_task.await {
        Ok(Ok(apps)) => apps,
        Ok(Err(err)) => {
            warn!("shell apps index failed: {err}");
            Vec::new()
        }
        Err(err) => {
            warn!("shell apps index task failed: {err}");
            Vec::new()
        }
    };
    debug!("indexed {} shell apps", results.len());

    let mut seen: HashSet<String> = HashSet::new();
    results.retain(|app| seen.insert(app.path.to_ascii_lowercase()));
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    results.retain(|app| !is_system_tool(app, &exclusion_paths));

    results
}

fn is_system_tool(app: &ApplicationInfo, exclusion_paths: &[String]) -> bool {
    let path_to_check = app.source_path.as_ref().unwrap_or(&app.path);
    if !looks_like_file_path(path_to_check) {
        return false;
    }
    let path_lower = path_to_check.to_ascii_lowercase();

    for sys_path in exclusion_paths {
        let sys_path_lower = sys_path.to_ascii_lowercase();
        if path_lower.starts_with(&sys_path_lower) {
            return true;
        }
    }

    false
}

fn looks_like_file_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains(":\\") || lower.contains(":/") || lower.starts_with("\\\\")
}

struct ComInitGuard {
    initialized: bool,
}

impl ComInitGuard {
    unsafe fn new() -> WinResult<Self> {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_ok() {
            Ok(Self { initialized: true })
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self { initialized: false })
        } else {
            Err(WinError::from(hr))
        }
    }
}

impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

struct CoTaskMemGuard(PWSTR);

impl Drop for CoTaskMemGuard {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            CoTaskMemFree(Some(self.0.as_ptr().cast()));
        }
    }
}

fn enumerate_shell_apps() -> WinResult<Vec<ApplicationInfo>> {
    unsafe {
        let _com_guard = ComInitGuard::new()?;
        let apps_folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, HANDLE::default())?;
        let enumerator: IEnumShellItems = apps_folder.BindToHandler(None, &BHID_EnumItems)?;

        let mut applications = Vec::new();
        loop {
            let mut fetched = 0u32;
            let mut items: [Option<IShellItem>; 1] = [None];
            enumerator.Next(&mut items, Some(&mut fetched))?;
            if fetched == 0 {
                break;
            }

            let Some(item) = items[0].take() else {
                continue;
            };

            if is_shell_item_hidden(&item) {
                continue;
            }

            let name = match shell_item_display_name(&item, SIGDN_NORMALDISPLAY) {
                Some(name) => name,
                None => continue,
            };
            if looks_like_uninstaller(&name) {
                continue;
            }

            let parsing_name = match shell_item_display_name(&item, SIGDN_DESKTOPABSOLUTEPARSING) {
                Some(value) => value,
                None => continue,
            };

            let app_type = infer_shell_app_type(&parsing_name);
            let mut keywords = vec![name.clone(), parsing_name.clone()];
            keywords.sort();
            keywords.dedup();
            let pinyin_index = build_pinyin_index([Some(name.as_str())].into_iter().flatten());

            applications.push(ApplicationInfo {
                id: format!("shell:{}", parsing_name.to_ascii_lowercase()),
                name,
                path: parsing_name,
                source_path: None,
                app_type,
                description: None,
                keywords,
                pinyin_index,
                working_directory: None,
                arguments: None,
            });
        }

        Ok(applications)
    }
}

fn shell_item_display_name(item: &IShellItem, sigdn: SIGDN) -> Option<String> {
    let display = unsafe { item.GetDisplayName(sigdn).ok()? };
    if display.is_null() {
        return None;
    }
    let _guard = CoTaskMemGuard(display);
    let value = unsafe { display.to_string().ok()? };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_shell_item_hidden(item: &IShellItem) -> bool {
    match unsafe { item.GetAttributes(SFGAO_HIDDEN) } {
        Ok(attributes) => attributes.contains(SFGAO_HIDDEN),
        Err(_) => false,
    }
}

fn infer_shell_app_type(parsing_name: &str) -> AppType {
    let lower = parsing_name.to_ascii_lowercase();
    if lower.starts_with("shell:appsfolder\\") && lower.contains('!') {
        AppType::Uwp
    } else {
        AppType::Win32
    }
}

fn looks_like_uninstaller(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("unins") || lower.contains("uninstall")
}
