# Keepstone

A calm, fast, local-first home for everything you keep: notes, tasks, and
soon passwords, calendar, habits and finances. Native Rust, GPU-rendered with
[GPUI](https://www.gpui.rs) via [GPUI Kit](https://gpui-kit.com). No webview, no ads, no paywall.

## Run

```bash
# Rust is pinned by rust-toolchain.toml; rustup installs it automatically.
cargo run                                 # debug data lives in a separate "Keepstone Dev" folder
KEEPSTONE_DATA_DIR=/tmp/ks cargo run      # throwaway data folder
cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3   # sample data
scripts/check.sh                          # fmt + clippy + all tests
```

macOS needs Xcode (with the Metal toolchain). Windows needs VS 2022 Build
Tools (C++), the MSVC Rust toolchain and CMake.

## Features today

- **Today**: what's due and overdue, done today, recent notes.
- **Notes**: Markdown, `[[wiki links]]` with backlinks, full-text search, pin, trash with undo, autosave.
- **Tasks**: quick add, due dates, priorities, status, Today/Upcoming/All/Done/Trash.
- **Command palette** (⌘K) and shortcuts for everything; collapsible, resizable sidebar.
- **Settings**: light/dark/system, fonts and sizes (interface zoom), content width, reduce motion.

## Docs

Start with [`AGENTS.md`](AGENTS.md), then [`HANDOFF.md`](HANDOFF.md) and [`docs/`](docs/).
