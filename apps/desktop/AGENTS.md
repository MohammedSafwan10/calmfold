# keepstone-desktop

The GPUI desktop app.

## Map

- `lib.rs`: `run()` (binary) and `start()` (shared with UI tests). Startup order lives here.
- `paths.rs`: data folder (`KEEPSTONE_DATA_DIR` override; debug builds use "Keepstone Dev").
- `library.rs`: `Entity<Library>`, the only path from views to storage. Emits `LibraryEvent`.
- `actions.rs`: every command as an action, default shortcuts, macOS menus.
- `shell.rs`: window chrome, sidebar, routing, command palette, app-level command handlers.
- `nav.rs`: sidebar contents as data (sections → entries → child links, badges).
- `ui_state.rs`: persisted layout (sidebar width/collapsed).
- `views/`: one entity per screen (`today`, `notes`, `tasks`, `settings`) + shared `task_row`.

## Adding a feature screen

1. Domain types + rules in `crates/core`, persistence in `crates/storage`, with tests.
2. `Library` methods that call storage and emit an event.
3. `views/<feature>.rs` entity implementing `Render` + `Focusable`.
4. A `Route` variant, a `NavEntry` in `nav.rs`, a `Go<Feature>` action,
   palette entry, and shortcut (if any).
5. Settings for the feature: a new `SettingPage` or group in `views/settings.rs`.
6. A UI integration test in `tests/ui.rs` driving the main flow.
7. Write the plan in `docs/plans/` first.

## Gotchas

- App-level action handlers must not update the window synchronously; use
  `with_shell` (it defers). Updating a window during its own dispatch silently fails.
- Autosave uses a replaceable `Task` as a debounce; dropping it cancels.
  Always `flush` before switching the open object.
- GPUI `Animation` uses wall-clock time; tests set reduced motion before
  asserting on toasts or other animated surfaces.
