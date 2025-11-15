use std::{collections::HashSet, fs, path::Path};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{debug, error, warn};
use tauri::async_runtime;
use windows::{
    core::Result as WinResult, Foundation::Size, Management::Deployment::PackageManager,
    Storage::Streams::DataReader,
};
use winreg::{enums::*, RegKey};

use crate::{
    models::{AppType, ApplicationInfo},
    text_utils::extend_keywords_with_pinyin,
    windows_utils::{expand_env_vars, extract_icon_from_path},
};

/// Build the application index by scanning Start Menu shortcuts and UWP apps.
pub async fn build_index() -> Vec<ApplicationInfo> {
    let mut results = Vec::new();

    let win32 = match async_runtime::spawn_blocking(enumerate_installed_win32_apps).await {
        Ok(apps) => apps,
        Err(err) => {
            error!("win32 index task failed: {err}");
            Vec::new()
        }
    };
    debug!("indexed {} installed Win32 apps", win32.len());
    results.extend(win32);

    match enumerate_uwp_apps().await {
        Ok(mut uwp_apps) => {
            debug!("indexed {} UWP entries", uwp_apps.len());
            results.append(&mut uwp_apps);
        }
        Err(err) => warn!("failed to enumerate UWP apps: {err}"),
    }

    // De-duplicate by id while keeping first occurrence ordering preference: Win32 before UWP.
    let mut seen = HashSet::new();
    results.retain(|app| seen.insert(app.id.clone()));
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    results
}
const UNINSTALL_SUBKEYS: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
];

fn enumerate_installed_win32_apps() -> Vec<ApplicationInfo> {
    let mut applications = Vec::new();
    let mut seen = HashSet::new();
    let roots = [
        RegKey::predef(HKEY_LOCAL_MACHINE),
        RegKey::predef(HKEY_CURRENT_USER),
    ];

    for root in roots {
        for subkey in UNINSTALL_SUBKEYS {
            let Ok(uninstall_key) = root.open_subkey(subkey) else {
                continue;
            };

            for entry in uninstall_key.enum_keys().flatten() {
                let Ok(app_key) = uninstall_key.open_subkey(&entry) else {
                    continue;
                };

                if let Some(app) = registry_entry_to_app(&app_key, subkey, &entry) {
                    if seen.insert(app.id.clone()) {
                        applications.push(app);
                    }
                }
            }
        }
    }

    applications
}

fn registry_entry_to_app(
    key: &RegKey,
    parent_path: &str,
    entry_name: &str,
) -> Option<ApplicationInfo> {
    // Skip system or hidden components.
    if key.get_value::<u32, _>("SystemComponent").ok() == Some(1) {
        return None;
    }
    if key.get_value::<u32, _>("NoDisplay").ok() == Some(1) {
        return None;
    }

    let display_name: String = key
        .get_value::<String, _>("DisplayName")
        .ok()?
        .trim()
        .to_string();
    if display_name.is_empty() {
        return None;
    }

    let display_icon_path = key
        .get_value::<String, _>("DisplayIcon")
        .ok()
        .and_then(|value| sanitize_executable_path(&value));

    let explicit_executable = key
        .get_value::<String, _>("ExecutablePath")
        .ok()
        .and_then(|value| sanitize_executable_path(&value));

    let install_executable = key
        .get_value::<String, _>("InstallLocation")
        .ok()
        .and_then(|value| fallback_executable_from_folder(&value));

    let install_source_executable = key
        .get_value::<String, _>("InstallSource")
        .ok()
        .and_then(|value| fallback_executable_from_folder(&value));

    let path = install_executable
        .or(explicit_executable)
        .or_else(|| {
            display_icon_path
                .clone()
                .filter(|candidate| !looks_like_uninstaller(candidate))
        })
        .or(install_source_executable)?;

    let description = key
        .get_value::<String, _>("Publisher")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let mut keywords = Vec::new();
    keywords.push(display_name.clone());
    if let Some(desc) = description.clone() {
        keywords.push(desc);
    }
    if let Ok(version) = key.get_value::<String, _>("DisplayVersion") {
        if !version.trim().is_empty() {
            keywords.push(version);
        }
    }

    keywords.retain(|value| !value.trim().is_empty());
    extend_keywords_with_pinyin(&mut keywords);
    keywords.sort();
    keywords.dedup();

    let icon_source = display_icon_path.unwrap_or_else(|| path.clone());
    let icon_b64 = extract_icon_from_path(&icon_source, 0).unwrap_or_default();

    Some(ApplicationInfo {
        id: format!("win32:installed:{}:{}", parent_path, entry_name).to_lowercase(),
        name: display_name,
        path,
        app_type: AppType::Win32,
        icon_b64,
        description,
        keywords,
    })
}

