这是一个非常棒的决定。将核心逻辑剥离为纯 CLI 工具不仅能让程序更轻量，也是调试搜索引擎逻辑的最佳方式。

你需要做的是将 `src-tauri` 目录转变为一个标准的 Rust Binary 项目，移除所有 Tauri 相关的依赖和代码，引入 `tokio` 作为异步运行时，并替换掉依赖 Tauri API 的部分（如路径获取、打开文件等）。

以下是详细的重构方案：

### 1. 清理 `Cargo.toml`

我们需要移除所有 Tauri 插件和构建工具，添加 `tokio` 和 `dirs`（用于获取配置路径）。

**覆盖 `src-tauri/Cargo.toml`：**

```toml
[package]
name = "egg-cli"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
fuzzy-matcher = "0.3"
base64 = "0.22"
urlencoding = "2"
image = { version = "0.24", default-features = false, features = ["png"] }
sha1 = "0.10"
winreg = "0.52"
log = "0.4"
env_logger = "0.11" # 新增：用于终端日志输出
pinyin = "0.10"
once_cell = "1"
tokio = { version = "1", features = ["full"] } # 新增：替代 Tauri 的运行时
dirs = "5.0" # 新增：替代 Tauri 的路径 API
open = "5.0" # 新增：替代 tauri-plugin-opener

[dependencies.windows]
version = "0.58"
features = [
    "Win32_System_Com",
    "Win32_System_Com_StructuredStorage",
    "Win32_System_WinRT",
    "Win32_System_LibraryLoader",
    "Win32_System_Threading",
    "Win32_UI_Shell",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_Storage_FileSystem",
    "Win32_Storage_EnhancedStorage",
    "Win32_Storage_StructuredStorage",
    "Win32_Graphics_Gdi",
    "Win32_System_Environment",
    "ApplicationModel",
    "ApplicationModel_Core",
    "Management_Deployment",
    "Foundation",
    "Foundation_Collections",
    "Storage_Streams",
]

```

---

### 2. 重构 `config.rs` (移除 Tauri 依赖)

原有的配置加载依赖 `AppHandle` 来获取路径，现在改用 `dirs` crate。

**修改 `src-tauri/src/config.rs`：**

```rust
use std::{fs, path::PathBuf};
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "settings.json";

// ... 保持 AppConfig 结构体和 Default 实现不变 ...
// (请保留原有的 struct AppConfig, default_* 函数, impl Default)

impl AppConfig {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = config_path() else {
            return Err("无法确定配置目录".into());
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        fs::write(path, data).map_err(|err| err.to_string())
    }
}

fn config_path() -> Option<PathBuf> {
    // 使用 dirs crate 获取 Roaming/Config 目录
    dirs::config_dir().map(|dir| dir.join("com.egg.app").join(CONFIG_FILE))
}

```

---

### 3. 重构 `state.rs` (移除 Tauri 依赖)

移除不必要的字段（如 `registered_hotkey`, `hotkey_capture_suspended`），这些是 GUI 交互特有的。

**修改 `src-tauri/src/state.rs`：**

```rust
use std::sync::{Arc, Mutex};
use crate::{bookmarks::BookmarkEntry, config::AppConfig, models::ApplicationInfo};

#[derive(Default, Clone)]
pub struct AppState {
    pub app_index: Arc<Mutex<Vec<ApplicationInfo>>>,
    pub bookmark_index: Arc<Mutex<Vec<BookmarkEntry>>>,
    pub config: Arc<Mutex<AppConfig>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            app_index: Arc::new(Mutex::new(Vec::new())),
            bookmark_index: Arc::new(Mutex::new(Vec::new())),
            config: Arc::new(Mutex::new(AppConfig::load())),
        }
    }
}

```

---

### 4. 核心逻辑重写 `core.rs` (原 `commands.rs`)

原 `commands.rs` 耦合了 Tauri 的 Command 系统。我们需要把搜索逻辑提取出来。

**新建/重命名为 `src-tauri/src/core.rs`：**

