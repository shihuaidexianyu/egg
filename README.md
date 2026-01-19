# egg-cli

A lightweight Windows command-line launcher inspired by Flow Launcher. Built with Rust for fast application indexing, Chrome bookmark searching, and web search.

## Features

- **Application Search**: Fuzzy search for Win32 and UWP applications
- **Bookmark Search**: Search Chrome bookmarks from all profiles
- **Web Search**: Direct Google search integration
- **Pinyin Support**: Chinese character matching with pinyin variants
- **Fast Indexing**: Efficient application and bookmark indexing

## Prerequisites

- Windows 10/11
- [Rust toolchain](https://www.rust-lang.org/tools/install) (stable)

## Installation

### Build from source

```bash
# Clone or navigate to the project directory
cd egg

# Build the release binary
cargo build --release

# The binary will be at:
# target/release/egg-cli.exe
```

### Add to PATH (optional)

To run `egg-cli` from anywhere, add the release directory to your system PATH:

```powershell
# Add to current session
$env:PATH += ";C:\Users\YourName\Desktop\egg\target\release"

# Or add permanently via System Environment Variables
```

## Usage

### Interactive Mode

```bash
egg-cli
```

**Commands:**

- `<query>` - Search for apps, bookmarks, or URLs
- `!1` - Launch the first search result
- `help` - Show available commands
- `reindex` - Rebuild application and bookmark indexes
- `clear` - Clear current search results
- `quit` - Exit egg-cli

**Examples:**

```
> chrome          # Search for Chrome
[1] Google Chrome
[2] Chrome Remote Desktop

> !1              # Launch first result
Launched successfully!

> https://github.com  # Open URL directly
[1] 打开网址: https://github.com

> !1
Launched successfully!
```

### Configuration

Configuration is stored in `%APPDATA%\egg-cli\settings.json`.

Default configuration:

```json
{
  "global_hotkey": "Alt+Space",
  "query_delay_ms": 120,
  "max_results": 40,
  "enable_app_results": true,
  "enable_bookmark_results": true,
  "prefix_app": "R",
  "prefix_bookmark": "B",
  "prefix_search": "S",
  "launch_on_startup": false,
  "force_english_input": true,
  "debug_mode": false,
  "system_tool_exclusions": [
    "c:\\windows\\system32",
    "c:\\windows\\syswow64",
    "c:\\windows\\winsxs"
  ]
}
```

## Development

### Build

```bash
cargo build
```

### Run

```bash
cargo run
```

### Test

```bash
cargo test
```

## Project Structure

```
egg/
├── src/                    # Source code
│   ├── main.rs            # CLI entry point and REPL
│   ├── config.rs          # Configuration management
│   ├── search_core.rs     # Search logic
│   ├── execute.rs         # Action execution
│   ├── indexer.rs         # Application indexing
│   ├── bookmarks.rs       # Chrome bookmark parsing
│   ├── state.rs           # Application state
│   ├── models.rs          # Data structures
│   ├── text_utils.rs      # Text processing (pinyin)
│   └── windows_utils.rs   # Windows-specific utilities
├── Cargo.toml             # Rust dependencies
└── README.md              # This file
```

## Architecture

### Core Modules

**Search Core** (`search_core.rs`):

- Pure business logic for fuzzy matching
- No platform-specific code
- Supports apps, bookmarks, and web search

**Indexer** (`indexer.rs`):

- Scans Start Menu shortcuts
- Enumerates Win32 apps from registry
- Lists UWP applications
- Icon extraction with caching

**Executor** (`execute.rs`):

- Launches Win32 applications via ShellExecute
- Activates UWP apps via ApplicationActivationManager
- Opens URLs in default browser

### Data Flow

1. User enters query → REPL parses input
2. Search core performs fuzzy match on indexes
3. Results displayed with numbered indices
4. User enters `!N` to execute
5. Executor launches selected item

## Technical Details

### Windows APIs Used

- **ShellExecuteW** - Launch Win32 applications
- **IApplicationActivationManager** - Activate UWP apps
- **Registry** - Enumerate installed software
- **PackageManager** - List UWP packages

### Dependencies

- `tokio` - Async runtime
- `serde` / `serde_json` - Serialization
- `fuzzy-matcher` - Fuzzy search algorithm
- `windows` crate - Win32/UWP APIs
- `dirs` - Cross-platform config directories
- `open` - Cross-platform URL opening
- `pinyin` - Chinese character conversion

## Limitations

- Windows-only (uses Win32/UWP APIs)
- Chrome bookmarks only (Edge/Firefox support planned)
- Google search only (custom engines configurable in code)

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.
