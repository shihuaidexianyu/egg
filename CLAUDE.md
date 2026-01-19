# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**egg-cli** is a lightweight Windows command-line launcher inspired by Flow Launcher, built with **Rust**. It provides a fast, terminal-based search interface for applications, Chrome bookmarks, and web search.

## Development Commands

### Primary Development
- `cargo run` - Run the CLI in development mode
- `cargo build` - Build the project
- `cargo build --release` - Build optimized release binary
- `cargo test` - Run tests

### Code Quality
- `cargo clippy` - Lint the code
- `cargo fmt` - Format code

## Architecture

### Project Structure

```
egg/
├── src/
│   ├── main.rs           # CLI entry point with REPL loop
│   ├── config.rs         # Configuration persistence
│   ├── search_core.rs    # Pure search logic (no platform code)
│   ├── execute.rs        # Action execution (launch apps, open URLs)
│   ├── indexer.rs        # Win32/UWP application indexing
│   ├── bookmarks.rs      # Chrome bookmark parsing
│   ├── state.rs          # Application state
│   ├── models.rs         # Data structures
│   ├── text_utils.rs     # Text processing (pinyin conversion)
│   └── windows_utils.rs  # Windows-specific utilities
├── Cargo.toml            # Rust dependencies
└── target/release/egg-cli.exe  # Compiled binary
```

### Core Modules

**main.rs** - CLI REPL Interface:
- Entry point with `#[tokio::main]`
- REPL loop reading stdin commands
- Result display and execution by index
- Commands: `help`, `quit`, `reindex`, `clear`, `!N`

**search_core.rs** - Search Logic:
- Pure business logic extracted from original Tauri commands
- Fuzzy matching using `fuzzy-matcher` crate
- Searches apps, bookmarks, and generates web search results
- Returns `(Vec<SearchResult>, HashMap<String, PendingAction>)`

**execute.rs** - Action Execution:
- `execute_action(&PendingAction, bool)` - Main entry point
- Win32 apps via `ShellExecuteW`
- UWP apps via `IApplicationActivationManager`
- URLs via `open` crate

**indexer.rs** - Application Indexing:
- `build_index(Vec<String>)` - Main entry point
- Scans Start Menu shortcuts (.lnk files)
- Enumerates Win32 apps from registry
- Lists UWP packages via `PackageManager`
- Icon extraction with SHA1 caching

**bookmarks.rs** - Chrome Integration:
- `load_chrome_bookmarks()` - Main entry point
- Parses all Chrome user profiles
- Extracts title, URL, folder path
- Generates pinyin keywords for Chinese

**state.rs** - State Management:
- `AppState` struct with thread-safe `Arc<Mutex<T>>`
- `PendingAction` enum for execution
- No GUI-specific fields (removed `hotkey_capture_suspended`, etc.)

**models.rs** - Data Structures:
- `ApplicationInfo` - App metadata with keywords, icons
- `BookmarkEntry` - Bookmark with folder path
- `SearchResult` - Display result with score
- `AppType` - Win32 vs UWP distinction

### Configuration

Configuration is stored in JSON format at:
- **Windows**: `%APPDATA%\egg-cli\settings.json`

Key settings:
- `system_tool_exclusions` - Paths to exclude from indexing
- `max_results` - Maximum search results (default: 40)
- `enable_app_results` / `enable_bookmark_results` - Toggle sources
- `prefix_*` - Mode prefixes (R/B/S for app/bookmark/search)

### REPL Flow

1. **Indexing on startup**:
   - Build app index via `indexer::build_index()`
   - Load bookmarks via `bookmarks::load_chrome_bookmarks()`
   - Store in `AppState` Arc<Mutex<Vec>>

2. **Query processing**:
   - Read stdin, trim whitespace
   - Check for commands (`help`, `quit`, `reindex`, `clear`)
   - Check for execution (`!N` syntax)
   - Otherwise treat as search query

3. **Search**:
   - Call `search_core::search()` with indexes
   - Returns results + pending actions
   - Store in `current_results` for execution

4. **Execution**:
   - User enters `!N`
   - Look up action in `current_results[N-1]`
   - Call `execute::execute_action()`
   - Launch and display success/error

## Platform Specificity

This is a **Windows-only** CLI tool with:
- Win32 API integration via `windows` crate
- UWP application support
- Chrome bookmark parsing (Windows paths)
- No cross-platform support planned

## Key Dependencies

**Core Runtime:**
- `tokio` - Async runtime (full features)
- `anyhow` - Error handling
- `env_logger` - Logging

**Search & Indexing:**
- `fuzzy-matcher` - Fuzzy search (SkimMatcherV2)
- `pinyin` - Chinese character conversion
- `serde` / `serde_json` - Configuration

**Windows APIs:**
- `windows` crate - Win32/UWP APIs
- `winreg` - Registry access
- `open` - URL launching

**Utilities:**
- `dirs` - Config directory resolution
- `base64` - Icon encoding
- `sha1` - Icon cache hashing
- `urlencoding` - Web search URLs

## Common Operations

### Adding a New Search Source

1. Add indexing function to appropriate module (e.g., `bookmarks.rs`)
2. Call in `main.rs` during startup
3. Store in `AppState` (add new field if needed)
4. Add matching logic to `search_core.rs`
5. Add `PendingAction` variant if needed
6. Add execution logic to `execute.rs`

### Modifying Search Behavior

- **Fuzzy matching parameters**: Edit `search_core.rs` matcher initialization
- **Result limits**: Change `MAX_RESULT_LIMIT` constant
- **Scoring algorithm**: Modify `match_application` / `match_bookmark` functions

### Debugging Indexing Issues

```bash
# Set RUST_LOG environment variable
$env:RUST_LOG="debug"
cargo run

# Or for indexer-specific logs
$env:RUST_LOG="egg_cli=debug"
cargo run
```

## Common Issues

**"应用不存在或已被移动"** - App path doesn't exist, may need reindex

**Bookmark loading fails** - Chrome must be closed or profile path is wrong

**UWP apps don't launch** - Check `ApplicationActivationManager` COM initialization

**Chinese search fails** - Pinyin conversion may need `pinyin` crate features

## Build Artifacts

**Development binary**: `target/debug/egg-cli.exe`
**Release binary**: `target/release/egg-cli.exe`

Use release binary for production (much faster due to optimizations).
