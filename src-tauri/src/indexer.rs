use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

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
    windows_utils::{
        expand_env_vars, extract_icon_from_path, parse_internet_shortcut, resolve_shell_link,
    },
};

/// Build the application index by scanning Start Menu shortcuts, installed Win32 software and UWP apps.
pub async fn build_index(exclusion_paths: Vec<String>) -> Vec<ApplicationInfo> {
    let mut results = Vec::new();

    let start_menu = match async_runtime::spawn_blocking(enumerate_start_menu_programs).await {
        Ok(apps) => apps,
        Err(err) => {
            warn!("start menu index task failed: {err}");
            Vec::new()
        }
    };
    debug!("indexed {} start menu shortcuts", start_menu.len());
    results.extend(start_menu);

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

    // De-duplicate by resolved target path while keeping Start Menu preference over registry entries.
    let mut seen: HashSet<(AppType, String, Option<String>)> = HashSet::new();
    results.retain(|app| {
        let key_path = app
            .source_path
            .as_ref()
            .unwrap_or(&app.path)
            .to_ascii_lowercase();
        let argument_key = app
            .arguments
            .as_ref()
            .map(|value| value.to_ascii_lowercase());
        seen.insert((app.app_type.clone(), key_path, argument_key))
    });
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    
    // Filter out system tools based on path
    results.retain(|app| !is_system_tool(app, &exclusion_paths));
    
    results
}

/// Check if an application is a Windows system tool based on its path
fn is_system_tool(app: &ApplicationInfo, exclusion_paths: &[String]) -> bool {
    let path_to_check = app.source_path.as_ref().unwrap_or(&app.path);
    let path_lower = path_to_check.to_ascii_lowercase();
    
    // Check if the app is in any excluded directory
    for sys_path in exclusion_paths {
        let sys_path_lower = sys_path.to_ascii_lowercase();
        if path_lower.starts_with(&sys_path_lower) {
            return true;
        }
    }
    
    false
}
const UNINSTALL_SUBKEYS: &[&str] = &[
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
];

const SUPPORTED_URL_PROTOCOLS: &[&str] = &["steam://", "com.epicgames.launcher://apps/"];

fn enumerate_start_menu_programs() -> Vec<ApplicationInfo> {
    let startup_dirs = startup_directories();
    let mut applications = Vec::new();

    for root in start_menu_roots() {
        if !root.is_dir() {
            continue;
        }

        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };

                if file_type.is_dir() {
                    stack.push(path);
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                if startup_dirs.iter().any(|startup| path.starts_with(startup)) {
                    continue;
                }

                let extension = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_ascii_lowercase());

                match extension.as_deref() {
                    Some("lnk") => {
                        if let Some(app) = shortcut_to_application(&path) {
                            applications.push(app);
                        }
                    }
                    Some("url") => {
                        if let Some(app) = internet_shortcut_to_application(&path) {
                            applications.push(app);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    applications
}

fn shortcut_to_application(path: &Path) -> Option<ApplicationInfo> {
    let shortcut = resolve_shell_link(path)?;
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }

    let resolved_target = shortcut
        .target_path
        .as_deref()
        .and_then(|raw| sanitize_executable_path(raw));
    let display_target = resolved_target
        .clone()
        .or_else(|| shortcut.target_path.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if display_target
        .as_ref()
        .map(|value| looks_like_uninstaller(value))
        .unwrap_or(false)
        || looks_like_uninstaller(&name)
    {
        return None;
    }

    let mut keywords = vec![name.clone()];
    if let Some(ref target) = display_target {
        keywords.push(target.clone());
        if let Some(file_name) = Path::new(target)
            .file_name()
            .and_then(|value| value.to_str())
        {
            keywords.push(file_name.to_string());
        }
    }
    if let Some(desc) = shortcut.description.clone() {
        keywords.push(desc.clone());
    }
    keywords.retain(|value| !value.trim().is_empty());
    extend_keywords_with_pinyin(&mut keywords);
    keywords.sort();
    keywords.dedup();

    let icon_candidate = shortcut.icon_path.as_deref().and_then(sanitize_icon_source);
    let icon_source = icon_candidate
        .or_else(|| display_target.clone())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let icon_b64 = extract_icon_from_path(&icon_source, shortcut.icon_index).unwrap_or_default();

    let description = shortcut
        .description
        .filter(|value| !value.trim().is_empty());
    let path_string = path.to_string_lossy().into_owned();
    let working_directory = shortcut.working_directory.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let arguments = shortcut.arguments.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    Some(ApplicationInfo {
        id: format!("win32:startmenu:{}", path_string.to_lowercase()),
        name,
        path: path_string,
        source_path: display_target,
        app_type: AppType::Win32,
        icon_b64,
        description,
        keywords,
        working_directory,
        arguments,
    })
}

fn internet_shortcut_to_application(path: &Path) -> Option<ApplicationInfo> {
    let shortcut = parse_internet_shortcut(path)?;
    let url = shortcut.url.trim();
    if url.is_empty() {
        return None;
    }

    let normalized_url = url.to_string();
    let lower_url = normalized_url.to_ascii_lowercase();
    if !SUPPORTED_URL_PROTOCOLS
        .iter()
        .any(|prefix| lower_url.starts_with(prefix))
    {
        return None;
    }

    let name = path
        .file_stem()
        .and_then(|value| value.to_str())?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }

    let mut keywords = vec![name.clone(), normalized_url.clone()];
    if let Some(desc) = shortcut
        .description
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        keywords.push(desc.to_string());
    }
    extend_keywords_with_pinyin(&mut keywords);
    keywords.sort();
    keywords.dedup();

    let icon_candidate = shortcut.icon_path.as_deref().and_then(sanitize_icon_source);
    let icon_source = icon_candidate.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let icon_b64 = extract_icon_from_path(&icon_source, shortcut.icon_index).unwrap_or_default();
    let path_string = path.to_string_lossy().into_owned();
    let description = shortcut
        .description
        .filter(|value| !value.trim().is_empty());

    Some(ApplicationInfo {
        id: format!("win32:url:{}", path_string.to_lowercase()),
        name,
        path: path_string,
        source_path: Some(normalized_url),
        app_type: AppType::Win32,
        icon_b64,
        description,
        keywords,
        working_directory: None,
        arguments: None,
    })
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(app_data) = env::var_os("APPDATA") {
        roots.push(PathBuf::from(app_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(program_data) = env::var_os("PROGRAMDATA") {
        roots.push(PathBuf::from(program_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }

    roots.into_iter().filter(|path| path.is_dir()).collect()
}

fn startup_directories() -> Vec<PathBuf> {
    let mut startup = Vec::new();
    if let Some(app_data) = env::var_os("APPDATA") {
        startup.push(
            PathBuf::from(app_data).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"),
        );
    }
    if let Some(program_data) = env::var_os("PROGRAMDATA") {
        startup.push(
            PathBuf::from(program_data).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"),
        );
    }

    startup.into_iter().filter(|path| path.is_dir()).collect()
}

fn sanitize_icon_source(raw: &str) -> Option<String> {
    let expanded = expand_env_vars(raw).unwrap_or_else(|| raw.to_string());
    if Path::new(&expanded).exists() {
        Some(expanded)
    } else {
        None
    }
}

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
        path: path.clone(),
        source_path: Some(path),
        app_type: AppType::Win32,
        icon_b64,
        description,
        keywords,
        working_directory: None,
        arguments: None,
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
                source_path: None,
                app_type: AppType::Uwp,
                icon_b64,
                description,
                keywords,
                working_directory: None,
                arguments: None,
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
