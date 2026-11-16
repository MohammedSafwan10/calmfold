# Testing

Run everything: `scripts/check.sh`. Current count: 148 tests.

| Layer | Where | Tool | What |
| --- | --- | --- | --- |
| Domain rules | `crates/core/src/*.rs` `#[cfg(test)]` | `#[test]` | Invariants, state-machine tables, parsing edge cases (Unicode, malformed links). |
| Persistence | `crates/storage/src/*.rs` | `#[test]`, in-memory SQLite | Round trips, ordering, filters, FTS search and syntax safety, backlinks, corrupt rows, reopen from file. |
| Crypto | `crates/crypto/src/*.rs` | `#[test]` | Round trip, wrong key/AAD, every-bit tamper, truncation, version, param bounds, zeroize/redaction. |
| Design system | `crates/ui/src/*.rs` | `#[test]` | Theme JSON parses, appearance load/save/clamp/corrupt, every used icon is embedded. |
| UI flows | `apps/desktop/tests/ui.rs` | `#[gpui_kit::test]` + `gpui_kit::test::TestWindowExt` | Real app via `start()`, headless window: actions, typing, Enter, autosave timer, trash/Undo, delete confirmation, palette, theme switch, layout persistence, quick-add parsing, recurrence, steps, projects, daily note, tag filter, mentions, format shortcuts, editor height (no inner scroll), Today chart and upcoming, note editor: byte-identical round trip, selection bar, `[[` menu keys, Markdown source mode, link/tag clicks, arrowing through math. Elements a test queries need `.test_support()`. |

## Writing a UI test

```rust
#[gpui_kit::test]
fn my_flow(cx: &mut TestAppContext) {
    let app = launch(cx);                 // real startup, temp data dir
    dispatch(&app, cx, NewNote);          // run a command
    type_text(&app, cx, "Title");         // type into focused input
    press(&app, cx, "enter");
    click(&app, cx, "note-trash");        // by ElementId
    cx.executor().advance_clock(Duration::from_millis(600)); // timers
    // assert on Library state, not on internals
}
```

- Prefer asserting the stored result (via `Library`) over UI internals.
- Animated surfaces (toasts, dialogs) use wall-clock animation: call
  `cx.set_reduce_motion(true)` first.
- Element IDs are part of the test contract (`note-trash`, `note-delete`, `undo`, `task-list`...).

## Manual check

`CALMFOLD_DATA_DIR=/tmp/ks cargo run`, then `scripts/screenshot.sh /tmp/k.png`
captures only the Calmfold window. Check light and dark mode.

## CI scope and cost

- CI runs the full `scripts/check.sh` on macOS and Windows for application,
  workspace, vendored, asset/theme and quality-workflow changes. Documentation
  and unrelated website changes skip these expensive app jobs. The bundled
  `website/src/data/changelog.json` explicitly counts as app input. Manual
  workflow dispatch remains available for a complete check of any revision.
- Native updater integration runs for updater code, signed-catalog rules,
  native entry points, packaging scripts and dependency/toolchain changes.
  It runs on main pushes or PRs, rather than both a feature-branch push and PR.
  Release candidates always run both native engine suites, regardless of paths.
- A newer branch/PR check cancels superseded checks. Reusable native invocations
  are scoped by their calling workflow so release checks do not cancel ordinary
  CI. Release candidates are queued by version, never canceled mid-signing.
- Release packaging waits for full quality and native gates. Both Mac and
  Windows suites remain mandatory. Raw candidate artifacts are retained three
  days and complete verified candidates seven days; published release downloads
  have their own permanent lifecycle. Caches accelerate compilation and do not
  replace verification. Ordinary CI and release quality jobs share the same
  per-OS dependency cache; release compilation keeps its separate profile cache.
  No cache was deleted as part of this optimization.
- If branch protection later requires the CI status for documentation-only PRs,
  use an always-running lightweight routing job instead of workflow-level path
  filtering, to avoid a required check remaining pending for skipped workflows.
