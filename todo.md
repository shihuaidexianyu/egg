### 总体架构 (Overall Architecture)

我们将采用一个标准的 Tauri 应用架构：

1.  **Rust 后端 (Core)**:
      * 负责处理**全局热键**（例如 `Alt+Space`）来显示/隐藏窗口。
      * 负责**应用索引**：在后台启动时异步扫描 Windows 系统，建立所有 `.exe`, `.lnk` 和 UWP (商店) 应用的索引，并缓存在内存中。
      * 负责**查询处理**：接收前端的输入，通过模糊搜索（Fuzzy Search）匹配缓存的应用索引和网页 URL。
      * 负责**命令执行**：根据前端的选择，执行启动应用、打开 URL 或进行网页搜索的操作。
2.  **Web 前端 (UI)**:
      * 使用现代 Web 技术（推荐使用 Svelte, React 或 Vue）构建。
      * 只包含一个**输入框**（Search Bar）和一个**结果列表**（Results List）。
      * 负责将用户输入实时发送到 Rust 后端。
      * 负责渲染 Rust 返回的 `SearchResult` 列表。
      * 处理上下键选择和回车键执行。

-----

### Rust 后端设计 (`src-tauri/src`)

这是项目的核心，我们将在这里处理所有与 Windows 系统的交互。

#### 1\. 项目配置 (`tauri.conf.json`)

这是第一步，配置窗口的外观和行为。

```json
{
  "windows": [
    {
      "label": "main",
      "fullscreen": false,
      "height": 450, // 窗口高度
      "width": 700,  // 窗口宽度
      "title": "RustLauncher",
      "alwaysOnTop": true,   // 始终在最前
      "decorations": false,  // 隐藏窗口边框
      "transparent": true,   // 开启透明
      "visible": false,      // 启动时隐藏
      "skipTaskbar": true,   // 不在任务栏显示
      "center": true         // 窗口居中
    }
  ],
  // ... 其他配置
}
```

#### 2\. 核心数据结构 (Structs)

我们需要定义 Rust 和 前端 之间通信的数据。

```rust
// src-tauri/src/main.rs (或单独的 state.rs)

use serde::{Serialize, Deserialize};
use std::sync::Mutex;

// 用于缓存在内存中的应用信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppType {
    Win32,
    Uwp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationInfo {
    pub name: String,
    pub path: String,       // Win32 的可执行路径, 或 UWP 的 AppUserModelId
    pub app_type: AppType,
    pub icon_b64: String, // 应用图标的 Base64 编码字符串
}

// Rust 后端持有的全局状态
pub struct AppState {
    // 使用 Mutex 保证多线程安全访问
    pub app_index: Mutex<Vec<ApplicationInfo>>,
}

// 发送给前端的搜索结果
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String, // 唯一ID，用于执行
    pub title: String,
    pub subtitle: String,
    pub icon: String,   // 图标 (Base64 或 标识符)
    pub score: i64,     // 匹配得分，用于排序
    
    // 执行所需的数据
    pub action_id: String, // "app", "uwp", "url", "search"
    pub action_payload: String, // 路径, AppId, 或 URL
}
```

#### 3\. Tauri 命令 (Commands)

这些是前端可以调用的 Rust 函数。

