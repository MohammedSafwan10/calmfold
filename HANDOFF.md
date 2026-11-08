# HANDOFF

Overwrite **Current state** and **Next steps** every session. Append to the log.

## Current state (2026-11-08)

- Branch `main`. `scripts/check.sh` passes: fmt, clippy `-D warnings`, 81 tests.
- Phase 1 foundation is done and runs on macOS (Apple Silicon, macOS 27).
  Windows is untested on a real machine (CI matrix includes it).
- Screens: Today, Notes, Tasks, Settings. Command palette, shortcuts, menus.
- Crypto crate for the vault is built and tested; the vault feature is not.

## Next steps (in order)

1. Notes: clickable `[[links]]` in preview, then `[[` autocomplete (plan 02 #1–2).
2. Tasks: detail sheet with DatePicker and notes field (plan 03 #1).
3. Window size/position restore; app icon (plan 01 open items).
4. Vault (plan 05): migration 0002, lock state machine with table test, setup/unlock UI.
5. Verify a Windows build (VS Build Tools + MSVC + CMake) and fix anything platform-specific.

## Known issues / risks

- GPUI comes from `gpui-pre` snapshots via gpui-kit; upgrades can break APIs.
  Upgrade gpui-kit deliberately, rerun `scripts/check.sh`, look at every screen.
- macOS blocks `osascript` keystrokes here, so real-window input automation
  isn't possible; UI flows are covered by headless tests in `apps/desktop/tests/ui.rs`.
- If the Mac display is idle, windows are off-screen; `scripts/screenshot.sh`
  falls back to off-screen windows.
- Note list isn't virtualized yet (fine for hundreds of notes).
- Global quick-capture hotkey and tray icon are unproven with GPUI; prototype first.

## Useful commands

```bash
rm -rf /tmp/ks && cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3
KEEPSTONE_DATA_DIR=/tmp/ks KEEPSTONE_DEV_ROUTE=notes cargo run   # debug: open on a screen
scripts/screenshot.sh target/shot.png
```

## Log

### 2026-11-08: Foundation

- Chose GPUI via gpui-kit 0.9.0 (ADR 0001), SQLite + FTS5 (ADR 0002),
  Argon2id + XChaCha20-Poly1305 (ADR 0003). Name: Keepstone.
- Built core (notes, tasks + state machine, wiki links), storage (migrations,
  FTS5, backlinks), crypto, design system (themes, fonts, appearance, icons,
  feedback), desktop shell and screens, UI integration tests, docs.
- Bugs found by tests and fixed: app-level commands silently did nothing
  (window borrowed during dispatch, now deferred); extra Lucide icons rendered
  blank (now embedded + guarded by a test); list previews repeated the title.
- Did not code-sign anything; dev builds never touch the keychain.
