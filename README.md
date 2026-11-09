# Calmfold

**Notes, tasks, calendar, habits and passwords, in one calm place.**
By [Nexdark Labs](https://nexdark.com) · [calmfold.nexdark.com](https://calmfold.nexdark.com)

Calmfold is a private, local-first desktop app for macOS and Windows.
Everything lives in one file on your own computer. There's no account, no
server, no tracking. It's written in Rust and draws its UI with
[GPUI](https://www.gpui.rs) (the framework behind the Zed editor) through
[GPUI Kit](https://gpui-kit.com), so it starts fast and doesn't run a browser
engine underneath.

## Download

Get the latest build from [Releases](../../releases):

- **Windows:** `Calmfold-<version>-windows-x64.zip`. Unzip and run
  `Calmfold.exe`. The build isn't code-signed yet, so SmartScreen may say
  "Windows protected your PC": click **More info → Run anyway**.
- **macOS (Apple Silicon):** `Calmfold-<version>-macos-arm64.zip`. Move
  `Calmfold.app` to Applications, then right-click → **Open** the first time.

## What it does

- **Today**: a calm dashboard with your schedule, what's due, habits, the
  next 7 days, holidays and birthdays. Sections can be reordered or hidden.
- **Notes**: a live-preview Markdown editor with `[[wiki links]]` and
  backlinks, tags, find in note, code highlighting, Mermaid diagrams, math,
  spell check, full-text search, pinning, trash with undo, Markdown
  import/export.
- **Tasks**: quick add in plain words ("Pay rent fri high"), due dates,
  priorities, repeats, subtasks, projects, and a trash.
- **Calendar**: month, week, day and agenda views; quick add ("Lunch with Sam
  fri 1pm", "Standup 9:30 every weekday"); drag to move; repeating events with
  one-off changes and end dates; `.ics` import from Google, Apple or Outlook;
  deleted events you can restore. Routines fold into quiet dots.
- **Holidays**: public holidays and festivals for about 245 countries and
  their states, built in and offline, and the holidays of the country you're
  in when you travel.
- **Birthdays and anniversaries**: "Mom's birthday nov 20 1966" shows
  "turns 60" when the day comes.
- **Habits**: daily, weekly or "3× a week" habits, counts like "8 glasses",
  streaks, and an Insights view.
- **Passwords**: an encrypted vault (Argon2id + XChaCha20-Poly1305) with a
  generator, one-time codes, health checks, custom fields, templates, tags,
  CSV import, encrypted backups and a recovery key. It auto-locks.
- **Everywhere**: ⌘K / Ctrl+K command palette, keyboard shortcuts, 20 themes,
  light and dark mode.

## Building from source

The Rust version is pinned in `rust-toolchain.toml`; rustup installs it. On
macOS you need Xcode with the Metal toolchain. On Windows you need the Visual
Studio 2022 C++ build tools, the MSVC Rust toolchain and CMake.

```bash
cargo run                                   # first build takes about 3 minutes
CALMFOLD_DATA_DIR=/tmp/cf cargo run         # use a throwaway data folder
cargo run -p calmfold-storage --example seed -- /tmp/cf/calmfold.sqlite3   # sample data
scripts/dev.sh                              # run, and rebuild + relaunch on every save
scripts/check.sh                            # formatting, clippy and all tests
scripts/bundle-mac.sh                       # build Calmfold.app (unsigned)
uv run scripts/gen-icons.py                 # rebuild icons from assets/brand/source
uv run scripts/gen-holidays.py              # refresh the holiday table
```

Debug builds keep their data in a separate "Calmfold Dev" folder, so running
from source never touches the data of an installed copy.

To publish a release, push a tag (`git tag v0.1.1 && git push origin v0.1.1`).
`.github/workflows/release.yml` builds Windows and macOS and attaches them.

## Where to read next

- [`docs/architecture.md`](docs/architecture.md): how the crates fit together.
- [`docs/security.md`](docs/security.md): what's encrypted and how.
- [`docs/roadmap.md`](docs/roadmap.md) and [`docs/plans/`](docs/plans/): what's next.
- [`HANDOFF.md`](HANDOFF.md): where development currently stands.
- [`AGENTS.md`](AGENTS.md): rules for contributing, for people and coding agents.