```rust
// src-tauri/src/main.rs

// 1. 提交查询
#[tauri::command]
fn submit_query(query: String, state: tauri::State<AppState>) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // 步骤 1: 检查是否为 URL
    if query.contains('.') || query.starts_with("http") {
        results.push(SearchResult {
            // ... (构造一个 "打开 URL" 的结果)
            action_id: "url".to_string(),
            action_payload: query.clone(),
            score: 100, // 优先
            // ...
        });
    }

    // 步骤 2: 搜索应用
    let app_index = state.app_index.lock().unwrap();
    
    // 使用 fuzzy-matcher crate 进行模糊匹配
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
    
    for app in app_index.iter() {
        if let Some(score) = matcher.fuzzy_match(&app.name, &query) {
            results.push(SearchResult {
                // ... (根据 ApplicationInfo 构造一个 "应用" 结果)
                title: app.name.clone(),
                subtitle: app.path.clone(),
                icon: app.icon_b64.clone(),
                score: score,
                action_id: match app.app_type {
                    AppType::Win32 => "app".to_string(),
                    AppType::Uwp => "uwp".to_string(),
                },
                action_payload: app.path.clone(),
                // ...
            });
        }
    }
    
    // 步骤 3: 添加默认网页搜索
    results.push(SearchResult {
        // ... (构造一个 "网页搜索" 的结果)
        title: format!("在 Google 上搜索: {}", query),
        action_id: "search".to_string(),
        action_payload: format!("https://google.com/search?q={}", urlencoding::encode(&query)),
        score: 0, // 最低优先级
        // ...
    });

    // 步骤 4: 排序和截断
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(8); // 只返回前 8 个结果
    results
}

// 2. 执行操作
#[tauri::command]
async fn execute_action(id: String, payload: String, app_handle: tauri::AppHandle) {
    match id.as_str() {
        "app" => {
            // 启动 Win32 应用
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &payload])
                .spawn()
                .ok();
        },
        "uwp" => {
            // 启动 UWP 应用 (这需要 windows-rs 的 COM 调用)
            // 示例: 使用 IApplicationActivationManager::ActivateApplication
            // ... (这部分比较复杂，需要 Windows-rs 的 COM 知识)
        },
        "url" | "search" => {
            // 打开 URL 或 搜索
            tauri::api::shell::open(&app_handle.shell_scope(), payload, None).ok();
        },
        _ => {}
    }
    
    // 执行后隐藏窗口
    if let Some(window) = app_handle.get_window("main") {
        window.hide().ok();
    }
}

// 3. 触发后台索引 (在应用启动时调用)
#[tauri::command]
async fn trigger_reindex(state: tauri::State<'_, AppState>) {
    // 异步执行，不阻塞主线程
    tauri::async_runtime::spawn(async move {
        let apps = indexer::build_index().await;
        let mut app_index = state.app_index.lock().unwrap();
        *app_index = apps;
        println!("应用索引完成!");
    });
}
```

#### 4\. 应用索引器 (The Hard Part: `src-tauri/src/indexer.rs`)

这是最复杂但也是最重要的部分。你需要 `windows-rs` 这个 crate。

```rust
// src-tauri/src/indexer.rs (伪代码和思路)

use crate::ApplicationInfo;
use windows::core::{PCWSTR, HSTRING};
use windows::Win32::System::Com::{CoInitialize, CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, ExtractIconExW, SHGetFileInfoW, ...};
use windows::Win32::Storage::EnhancedStorage::{PKEY_AppUserModel_ID};
use windows::Management::Deployment::PackageManager; // WinRT API

// 主索引函数
pub async fn build_index() -> Vec<ApplicationInfo> {
    let mut apps = Vec::new();
    
    // 1. 索引 Win32 应用 (.lnk)
    //    - 找到开始菜单路径 (CSIDL_STARTMENU, CSIDL_COMMON_STARTMENU)
    //    - 使用 `walkdir` 遍历
    //    - 对每个 .lnk 文件:
    //        - 使用 COM (IShellLink) 解析
    //        - 获取目标路径 (target_path)
    //        - 获取应用名称
    //        - 获取图标位置 (icon_location)
    //        - 提取图标 (ExtractIconExW / SHGetFileInfoW)
    //        - 将 HICON 转为 Base64 (需要一个辅助函数 `hicon_to_base64`)
    //        - 存入 `apps` 列表

    // 2. 索引 UWP (商店) 应用
    //    - 初始化 WinRT
    //    - `let manager = PackageManager::new().unwrap();`
    //    - `let packages = manager.FindPackagesForUser(&HSTRING::from("")).unwrap();`
    //    - 遍历 `packages`:
    //        - `let entries = package.GetAppListEntriesAsync().await.unwrap();`
    //        - 遍历 `entries`:
    //            - `name = entry.DisplayInfo().DisplayName().to_string()`
    //            - `app_id = entry.AppUserModelId().to_string()`
    //            - `logo_stream = entry.DisplayInfo().GetLogo(...).OpenReadAsync().await`
    //            - 从 `logo_stream` 读取数据, 转为 Base64
    //            - 存入 `apps` 列表 (type: Uwp, path: app_id)

    apps
}
```

