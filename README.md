# RustLauncher 启动器

基于 **Tauri 2 + React + TypeScript** 的轻量级启动器，目标是提供类似 Flow Launcher 的体验：

- `Alt+Space` 一键唤出/隐藏窗口
- 在同一个输入框中模糊搜索 **应用 / Chrome 书签 / 网络搜索**
- 支持 `r / b / s` 一类前缀切换不同搜索模式
- 自带设置页面，可以修改全局快捷键、搜索延迟、最大结果数和模式前缀

> 当前实现主要针对 Windows 平台，应用索引采用 Win32/UWP 扫描，书签索引来自本机 Chrome。

---

## 功能概览

- **应用搜索**：
	- 自动索引 Win32 / UWP 应用
	- 支持名称、拼音/首字母以及自定义关键字的模糊匹配
- **书签搜索**：
	- 从 Chrome 收藏夹构建索引
	- 支持按标题、文件夹路径或 URL 搜索
- **网络搜索**：
	- 直接输入内容回车，会在结果中附加一条“在 Google 上搜索”的候选
	- 当输入本身类似 URL 时，会优先出现“打开网址”项
- **模式前缀**：
	- 默认前缀：
		- `r`：应用模式（只在本机应用中搜索）
		- `b`：书签模式（只在书签中搜索）
		- `s`：搜索模式（只追加网络搜索）
	- 用法示例：
		- `r 记事本`：仅在应用中搜索“记事本”
		- `b 前端`：仅在书签中搜索“前端”
		- `s rust tauri`：只触发网络搜索候选
- **结果列表**：
	- 上下方向键切换选中项
	- 回车执行选中项
	- 每条结果显示来源标签：应用 / 书签 / 网址 / 搜索
- **窗口行为**：
	- 全局快捷键（默认 `Alt+Space`）唤起主窗口
	- 回车执行后自动隐藏窗口，并清空搜索状态
	- `Esc` 也会通过统一事件重置并隐藏窗口
- **设置窗口**：
	- 内置 Tauri 子窗口，使用 React 编写
	- 支持修改：
		- 全局快捷键
		- 搜索防抖延迟（ms）
		- 最大结果数量
		- 是否启用“应用结果”和“书签结果”
		- 三种模式的前缀字母

---

## 技术栈

- **前端**：React + TypeScript + Vite
- **桌面壳**：Tauri 2
- **后端逻辑（Rust）**：
	- 应用索引 & 启动（Win32 / UWP）
	- Chrome 书签加载与模糊匹配
	- 全局快捷键绑定
	- 配置读写 (`settings.json`)

主要入口：

- `src/App.tsx`：根据 URL 参数切换主窗口 / 设置窗口
- `src/components/LauncherWindow.tsx`：主搜索窗口
- `src/components/SettingsWindow.tsx`：设置窗口
- `src-tauri/src/commands.rs`：Tauri 后端命令（查询、执行、索引、配置）
- `src-tauri/src/config.rs`：应用配置结构及持久化逻辑

---

## 环境准备

### 必备工具

- [Node.js 18+](https://nodejs.org/)（推荐启用 Corepack）
- [Rust toolchain](https://www.rust-lang.org/tools/install)（稳定版即可）
- Windows 平台需要满足 [Tauri 系统要求](https://tauri.app/v1/guides/getting-started/prerequisites/)

### 启用 pnpm（推荐）

```bash
corepack enable
corepack prepare pnpm@9.12.0 --activate
```

如果 Corepack 无法修改全局 Node，请手动安装 pnpm：

```bash
npm install -g pnpm@9.12.0
```

---

## 本地开发

安装依赖：

```bash
pnpm install
```

仅启动前端（浏览器中访问）：

```bash
pnpm run dev
```

启动 Tauri 桌面应用（推荐开发方式）：

```bash
pnpm run tauri dev
```

启动后：

- 使用默认快捷键 `Alt+Space` 唤出 RustLauncher 主窗口
- 输入内容即可搜索
- `Ctrl+,` 可以从主窗口打开设置窗口

---

## 常用脚本

```bash
pnpm run build        # TypeScript 类型检查 + 生产构建
pnpm run preview      # 启动本地预览服务器
pnpm run format       # 使用 Prettier 格式化代码
pnpm run lint         # 仅做类型检查
pnpm run tauri build  # 构建桌面安装包
```

---

## 打包桌面应用

1. **安装打包依赖（仅需一次）**：
	 - Rust 工具链，并添加 MSVC 目标：`rustup target add x86_64-pc-windows-msvc`
	 - 安装 WebView2 运行时和 Tauri 文档中要求的 Windows 构建工具

2. **构建前端与桌面二进制**：

	 ```bash
	 pnpm install
	 pnpm run build          # 可选：提前暴露前端问题
	 pnpm run tauri build    # 在 src-tauri/target/release/ 下生成 exe 和安装包
	 ```

3. **分发产物**：
	 - `src-tauri/target/release/tauri-app.exe`：可直接运行的便携版程序
	 - `src-tauri/target/release/*.msi` 或 `*.nsis.exe`：给用户安装的安装包

需要调试版时，可以在构建命令后追加 `--debug`。如需自定义图标、签名或自动更新等，请编辑 `src-tauri/tauri.conf.json` 中的 `bundle` 配置。

---

## 配置与数据存储

应用的所有持久化设置都保存在 `settings.json` 中，路径由 Tauri 的 `app_config_dir` 决定，基于本项目的标识符 `com.rustlauncher.app`：

- **Windows**：`%APPDATA%/com.rustlauncher.app/settings.json`
	- 示例：`C:\Users\<你>\AppData\Roaming\com.rustlauncher.app\settings.json`
- **macOS**：`~/Library/Application Support/com.rustlauncher.app/settings.json`
- **Linux**：`$XDG_CONFIG_HOME/com.rustlauncher.app/settings.json`
	- 若未设置 `XDG_CONFIG_HOME`，则为 `~/.config/com.rustlauncher.app/settings.json`

`settings.json` 中主要字段包括：

- `global_hotkey`：全局唤起快捷键（如 `Alt+Space`）
- `query_delay_ms`：搜索防抖延迟（毫秒）
- `max_results`：结果数量上限
- `enable_app_results` / `enable_bookmark_results`：是否启用应用/书签结果
- `prefix_app` / `prefix_bookmark` / `prefix_search`：三种模式的前缀字母

> 建议优先通过应用内的“设置”页面修改上述配置。手动编辑 `settings.json` 时，请在关闭应用后进行，并在修改完成后重新启动 RustLauncher 以生效。

---

## 已知限制 / 后续计划

- 当前主要在 Windows 上开发与测试，其他平台支持尚未完善
- 书签索引仅支持 Chrome，如需支持 Edge/Firefox，可在后续版本扩展
- 搜索结果固定追加 Google 搜索，如需自定义搜索引擎可以在后端增加配置项

欢迎基于本项目进行二次开发或提交 PR，一起打磨更好用的 Rust 桌面启动器。
