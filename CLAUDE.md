# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Run

```bash
# Development build
cargo build

# Release build (optimized binary)
cargo build --release

# Run the application
cargo run

# Run with debug logging
RUST_LOG=debug cargo run
```

### Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

## Architecture Overview

egg-cli is a Windows-only command-line launcher built with Rust. It uses a TUI (Terminal User Interface) built with ratatui/crossterm for fuzzy searching and launching applications, bookmarks, and web searches.

### Core Architecture Pattern

The application follows a **state-concurrency pattern** with centralized shared state:

- **AppState** (`state.rs`): Centralized shared state wrapped in `Arc<Mutex<T>>` for thread-safe access across async tasks
- **TuiState** (`main.rs`): Local UI state for the terminal interface, not shared
- **Search operations** are cached in a LRU cache (`SearchCache`) to avoid re-computing fuzzy matches
- **Recent actions** are tracked in an LRU list (`RecentList`) to show frequently used items when query is empty

### Key Data Flow

1. **Startup**: Load config → Build app index (async) → Load bookmarks → Spawn background refresh task
2. **Input**: User types in TUI → On every keystroke, `refresh_results()` checks cache or performs fuzzy search
3. **Search**: `search_core::search()` performs fuzzy matching across apps/bookmarks using `fuzzy-matcher` crate
4. **Selection**: User selects result → `PendingAction` is created → TUI exits → `execute::execute_action()` launches the item
5. **Post-launch**: Action is added to `recent_actions` list

### Module Responsibilities

**main.rs** (523 lines)

- Entry point, tokio runtime setup
- TUI rendering and event loop (ratatui)
- Keystroke handling (Ctrl+C/N/P/W, Enter, Esc, arrow keys, Home/End, Backspace/Delete)
- Search result caching orchestration
- Spawns background index refresh task after 2 seconds

**search_core.rs** (291 lines)

- Pure business logic for fuzzy search - NO platform-specific code
- `search()` function returns `(Vec<SearchResult>, HashMap<String, PendingAction>)`
- Query modes: All, Bookmark, Application, Search
- URL detection for direct navigation
- Pinyin support for Chinese character matching

**indexer.rs** (543 lines)

- Application discovery: Start Menu shortcuts (.lnk), Win32 registry enumeration, UWP packages
- De-duplication by `(AppType, path, arguments)`
- Icon extraction with caching (via `windows_utils.rs`)
- Runs in blocking tasks spawned from async context

**execute.rs** (212 lines)

- Launches Win32 apps via `ShellExecuteW`
- Activates UWP apps via `IApplicationActivationManager` (in dedicated thread with COM initialized)
- Opens URLs via `open` crate
- Uses `UwpLauncher` pattern with dedicated thread to handle COM requirements

**bookmarks.rs** (309 lines)

- Loads Chrome/Edge bookmarks from `Bookmarks` JSON file
- Supports multiple browser profiles via registry detection
- Recursively parses bookmark tree structure

**state.rs** (146 lines)

- `AppState`: Shared state container with Arc<Mutex<T>> wrappers
- `RecentList`: LRU cache for recently launched items (capacity: 12)
- `SearchCache`: LRU cache for search results (capacity: 8)
- `PendingAction` enum: Application, Bookmark, Url, Search

**models.rs** (32 lines)

- `ApplicationInfo`: Core app metadata (id, name, path, app_type, keywords, pinyin_index)
- `SearchResult`: UI result representation (id, title, subtitle, score, action_id)
- `AppType`: Win32 or Uwp

**config.rs** (137 lines)

- `AppConfig`: Configuration loaded from `%APPDATA%\egg-cli\settings.json`
- Defaults: query_delay_ms=120, max_results=40, enables apps and bookmarks
- `system_tool_exclusions`: Paths to exclude from indexing

**cache.rs** (38 lines)

- Persists application index to `%LOCALAPPDATA%\egg\cache\index.json`
- Loads cached index on startup to speed up launch time
- Cache is invalidated when new apps are detected

**text_utils.rs** (43 lines)

- `build_pinyin_index()`: Converts Chinese text to pinyin for fuzzy matching
- Enables searching Chinese apps with English pinyin input

**windows_utils.rs** (399 lines)

- Windows API helpers: shell link resolution, .url file parsing
- `ComGuard`: RAII wrapper for COM initialization/cleanup
- Environment variable expansion
- User SID retrieval for security contexts

### Threading Model

- Main thread: Tokio async runtime + TUI event loop
- Background task: Index refresh after 2 seconds (async)
- Blocking tasks: Start menu enumeration, Win32 registry scanning (spawn_blocking)
- UWP launcher thread: Dedicated thread with COM apartment for UWP activation
- All shared state protected by `Arc<Mutex<T>>`

### Windows APIs Used

- **ShellExecuteW**: Launch Win32 applications
- **IApplicationActivationManager**: Activate UWP apps
- **Registry** (winreg): Enumerate installed software from `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
- **PackageManager**: List UWP packages
- **COM**: Required for UWP activation (STA thread)

### Configuration File Location

- Path: `%APPDATA%\egg-cli\settings.json`
- Runtime: Config loaded once at startup, stored in `AppState.config`
- Changes require restart to take effect

### Cache Locations

- App index: `%LOCALAPPDATA%\egg\cache\index.json`
- In-memory search cache: `AppState.search_cache` (8 entries, LRU)
- In-memory recent actions: `AppState.recent_actions` (12 entries, LRU)

## Implementation Notes

- **Pinyin support**: Chinese apps are indexed with both original names and pinyin equivalents for flexible search
- **De-duplication**: Apps are de-duplicated by `(AppType, normalized_path, arguments)` to prevent registry/Start Menu duplicates
- **Async/sync boundary**: Heavy IO (indexing) runs in `spawn_blocking`, UI stays responsive
- **Error handling**: Most errors are logged and ignored (e.g., failing to parse a single shortcut), don't fail the whole operation
- **TUI limitations**: No Unicode grapheme cluster handling, cursor movement works on char boundaries (may break with combining characters)

## Known Limitations

- Windows-only (uses Win32/UWP APIs exclusively)
- Chrome/Edge bookmarks only (Firefox/Safari support not implemented)
- No tests directory (consider adding integration tests for core search logic)
- Chinese pinyin matching only supports simple pinyin, not tones or variants
