use std::{
    collections::HashSet,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    ptr,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{debug, error, warn};
use tauri::async_runtime;
use walkdir::WalkDir;
use windows::{
    core::{Interface, Result as WinResult, PCWSTR},
    Foundation::Size,
    Management::Deployment::PackageManager,
    Storage::Streams::DataReader,
    Win32::{
        Foundation::MAX_PATH,
        Storage::FileSystem::WIN32_FIND_DATAW,
        System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
        UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH, SLGP_UNCPRIORITY},
    },
};

use crate::{
    models::{AppType, ApplicationInfo},
    windows_utils::{extract_icon_from_path, os_str_to_wide, wide_to_string, ComGuard},
};

/// Build the application index by scanning Start Menu shortcuts and UWP apps.
pub async fn build_index() -> Vec<ApplicationInfo> {
    let mut results = Vec::new();

    let win32 = match async_runtime::spawn_blocking(build_win32_index).await {
        Ok(apps) => apps,
        Err(err) => {
            error!("win32 index task failed: {err}");
            Vec::new()
        }
    };
    debug!("indexed {} Win32 shortcuts", win32.len());
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

fn build_win32_index() -> Vec<ApplicationInfo> {
    let mut applications = Vec::new();
    let mut seen = HashSet::new();

    let com_guard = unsafe { ComGuard::new() };
    if let Err(err) = &com_guard {
        error!("failed to initialise COM for shortcut parsing: {err}");
    }
    // Keep guard alive for the entire traversal.
    let _guard = com_guard.ok();

    for dir in start_menu_locations() {
        if !dir.exists() {
            continue;
        }

        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !is_shortcut(path) {
                continue;
            }

            match parse_shortcut(path) {
                Ok(Some(app)) => {
                    if seen.insert(app.id.clone()) {
                        applications.push(app);
                    }
                }
                Ok(None) => {}
                Err(err) => warn!("failed to parse shortcut {:?}: {err}", path),
            }
        }
    }

    applications
}

fn start_menu_locations() -> Vec<PathBuf> {
    let mut locations = Vec::new();

    if let Ok(appdata) = env::var("APPDATA") {
        locations.push(PathBuf::from(appdata).join("Microsoft/Windows/Start Menu/Programs"));
    }

    if let Ok(program_data) = env::var("ProgramData") {
        locations.push(PathBuf::from(program_data).join("Microsoft/Windows/Start Menu/Programs"));
    }

    locations
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| ext.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
}

fn parse_shortcut(path: &Path) -> WinResult<Option<ApplicationInfo>> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist: IPersistFile = shell_link.cast()?;

        let wide_path = os_str_to_wide(path.as_os_str());
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ)?;

        let mut target = [0u16; MAX_PATH as usize];
        shell_link.GetPath(
            &mut target,
            ptr::null_mut::<WIN32_FIND_DATAW>(),
            (SLGP_UNCPRIORITY.0 | SLGP_RAWPATH.0) as u32,
        )?;

        let target_path = match wide_to_string(&target) {
            Some(value) => value,
            None => return Ok(None),
        };

        let mut description_buffer = [0u16; 512];
        let description = if shell_link.GetDescription(&mut description_buffer).is_ok() {
            wide_to_string(&description_buffer)
        } else {
            None
        };

        let fallback_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        let name = description
            .as_ref()
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or(fallback_name);

        let mut keywords = Vec::new();
        if let Some(desc) = description.clone() {
            if !desc.is_empty() {
                keywords.push(desc);
            }
        }
        if let Some(file_name) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|s| s.to_string())
        {
            keywords.push(file_name);
        }
        if let Some(target_name) = Path::new(&target_path)
            .file_name()
            .and_then(|stem| stem.to_str())
            .map(|s| s.to_string())
        {
            keywords.push(target_name);
        }
        keywords.retain(|value| !value.is_empty());
        keywords.sort();
        keywords.dedup();

        let mut icon_b64 = String::new();
        let mut icon_path_buffer = [0u16; MAX_PATH as usize];
        let mut icon_index = 0;
        if shell_link
            .GetIconLocation(&mut icon_path_buffer, &mut icon_index)
            .is_ok()
        {
            if let Some(icon_path) = wide_to_string(&icon_path_buffer) {
                if let Some(encoded) = extract_icon_from_path(&icon_path, icon_index) {
                    icon_b64 = encoded;
                }
            }
        }

        if icon_b64.is_empty() {
            if let Some(encoded) = extract_icon_from_path(&target_path, 0) {
                icon_b64 = encoded;
            }
        }

        let application = ApplicationInfo {
            id: format!("win32:{}", target_path.to_lowercase()),
            name,
            path: target_path,
            app_type: AppType::Win32,
            icon_b64,
            description,
            keywords,
        };

        Ok(Some(application))
    }
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