#### 5\. 全局热键 (`src-tauri/src/main.rs`)

在 `main` 函数的 `.setup()`钩子中设置。

```rust
// src-tauri/src/main.rs
use tauri::{GlobalShortcutManager, Manager};

fn main() {
    tauri::Builder::default()
        .manage(AppState { app_index: Mutex::new(vec![]) }) // 注入状态
        .invoke_handler(tauri::generate_handler![
            submit_query, 
            execute_action, 
            trigger_reindex
        ])
        .setup(|app| {
            let handle = app.handle();
            let mut shortcuts = handle.global_shortcut_manager();
            
            // 注册 Alt+Space
            shortcuts.register("Alt+Space", move || {
                let window = handle.get_window("main").unwrap();
                if window.is_visible().unwrap_or(false) {
                    window.hide().ok();
                } else {
                    window.show().ok();
                    window.set_focus().ok();
                }
            })?;
            
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
```

-----

### Web 前端设计 (`src/`)

这里我们使用 React (Svelte/Vue 思路类似) 来快速构建 UI。

#### 1\. 核心依赖 (JS)

```bash
npm install @tauri-apps/api
```

#### 2\. 核心组件 (`src/App.jsx`)

```jsx
import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { appWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import './App.css'; // 用于设置样式

function App() {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState([]);
  const [selectedIndex, setSelectedIndex] = useState(0);

  // 1. 在启动时触发索引
  useEffect(() => {
    invoke('trigger_reindex');
  }, []);

  // 2. 监听 Escape 键和来自后端的隐藏事件
  useEffect(() => {
    const handleEsc = (e) => {
      if (e.key === 'Escape') {
        appWindow.hide();
      }
    };
    window.addEventListener('keydown', handleEsc);
    
    // 监听 Rust 执行后的隐藏事件
    const unlisten = listen('hide_window', () => {
        setQuery('');
        setResults([]);
        appWindow.hide();
    });

    return () => {
      window.removeEventListener('keydown', handleEsc);
      unlisten.then(f => f());
    };
  }, []);

  // 3. 处理查询变化
  const handleQueryChange = async (e) => {
    const newQuery = e.target.value;
    setQuery(newQuery);
    if (newQuery.trim() === '') {
      setResults([]);
    } else {
      const newResults = await invoke('submit_query', { query: newQuery });
      setResults(newResults);
      setSelectedIndex(0);
    }
  };

  // 4. 处理键盘导航
  const handleKeyDown = (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.min(prev + 1, results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) => Math.max(prev - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const selected = results[selectedIndex];
      if (selected) {
        invoke('execute_action', { 
            id: selected.action_id, 
            payload: selected.action_payload 
        });
        // Rust 后端会通过 execute_action 发送事件或直接隐藏窗口
        // 我们在这里清理前端状态
        setQuery('');
        setResults([]);
      }
    }
  };

  return (
    <div className="container" data-tauri-drag-region>
      <input
        type="text"
        className="search-bar"
        value={query}
        onChange={handleQueryChange}
        onKeyDown={handleKeyDown}
        placeholder="搜索应用和网页..."
        autoFocus
      />
      <ul className="results-list">
        {results.map((item, index) => (
          <li
            key={item.id}
            className={index === selectedIndex ? 'result-item selected' : 'result-item'}
            // 允许鼠标悬停时更新索引
            onMouseEnter={() => setSelectedIndex(index)}
          >
            <img 
              src={`data:image/png;base64,${item.icon}`} 
              className="result-icon"
              alt=""
            />
            <div className="result-text">
              <div className="result-title">{item.title}</div>
              <div className="result-subtitle">{item.subtitle}</div>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

export default App;
```

#### 3\. 核心样式 (`src/App.css`)

这是实现原生观感的关键。

