# Architecture

## Layers

```
apps/desktop (GPUI shell, screens, actions)      ← later: apps/mobile, apps/web, server/
   │ uses
   ├── crates/ui       design system (gpui-kit only)
   ├── crates/storage  SQLite + migrations + FTS5
   ├── crates/crypto   vault crypto (no UI, no IO)
   └── crates/core     domain types and rules (no IO)
```

Only `apps/*` and `crates/ui` know about GPUI. Everything a future mobile or web
app needs (rules, storage, crypto) is UI-free, so a new platform is a new shell.

## Data flow

```
user input → View (Entity) → Library method → core rule → Store (SQLite)
                                   │
                                   └─ emit LibraryEvent ──▶ subscribed views reload
```

- `Library` (`apps/desktop/src/library.rs`) is the single owner of the `Store`.
- Views keep their own view state (selection, input entities, tab) and re-query
  on `LibraryEvent`. No view caches another view's data.
- Writes are synchronous single transactions (sub-millisecond in WAL mode). If a
  feature needs slow work (Argon2, indexing, sync), run it with
  `cx.background_spawn` and apply the result on the foreground.

## Startup (`keepstone_desktop::start`)

1. Resolve data folder (`paths.rs`) and open/migrate SQLite (fails fast, before the window).
2. `gpui_kit::init`, then `keepstone_ui::init` (fonts → themes → saved appearance).
3. Bind keys, set menus, register app-level command handlers.
4. Open the window with `AppShell`; register it as the command target.

UI tests call the same `start`, so they exercise the real startup.

## Commands

Every command is a GPUI action. `shell::register_commands` installs app-level
handlers that forward to the shell through `with_shell`, which **defers** the
update (the window is borrowed during dispatch). Shortcuts, the macOS menu bar
and the command palette therefore run identical code.

## State machines

| Thing | Where | Pinned by |
| --- | --- | --- |
| Task status (Todo/InProgress/Done/Cancelled) | `crates/core/src/task.rs` | `transition_table_is_exact` |
| Trash (active ⇄ trashed) for notes and tasks | `note.rs`, `task.rs` | `trashed_*` tests |
| Note editor (Empty/Clean/Dirty, debounced autosave, flush on switch/blur) | `views/notes.rs` | `switching_notes_flushes_unsaved_typing`, `new_note_typing_autosaves_with_links` |
| Appearance (load → sanitize → apply; System follows OS) | `crates/ui/src/appearance.rs` | appearance tests, `theme_commands_switch_mode_and_save` |
| Sidebar layout (width clamp, collapsed, debounced save) | `ui_state.rs`, `shell.rs` | `sidebar_toggle_persists_layout` |

## Files on disk

Data folder (`Settings → General → Data folder`):

- `keepstone.sqlite3` (+ `-wal`, `-shm`): all notes and tasks.
- `appearance.json`: appearance settings.
- `layout.json`: sidebar width/collapsed.

Debug builds use a separate `Keepstone Dev` folder; `KEEPSTONE_DATA_DIR` overrides.

## Sync (planned, not built)

Local-first stays the source of truth. A small Rust server (axum) on the
Oracle VPS via Coolify will store only end-to-end encrypted change blobs.
The schema already has UUID v7 IDs, `updated_at`, and soft deletes for this.
See `docs/plans/06-sync.md`.