fn sanitize_executable_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_quotes = trimmed.trim_matches(|c| c == '"' || c == '\'');
    let candidate = without_quotes
        .split(&[',', ';'][..])
        .next()
        .map(str::trim)?;
    if candidate.is_empty() {
        return None;
    }

    let expanded = expand_env_vars(candidate).unwrap_or_else(|| candidate.to_string());
    let path = Path::new(&expanded);
    if path.is_file() {
        Some(expanded)
    } else {
        None
    }
}

fn fallback_executable_from_folder(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_env_vars(trimmed).unwrap_or_else(|| trimmed.to_string());
    let normalized_folder = expanded.trim_end_matches(['/', '\\']).to_string();
    if normalized_folder.is_empty() {
        return None;
    }
    let folder_path = Path::new(&normalized_folder);
    if !folder_path.is_dir() {
        return None;
    }

    let mut candidates = Vec::new();
    if let Ok(entries) = fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let file_type = entry.file_type().ok();
            if file_type.is_none_or(|ft| !ft.is_file()) {
                continue;
            }
            let file_path = entry.path();
            if file_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
            {
                candidates.push(file_path);
            }
        }
    }

    candidates
        .into_iter()
        .max_by_key(|path| path.metadata().ok().map(|m| m.len()).unwrap_or(0))
        .and_then(|path| path.into_os_string().into_string().ok())
}

fn looks_like_uninstaller(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("unins") || lower.contains("uninstall")
}

async fn enumerate_uwp_apps() -> WinResult<Vec<ApplicationInfo>> {
    let manager = PackageManager::new()?;
    let mut applications = Vec::new();

    let iterable = manager.FindPackages()?;
    let iterator = iterable.First()?;
    while iterator.HasCurrent()? {
        let package = iterator.Current()?;
        iterator.MoveNext()?;

        let entries_future = package.GetAppListEntriesAsync()?;
        let entries = entries_future.get()?;

        let size = entries.Size()?;
        for index in 0..size {
            let entry = entries.GetAt(index)?;

            let app_id = entry.AppUserModelId()?.to_string();
            let display_info = entry.DisplayInfo()?;
            let display_name = display_info.DisplayName()?.to_string();
            let description = display_info
                .Description()
                .ok()
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty());

            let mut keywords = Vec::new();
            if let Some(desc) = description.clone() {
                keywords.push(desc);
            }
            keywords.push(display_name.clone());
            keywords.push(app_id.clone());

            if let Ok(package_id) = package.Id() {
                if let Ok(name) = package_id.Name() {
                    keywords.push(name.to_string());
                }
                if let Ok(family) = package_id.FamilyName() {
                    keywords.push(family.to_string());
                }
                if let Ok(full) = package_id.FullName() {
                    keywords.push(full.to_string());
                }
            }
            keywords.retain(|value| !value.is_empty());
            extend_keywords_with_pinyin(&mut keywords);
            keywords.sort();
            keywords.dedup();

            let icon_b64 = load_uwp_logo(&display_info).unwrap_or_default();

            applications.push(ApplicationInfo {
                id: format!("uwp:{}", app_id.to_lowercase()),
                name: display_name,
                path: app_id,
                app_type: AppType::Uwp,
                icon_b64,
                description,
                keywords,
            });
        }
    }

    Ok(applications)
}

fn load_uwp_logo(display_info: &windows::ApplicationModel::AppDisplayInfo) -> Option<String> {
    let logo_ref = display_info
        .GetLogo(Size {
            Width: 64.0,
            Height: 64.0,
        })
        .ok()?;

    let stream = logo_ref.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as usize;
    if size == 0 {
        let _ = stream.Close();
        return None;
    }

    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut buffer = vec![0u8; size];
    if reader.ReadBytes(buffer.as_mut_slice()).is_err() {
        let _ = reader.Close();
        let _ = stream.Close();
        return None;
    }
    let _ = reader.Close();
    let _ = stream.Close();

    Some(BASE64.encode(buffer))
}