```css
/* 确保窗口透明背景生效 */
html, body, #root {
  background: transparent;
  margin: 0;
  padding: 0;
}

.container {
  /* 使用 backdrop-filter 实现 Windows 的亚克力/Mica效果 */
  background-color: rgba(30, 30, 30, 0.7); /* 暗色背景 */
  backdrop-filter: blur(20px) saturate(180%);
  border-radius: 10px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3);
  overflow: hidden;
  margin: 10px; /* 为阴影留出空间 */
  height: calc(100vh - 20px); /* 撑满窗口，减去 margin */
  display: flex;
  flex-direction: column;
}

.search-bar {
  width: 100%;
  box-sizing: border-box;
  padding: 16px 20px;
  font-size: 24px;
  background: transparent;
  border: none;
  color: white;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  outline: none;
}

.results-list {
  list-style: none;
  margin: 0;
  padding: 0 10px 10px 10px;
  overflow-y: auto;
}

.result-item {
  display: flex;
  align-items: center;
  padding: 10px;
  border-radius: 6px;
  margin-top: 5px;
}

/* 选中的条目 */
.result-item.selected {
  background-color: rgba(80, 120, 255, 0.6);
}

.result-icon {
  width: 32px;
  height: 32px;
  margin-right: 12px;
}

.result-text {
  overflow: hidden;
}

.result-title {
  font-size: 16px;
  color: white;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.result-subtitle {
  font-size: 13px;
  color: rgba(255, 255, 255, 0.7);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
```

### 关键依赖总结

  * **Rust Crates**:
      * `tauri` (核心)
      * `windows-rs` (用于应用索引和执行 UWP)
      * `fuzzy-matcher` (用于模糊搜索)
      * `walkdir` (用于遍历开始菜单)
      * `lnk` (用于解析 .lnk 快捷方式)
      * `base64` (用于编码图标)
      * `urlencoding` (用于网页搜索)
  * **JS Libs**:
      * `react` (或 `svelte`, `vue`)
      * `@tauri-apps/api`

这个规划为你提供了一个不含插件系统、专注于 Windows 应用和网页启动的完整蓝图。最大的挑战在于 `indexer.rs` 中使用 `windows-rs` 与 Windows API 进行复杂的交互（特别是UWP和图标提取），但这绝对是可以实现的。

-----

### 图标与资源处理

1.  **Win32 图标提取**：
  * 使用 `ExtractIconExW` 尝试直接提取 `.lnk` 目标可执行文件的主图标。
  * 如果失败，降级为 `SHGetFileInfoW` 以避免空图标，并在日志中记录缺失项。
  * 将 `HICON` 转换为 `Image` 后再编码为 PNG/Base64，避免直接序列化原始位图数据。
2.  **UWP Logo**：
  * 通过 `AppDisplayInfo::GetLogo` 获取 `IRandomAccessStreamWithContentType`。
  * 转换到 `Vec<u8>` 时要异步读完所有缓冲，保持 UI 主线程不阻塞。
3.  **缓存策略**：
  * 为避免每次启动都进行昂贵的图标转换，可将 Base64 缓存到 `AppData\Local\RustLauncher\icons`，通过 `sha1(name + path)` 命名。
  * 如果缓存命中，直接读取文件；若缺失则重新生成并回写。

-----

### 索引生命周期与刷新策略

1.  **首次启动**：
  * 启动后立即触发 `trigger_reindex`，同时向前端发送 `indexing_started` 事件以显示 Loading 提示。
2.  **增量刷新**：
  * 监听 Windows Shell 的 `SHChangeNotify` 事件（可选），当开始菜单目录发生变动时，延迟 2 秒批量重新索引。
  * 对 UWP 列表每隔 10 分钟重新查询一次，避免占用过多资源。
3.  **手动刷新**：
  * 替换 `trigger_reindex` 为返回 `Result<(), String>` 的命令，在前端 `Ctrl+R` 时调用，并弹出 Toast。

-----

### Rust 模块划分建议

* `state.rs`：定义 `AppState`、`SearchResult`、`ApplicationInfo` 等共享类型。
* `commands.rs`：放置所有 `#[tauri::command]` 函数，保持 `main.rs` 简洁。
* `indexer.rs`：实现 Win32/UWP 索引逻辑及缓存辅助函数。
* `windows_utils.rs`：封装 Windows API 调用（COM 初始化、HICON -> PNG 等）。
* `models.rs`：如需未来扩展可拆分 API 响应/请求结构，保持类型整洁。

