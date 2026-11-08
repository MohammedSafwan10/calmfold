# HANDOFF

Overwrite **Current state** and **Next steps** every session. Append to the log.

## Current state (2026-11-08)

- Branch `main`. `scripts/check.sh` passes: fmt, clippy `-D warnings`, 139 tests (29 UI).
- Dev loop: `scripts/dev.sh [--sample]` rebuilds and relaunches on save (watchexec).
- Notes, Tasks and Today are feature-complete for plans 02-04 (see checklists):
  tags, mentions, daily notes, import/export, Markdown formatting (Aa menu +
  shortcuts), quick add with dates/priority/projects, repeats, steps, projects,
  week chart, next 7 days.
- Note editing is Markdown source with a separate Preview (⌘E).
- Crypto crate for the vault is built and tested; the vault feature is not.

## Next steps (in order)

1. Decide on live (visual) editing. Candidate: `gpui-kit-markdown-editor`
   (git dep, block WYSIWYG on `gpui_kit::base::text::editor_host`). Prototype on a
   branch first; must keep `[[links]]` + autocomplete, `#tags`, exact Markdown
   round-trip of existing notes, undo, paste. Known gap: empty paragraphs are dropped.
2. Window size/position restore; app icon (plan 01 open items).
3. Vault (plan 05): migration, lock state machine with table test, setup/unlock UI.
4. Verify a Windows build and fix anything platform-specific.

## Known issues / risks

- GPUI comes from `gpui-pre` snapshots via gpui-kit; upgrades can break APIs.
  Upgrade gpui-kit deliberately, rerun `scripts/check.sh`, look at every screen.
- macOS blocks `osascript` keystrokes here, so real-window input automation
  isn't possible; UI flows are covered by headless tests in `apps/desktop/tests/ui.rs`.
- If the Mac display is idle, windows are off-screen; `scripts/screenshot.sh`
  falls back to off-screen windows.
- Note list isn't virtualized yet (fine for hundreds of notes).
- Sidebar element id includes its width so drag-resize tracks the pointer
  without easing lag; side effect: submenu expansion resets after a resize
  (current route's submenu reopens). Fix properly by keying only while dragging.
- "System" reduce motion is read once at startup.
- Storage limits (VACUUM + FTS rowid, ASCII-only `lower()`): see `crates/storage/AGENTS.md`.
- Global quick-capture hotkey and tray icon are unproven with GPUI; prototype first.

## Useful commands

```bash
rm -rf /tmp/ks && cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3
KEEPSTONE_DATA_DIR=/tmp/ks KEEPSTONE_DEV_ROUTE=notes cargo run   # debug: open on a screen
scripts/screenshot.sh target/shot.png
```

## Log

### 2026-11-08: Finish Notes, Tasks, Today

- Core: `recurrence`, `quick_add`, `tags`, `project`, `format`; migration 0002
  (note_tags, projects, task recurrence/parent/project).
- Notes: three-control toolbar, grouped format menu, ⌘E toggle, indent guides
  off, content-height editor (fixes the scroll-inside-editor bug), tag filter,
  unlinked mentions with Link, daily note, import/export.
- Tasks: quick-add parsing, repeats, steps in the detail panel, project views.
- Today: rewrite with week chart and next 7 days (rows open the task).
- Toasts moved bottom-right (they covered the toolbar).
- 10 new UI tests; queried elements use `.test_support()`.

### 2026-11-08: Panels and menus

- Reusable `SidePanel` (`crates/ui/src/panel.rs`) + shared `LayoutStore`
  (`apps/desktop/src/ui_state.rs`); the notes list is the first user.
  ⌘⇧L / title-bar button toggles it. Tested headless and with real keystrokes.
- Row menus reorganized (submenus show current value, icons, right-click = same menu).
- `[[` completion highlights exactly the typed prefix.
- Git identity: global `Mohammed Safwan <itzmesafwan1@gmail.com>`.

### 2026-11-08: Links, autocomplete, task detail

- Note body is now gpui-kit `Editor` (UI font, soft wrap, no gutter) with a
  `CompletionProvider` (`views/link_completion.rs`): typing `[[` suggests note
  titles (prefix matches first) and inserts `Title]]`.
- Preview rewrites `[[links]]` to `keepstone://note/…` Markdown links
  (`core::links::to_markdown_links`); clicking opens the note or creates it.
- Title matching folds Unicode case in Rust (fixes the `lower()` review item).
- Task detail sheet (`views/task_detail.rs`): click a task or "Details…".
  Title/notes autosave, also on blur and when the sheet closes.
- 9 new tests (core links, storage suggestions, 3 UI flows).

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
- Review fixes: notes and layout now save on ⌘Q and window close; a failed
  save no longer lets the editor switch notes and drop typing; title input
  capped at 300 chars; palette runs commands after it closes and focuses its
  search on open (New note from ⌘K now focuses the title); task menu only
  offers allowed transitions (Start/Pause/Cancel/Reopen); tasks get "Delete
  forever" (confirmed) and an "Empty trash" button on the Trash tab; Upcoming
  quick-add is due tomorrow; Tasks/badges refresh after midnight; load
  failures show an error state instead of an empty list. 3 regression tests added.
