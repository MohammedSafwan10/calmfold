# Keepstone

Keepstone is a personal notes and tasks app for the desktop. Everything is
stored in one SQLite file on your own machine. There is no account, no server,
and no sync unless you set one up yourself.

It's written in Rust and draws its UI with [GPUI](https://www.gpui.rs) (the
framework behind the Zed editor) through [GPUI Kit](https://gpui-kit.com), so
it starts fast and doesn't run a browser engine underneath.

A password vault, calendar, habit tracker and budget are planned. The
encryption code for the vault already exists in `crates/crypto`, but there's
no vault screen yet.

## What works today

- **Today**: tasks due today or overdue, how many you finished today, and your
  most recent notes.
- **Notes**: Markdown with an edit/preview switch. Link notes with
  `[[Note title]]` and see which notes link back. Full-text search, pinning,
  and a trash with undo. Notes save themselves half a second after you stop
  typing.
- **Tasks**: type a title and press Enter. Set due dates, priority, and status
  (to do, in progress, done, cancelled). Lists for Today, Upcoming, All open,
  Done, and Trash.
- **Keyboard**: a command palette on ⌘K (Ctrl+K on Windows and Linux) and
  shortcuts for the main screens.
- **Settings**: light, dark, or follow the system; interface and code font
  sizes; how wide notes can get; reduced motion.

## Building it

The Rust version is pinned in `rust-toolchain.toml`, and rustup installs it
for you. On macOS you need Xcode with the Metal toolchain. On Windows you need
the Visual Studio 2022 C++ build tools, the MSVC Rust toolchain, and CMake.

```bash
cargo run                               # first build takes about 3 minutes
KEEPSTONE_DATA_DIR=/tmp/ks cargo run    # use a throwaway data folder
cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3   # fill it with sample data
scripts/check.sh                        # formatting, clippy, and all tests
```

Debug builds keep their data in a separate "Keepstone Dev" folder, so running
from source never touches the data of an installed copy.

## Where to read next

- [`docs/architecture.md`](docs/architecture.md) explains how the crates fit together.
- [`docs/roadmap.md`](docs/roadmap.md) and [`docs/plans/`](docs/plans/) list what's coming.
- [`HANDOFF.md`](HANDOFF.md) describes where development currently stands.
- [`AGENTS.md`](AGENTS.md) has the rules for contributing, for people and coding agents.