-----

### 前端交互细节

1.  **键盘导航**：
  * 支持 `Ctrl+N`/`Ctrl+P` 作为备用的上下导航。
  * `Tab` 在候选项之间循环，`Shift+Tab` 反向循环。
2.  **输入法适配**：
  * 通过 CSS `ime-mode: active;` 改善中文输入体验。
  * 监听 `compositionstart`/`compositionend`，仅在输入完成后触发查询，避免拼音中间态干扰。
3.  **结果渲染优化**：
  * 当结果超过 8 条时启用虚拟滚动或 `react-window`，减少重排。
  * 对 `icon` 字段增加 `loading` 骨架，避免首帧闪烁。
4.  **窗口动画**：
  * 通过 `appWindow.show().then(() => appWindow.setFocus())` 避免首次展示时焦点丢失。
  * CSS 使用 `opacity` + `transform` 过渡，模拟 Windows Spotlight 弹出效果。

-----

### 启动与部署流程

1.  **开发流程**：
  * `npm install` -> `npm run tauri dev`。
  * Rust 端使用 `cargo fmt`, `cargo clippy`，前端使用 `eslint` + `prettier` 保持一致风格。
2.  **打包发布**：
  * `npm run tauri build` 生成安装包；在 `tauri.conf.json` 中配置 `bundle.windows.wix` 选项以支持自定义图标。
  * 使用 GitHub Actions（windows-latest）自动构建 MSI/NSIS，上传到 Release。
3.  **版本更新**：
  * 结合 `tauri::updater` 提供自动更新；后端托管 `latest.json` 和安装包。

-----

### 测试与质量保障

* **Rust 端单元测试**：对 `fuzzy_match`、`icon_cache` 等纯函数编写单元测试，使用 `#[cfg(test)]`。
* **集成测试**：通过 `tauri::test` 或 `specta` + `tauri-specta` 生成 TypeScript 类型并验证命令签名。
* **前端测试**：使用 `vitest` + `react-testing-library` 对搜索交互和组件状态进行测试。
* **端到端测试**：考虑使用 `playwright` 的 Electron/Tauri 支持，验证窗口显示、快捷键、执行动作是否生效。

-----

### 性能与资源监控

* **内存占用**：索引结果保存在内存，目标控制在数千应用时 < 80MB；可以在 `AppState` 引入 `Arc<Vec<_>>` 减少拷贝。
* **启动速度**：主线程在 100ms 内完成初始化，重度操作放入后台任务。
* **日志记录**：使用 `tracing` + `tracing-subscriber` 输出结构化日志；在发布模式下降级为 `info`。
* **崩溃监控**：集成 `sentry` 或 `tauri-plugin-log`，便于收集用户侧崩溃信息。

-----

### 安全与权限

* 请求 `capabilities/default.json` 中的最小权限集合，避免打包时触发 UAC 警告。
* 对前端输入做 URL 校验，防止 `javascript:` 等危险协议被执行。
* 执行 Win32 应用前，判断路径是否存在且位于允许的盘符，后续可引入白名单机制。

-----

### 未来扩展想法

1.  **插件系统**：
  * 定义 `PluginResult`/`PluginAction` 协议，允许通过 HTTP 或本地脚本返回结果。
2.  **多搜索源**：
  * 集成百科、代码片段、常用命令快捷方式等，按类别分组显示。
3.  **同步功能**：
  * 使用 `tauri-plugin-store` 同步搜索历史/收藏到云端。
4.  **UI 主题**：
  * 根据系统主题切换明暗色，支持自定义背景模糊程度。

-----

### 待办清单

- [x] 完成 `windows_utils.rs` 中的图标转换与缓存逻辑
- [x] 实现 `indexer::build_index` 的 Win32/UWP 索引
- [x] 在前端加入查询节流与组合输入支持
- [x] 为 Execute 命令增加错误回传并在 UI 显示 Toast
- [x] 集成 `cargo xtask` 脚本自动化格式化/检查