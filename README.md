# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Prerequisites

- [Node.js 18+](https://nodejs.org/) with [Corepack](https://nodejs.org/api/corepack.html) enabled or a global `pnpm` installation
- [Rust toolchain](https://www.rust-lang.org/tools/install) (nightly not required)
- Tauri system requirements for your target OS

### Enable pnpm via Corepack (recommended)

```
corepack enable
corepack prepare pnpm@9.12.0 --activate
```

If Corepack cannot modify your global Node installation, install pnpm manually instead:

```
npm install -g pnpm@9.12.0
```

## Getting started

```bash
pnpm install
pnpm run dev          # web-only dev server
pnpm run tauri dev    # tauri desktop dev
```

## Additional scripts

```bash
pnpm run build        # type-check + production build
pnpm run preview      # preview production build
pnpm run format       # apply Prettier formatting
pnpm run lint         # type-check only
pnpm run tauri build  # create distributable application
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
