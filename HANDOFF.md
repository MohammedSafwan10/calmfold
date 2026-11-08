# HANDOFF

Overwrite **Current state** and **Next steps** every session. Append to the log.

## Current state (2026-11-09)

- Branch `main`. `scripts/check.sh` passes (fmt, clippy `-D warnings`, all tests).
- Done from `docs/TASKS.md`: editor perf (to 50 KB), find, code colors, Mermaid,
  spell-check, 20 themes + typography, **Calendar** (§4).
- Calendar: `core::event` (+ quick add `event_quick_add`), migration 0003,
  `storage/events.rs`, `Library::calendar` / event methods (`EventsChanged`),
  `views/calendar.rs` (state, header, agenda), `calendar_grid.rs` (month, week/day
  time grid), `calendar_layout.rs` (pure geometry, unit-tested), `event_detail.rs`
  (sheet). Settings → Calendar: week start, 12/24-hour clock.
- Crypto crate for the vault is built and tested; the vault feature is not.

## Next steps (in order)

`docs/TASKS.md` §5 Habits → §6 Today rebuild → §7 Password vault; then the
500 KB+ editor item in §1.

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
- Calendar: repeating events can't be dragged (only whole-series edits exist; no
  per-occurrence exceptions yet). Deleted events have no trash view: Undo is the
  way back. No time zones (local wall-clock times) and no ICS import yet.
- Calendar month cells fit as many lines as the window height allows (estimated
  from the viewport, `MONTH_CHROME`); the rest go behind "N more".

## Useful commands

```bash
rm -rf /tmp/ks && cargo run -p keepstone-storage --example seed -- /tmp/ks/keepstone.sqlite3
KEEPSTONE_DATA_DIR=/tmp/ks KEEPSTONE_DEV_ROUTE=notes cargo run   # debug: open on a screen
scripts/screenshot.sh target/shot.png
KEEPSTONE_DEV_ROUTE=notes KEEPSTONE_DEV_SOURCE=1 cargo run    # debug: first note as Markdown source
KEEPSTONE_DEV_ROUTE=calendar KEEPSTONE_DEV_CALENDAR=week cargo run  # debug: calendar view (month|week|day|agenda)
```

## Log

### 2026-11-09: Calendar

- Research (Fantastical, Apple Calendar, Notion Calendar): one natural-language
  field, month/week/day/agenda, multi-day bars, side-by-side overlaps, a now
  line, drag to reschedule, ⌘←/⌘→ periods, T for today.
- Events are local civil times; all-day ends are exclusive midnights; monthly
  repeats count from the first date (31st stays at month end). Quick add reads
  only the end of the text; undated text lands on the selected day.
- Drag: month cells and the all-day row take events (timed → all-day) and tasks
  (sets the due date); the time grid snaps to 15 min and keeps the length.
  Double-click empty time or a day creates "New event" with its title selected.
- Calendar keys are bound only on the grid (`CalendarGrid` context), so typing
  in quick add never switches views (UI test covers it).
- Month view checked in the real window (dark). Week/day/agenda are covered by
  UI tests only: this session the reader couldn't open fresh screenshots and
  real keys/clicks didn't reach the window.

### 2026-11-09: Measured editor performance

- `scripts/perf.sh` + `--features perf` (`apps/desktop/src/perf.rs`): real HID
  keystrokes/scroll, GPUI foreground journal → latency, draw, late frames.
  `[profile.perf]` = release + symbols. `perf_seed` example makes 1.5 KB…2 MB notes.
- Vendored `zorite-editor` (`vendor/`, `[patch.crates-io]`) with 7 patches; 50 KB
  typing p95 341 → 23 ms with 0 missed refreshes. Details: `docs/perf.md`.
- Gotchas found: GPUI's `caused_invalidation` is false for keys (views notify
  after the handler); `set frontmost` via System Events leaves no key window;
  a full-screen app on another Space hides our window (no frames, no keys).

### 2026-11-08: Live-preview note editor

- Replaced the gpui-kit `Editor` + Preview with `zorite-editor` (audited: no
  `unsafe`, no production panics, env-gated logs). Same `gpui-pre 0.3.11` as
  gpui-kit; hard rule 2 now names this one exception.
- Host work in `note_editor.rs`: theme → `SyntaxStyle`, `[[` suggestions with
  capture-phase keys, selection bar (base `Button`, `focusable(false)`),
  block formats through `core::format` as single undo steps, math provider,
  `EditMath`/`EditProperties` handling (zorite stops the caret otherwise),
  tag-vs-link click routing, URL allow-list (http/https/mailto).
- Verified in the real window (dark + light): bold, links, highlights, colors,
  selection bar, Bold click saved `**Insurance**`.
- Removed: `TogglePreview` (⌘E is now inline code), `link_completion.rs`, `lsp-types`.

### 2026-11-08: Italic and highlight render

- Italic looked upright: Inter italics weren't bundled. Added Inter 4.1
  Italic/Medium/SemiBold/Bold Italic (same release as the others).
- `==highlight==` showed raw: CommonMark has no highlight. `core::format::highlight_spans`
  + preview turns them into `<mark>` (code, links and tags untouched).
- Editor research and decision: see Next steps #1.

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
