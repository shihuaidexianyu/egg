use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

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
    let (shell_task, start_menu_task) = tokio::join!(
        tokio::task::spawn_blocking(enumerate_shell_apps),
        tokio::task::spawn_blocking(enumerate_start_menu_urls),
    );
    let mut results = match shell_task {
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

    let start_menu = match start_menu_task {
        Ok(apps) => apps,
        Err(err) => {
            warn!("start menu index task failed: {err}");
            Vec::new()
        }
    };
    debug!("indexed {} start menu urls", start_menu.len());
    results.extend(start_menu);

    let mut seen: HashSet<String> = HashSet::new();
    results.retain(|app| seen.insert(app.path.to_ascii_lowercase()));
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    results.retain(|app| !is_system_tool(app, &exclusion_paths));

    results
}

fn is_system_tool(app: &ApplicationInfo, exclusion_paths: &[String]) -> bool {
    let path_to_check = app.source_path.as_ref().unwrap_or(&app.path);
    let path_lower = path_to_check.to_ascii_lowercase();

    for sys_path in exclusion_paths {
        let sys_path_lower = sys_path.trim().to_ascii_lowercase();
        if sys_path_lower.is_empty() {
            continue;
        }
        if path_lower.starts_with(&sys_path_lower) {
            return true;
        }
        if sys_path_lower.starts_with('{') && path_lower.contains(&sys_path_lower) {
            return true;
        }
    }

    false
}

fn looks_like_file_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains(":\\") || lower.contains(":/") || lower.starts_with("\\\\")
}

const SUPPORTED_URL_PROTOCOLS: &[&str] = &["steam://", "com.epicgames.launcher://apps/"];

fn enumerate_start_menu_urls() -> Vec<ApplicationInfo> {
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

                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("url"))
                {
                    if let Some(app) = internet_shortcut_to_application(&path) {
                        applications.push(app);
                    }
                }
            }
        }
    }

    applications
}

fn internet_shortcut_to_application(path: &Path) -> Option<ApplicationInfo> {
    let shortcut = parse_internet_shortcut(path)?;
    let url = shortcut.url.trim();
    if url.is_empty() {
        return None;
    }

    let lower_url = url.to_ascii_lowercase();
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

    let mut keywords = vec![name.clone(), url.to_string()];
    if let Some(desc) = shortcut
        .description
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        keywords.push(desc.to_string());
    }
    keywords.sort();
    keywords.dedup();
    let description = shortcut
        .description
        .filter(|value| !value.trim().is_empty());
    let pinyin_index = build_pinyin_index(
        [Some(name.as_str()), description.as_deref()]
            .into_iter()
            .flatten(),
    );
    let path_string = path.to_string_lossy().into_owned();

    Some(ApplicationInfo {
        id: format!("url:startmenu:{}", path_string.to_ascii_lowercase()),
        name,
        path: url.to_string(),
        source_path: Some(path_string),
        app_type: AppType::Win32,
        description,
        keywords,
        pinyin_index,
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

#[derive(Debug, Clone)]
struct InternetShortcutInfo {
    url: String,
    description: Option<String>,
}

fn parse_internet_shortcut(path: &Path) -> Option<InternetShortcutInfo> {
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
    Some(InternetShortcutInfo { url, description })
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

fn is_blacklisted_shell_item(name: &str, parsing_name: &str) -> bool {
    if looks_like_uninstaller(name) || looks_like_uninstaller(parsing_name) {
        return true;
    }

    let trimmed = parsing_name.trim();
    if trimmed.is_empty() {
        return true;
    }

    if looks_like_file_path(trimmed) {
        let path = Path::new(trimmed);
        if !path.is_file() {
            return true;
        }

        if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            if matches!(
                ext_lower.as_str(),
                "dll" | "sys" | "drv" | "pnf" | "mui" | "dat" | "log"
            ) {
                return true;
            }
        }
    }

    false
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
            if is_blacklisted_shell_item(&name, &parsing_name) {
                continue;
            }

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