```rust
use std::sync::Arc;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use crate::{
    bookmarks, indexer,
    models::{AppType, SearchResult},
    state::AppState,
    windows_utils,
};

// 触发索引更新
pub async fn trigger_reindex(state: Arc<AppState>) {
    println!("正在建立索引...");
    
    let state_clone = state.clone();
    let app_handle = tokio::spawn(async move {
        let exclusion_paths = state_clone.config.lock().unwrap().system_tool_exclusions.clone();
        let apps = indexer::build_index(exclusion_paths).await;
        *state_clone.app_index.lock().unwrap() = apps;
        println!("应用索引完成");
    });

    let state_clone = state.clone();
    let bookmark_handle = tokio::task::spawn_blocking(move || {
        let bookmarks = bookmarks::load_chrome_bookmarks();
        *state_clone.bookmark_index.lock().unwrap() = bookmarks;
        println!("书签索引完成");
    });

    let _ = tokio::join!(app_handle, bookmark_handle);
}

// 搜索逻辑 (去除了 mode 过滤，简化为全搜索)
pub async fn search(query: &str, state: Arc<AppState>) -> Vec<SearchResult> {
    let query_str = query.trim().to_string();
    if query_str.is_empty() {
        return Vec::new();
    }

    let state = state.clone();
    
    tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        let matcher = SkimMatcherV2::default();
        
        // 1. 搜索应用
        let apps = state.app_index.lock().unwrap();
        for app in apps.iter() {
            // 简单的模糊匹配逻辑复用
            let mut best_score = matcher.fuzzy_match(&app.name, &query_str);
            // ... (可以把原 commands.rs 里的 keyword 匹配逻辑搬过来) ...

            if let Some(score) = best_score {
                results.push(SearchResult {
                    id: format!("app::{}", app.id), // 使用前缀区分类型
                    title: app.name.clone(),
                    subtitle: app.path.clone(),
                    icon: String::new(),
                    score,
                    action_id: match app.app_type {
                        AppType::Win32 => "app".to_string(),
                        AppType::Uwp => "uwp".to_string(),
                    },
                });
            }
        }

        // 2. 搜索书签
        let bookmarks = state.bookmark_index.lock().unwrap();
        for bm in bookmarks.iter() {
            if let Some(score) = matcher.fuzzy_match(&bm.title, &query_str) {
                 results.push(SearchResult {
                    id: format!("bm::{}", bm.url), // 将 URL 存入 ID 方便执行
                    title: bm.title.clone(),
                    subtitle: bm.url.clone(),
                    icon: String::new(),
                    score,
                    action_id: "bookmark".to_string(),
                });
            }
        }

        // 排序
        results.sort_by(|a, b| b.score.cmp(&a.score));
        results.truncate(20); // 限制返回 20 条
        results
    })
    .await
    .unwrap_or_default()
}

// 执行逻辑
pub fn execute(item: &SearchResult) {
    let parts: Vec<&str> = item.id.split("::").collect();
    if parts.len() < 2 { return; }
    
    let _type = parts[0];
    let _val = parts[1..].join("::"); // 防止 URL 中也有 ::

    match _type {
        "app" => {
            // 简单起见，这里只处理 Win32，复用 windows_utils 里的逻辑或者用 open
            println!("启动应用: {}", item.subtitle);
            let _ = open::that(&item.subtitle);
        }
        "bm" => {
             println!("打开链接: {}", _val);
             let _ = open::that(_val);
        }
        _ => {}
    }
}

```

---

### 5. 新的入口 `main.rs`

实现一个简单的 REPL (Read-Eval-Print Loop)。

**覆盖 `src-tauri/src/main.rs`：**

```rust
mod bookmarks;
mod config;
mod core;
mod indexer;
mod models;
mod state;
mod text_utils;
mod windows_utils; // 保留它，因为 indexer 依赖它

use std::io::{self, Write};
use std::sync::Arc;
use state::AppState;

#[tokio::main]
async fn main() {
    // 初始化日志
    env_logger::init();

    println!("EGG-CORE CLI v0.1.0");
    println!("-------------------");

    // 初始化状态
    let state = Arc::new(AppState::new());

    // 触发索引
    core::trigger_reindex(state.clone()).await;

    println!("\n请输入搜索关键词 (输入 'exit' 退出):");

    let stdin = io::stdin();
    let mut input = String::new();

    loop {
        print!("> ");
        io::stdout().flush().unwrap();
        input.clear();

        if stdin.read_line(&mut input).is_err() {
            break;
        }

        let query = input.trim();
        if query == "exit" {
            break;
        }

        if query.is_empty() {
            continue;
        }

        // 执行搜索
        let results = core::search(query, state.clone()).await;

        if results.is_empty() {
            println!("  没有找到结果");
        } else {
            for (i, item) in results.iter().enumerate() {
                println!("  [{}] {} ({})", i, item.title, item.action_id);
            }

            // 简单的交互：允许用户输入序号执行
            print!("  输入序号执行 (直接回车跳过): ");
            io::stdout().flush().unwrap();
            let mut choice = String::new();
            stdin.read_line(&mut choice).unwrap();
            
            if let Ok(idx) = choice.trim().parse::<usize>() {
                if let Some(item) = results.get(idx) {
                    core::execute(item);
                }
            }
        }
    }
}

```

---

### 6. 清理其他文件

* 删除 `src-tauri/src/lib.rs` (这是 Tauri lib 入口，不再需要)。
* 删除 `src-tauri/src/hotkey.rs`, `src-tauri/src/hotkey_capture.rs` (CLI 不需要全局快捷键)。
* 保留 `bookmarks.rs`, `indexer.rs`, `models.rs`, `text_utils.rs`, `windows_utils.rs` (但需要检查其中是否有引用 `tauri::AppHandle` 的地方，若有则需移除或替换)。

### 总结

这样你就得到了一个纯净的、基于 Rust 终端的启动器核心。它保留了你最宝贵的：

1. **Windows UWP/Win32 扫描逻辑**
2. **Chrome 书签解析逻辑**
3. **高性能模糊匹配逻辑**

你现在可以通过 `cargo run` 直接在终端里测试搜索效果，调试匹配算法，而无需启动笨重的 Webview 前端。这也是后续开发新 UI (比如迁移到 Slint, Iced 或者只是做一个系统服务) 的坚实基础。
