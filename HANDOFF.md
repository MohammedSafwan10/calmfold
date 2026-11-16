# HANDOFF

Overwrite **Current state** and **Next steps** every session. Append to the log.

## Current state (2026-11-16)

- App: **Calmfold by Nexdark Labs** (renamed from Keepstone). Branch `main`,
  repo `MohammedSafwan10/calmfold`. `scripts/check.sh` passes; CI runs on
  macOS and Windows.
- Done: notes editor, tasks, Today, calendar (exceptions, ends, ICS import,
  deleted events, holidays for ~245 countries + states, birthdays), habits,
  focus/pomodoro timer (task-linked, header pill when active), password vault
  (recovery, backups), app icon, release workflow (`release.yml`, unsigned
  Windows + macOS zips on `v*` tags).
- Developer workspace: optional sidebar destination with searchable projects,
  links/folder references, real setup Notes and project Tasks, archive/restore,
  and encrypted environment sets shared with Passwords. Migration 0011 and
  item-data v2 for environments only; crypto envelope/KDF unchanged. See
  `docs/plans/11-developer.md` and `docs/security.md` for limits and export policy.
- Developer/environment audit follow-up and Workspace customization are fixed
  and verified; the latest quality gate passes 345 tests (75 UI tests).
  Settings → Workspace controls sidebar order/visibility with direct arrows,
  switches and reset/Undo. Hidden features live in inline More; keyboard and
  command access remain available. General selects the startup page.
- Website is a scaffold/assets snapshot, not a finished site: no pages yet,
  and the release-fetch build step needs a published feed or saved release.json.
  Astro is patched to 7.4.12; npm audit reports zero known vulnerabilities.
- Production updates: native Sparkle/Velopack adapters replace the legacy installer.
  Mac installed upgrade/relaunch and tampered archive tests pass locally and CI.
  Windows native checks caught a boxed SDK type mismatch, corrected for rerun.
  Version 0.1.2 candidate packaging/signing is in progress; no public release yet. See
  `docs/plans/13-production-updates.md` and ADR 0008 for exact gates/limits.

## Next steps (in order)

Everything in `docs/TASKS.md` is done except the
500 KB+ editor item in §1.
Developer's first release is implemented. General utilities, environment
comparison, CLI injection and server/sync integrations remain explicitly deferred.
Workspace customization is complete. Finish website pages and release metadata
before deploying; do not treat the scaffold as a deployable website.
Current priority: native installed-app updater prototypes (Mac first, Windows on
its native runner), then engine adoption and atomic signed release publishing.
The available Sparkle bridge currently uses standard Sparkle UI; GPUI-only UI
needs a tested bridge/helper. MINISIGN_KEY exists in GitHub but its correspondence
to the embedded public key has not been verified. No production release published.

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
- Vault: Touch ID only in signed release builds
  (not built); lock screen without sleep isn't detected. Backups open with
  the master password at export time only (no recovery key in the file).
- Calendar: one occurrence of a repeating event can be moved (drag, or the
  detail panel → "Only this event") or deleted on its own; changing the
  series' schedule resets those. A series can end ("Ends" in the panel).
  Not yet: "this and following", per-occurrence titles/notes. Deleted events
  live in Calendar → View options → Deleted events… (restore, delete
  forever, empty). Times are local wall-clock; `.ics` import converts UTC and
  `TZID` times to local on the way in (Windows zone names fall back to
  local). No ICS export or live subscriptions yet.
- Calendar month cells fit as many lines as the window height allows (estimated
  from the viewport, `MONTH_CHROME`); the rest go behind "N more".

## Useful commands

```bash
rm -rf /tmp/ks && cargo run -p calmfold-storage --example seed -- /tmp/ks/calmfold.sqlite3
CALMFOLD_DATA_DIR=/tmp/ks CALMFOLD_DEV_ROUTE=notes cargo run   # debug: open on a screen
scripts/screenshot.sh target/shot.png
CALMFOLD_DEV_ROUTE=notes CALMFOLD_DEV_SOURCE=1 cargo run    # debug: first note as Markdown source
CALMFOLD_DEV_ROUTE=calendar CALMFOLD_DEV_CALENDAR=week cargo run  # debug: calendar view (month|week|day|agenda)
```

## Log

### 2026-11-16: Native offline release history

- Settings → About → Read release notes and command-palette What’s new open
  a full content-area reading view. Back/Escape returns to About; app navigation
  remains present. Bundled JSON provides version/date, installed marker, concise
  introductions and New/Improved/Fixed sections, shared with signed feed notes.
- Version rail shows one release at a time; a native dropdown replaces it on
  narrow windows. Tests cover old/current selection and dropdown navigation.
  Settings gear has no persistent filled tile; hover/focus remain native.
- Native flow passes opening, back and Escape in both themes. No WebView, remote
  HTML, startup popup or new permanent sidebar entry. Plan 14 tracks review.


### 2026-11-16: Adopt native update engines (release work in progress)

- Replace the custom installer and obsolete unbundled probes with Sparkle 2.10.1
  and Velopack 1.2.161. Shared signed metadata, bounded authenticated downloads,
  OS instance locks, retry backoff, save guards and SQLite recovery snapshots.
- Mac real installed fixture upgrades and relaunches; modified archives are
  rejected while the original stays running. Production nested-helper layout
  passes too. Build Sparkle without UI bits; verify no WebKit dependency.
- Add equivalent Windows installed-package, relaunch, sibling replacement,
  instance lock and catalog/package rejection checks in native CI. Windows
  results and release publication remain pending; do not claim completion.


### 2026-11-16: Start production updater plan and harden existing boundaries

- Added plan 13 and ADR 0008, with explicit installed-app/native-OS gates and
  staged publishing/data-recovery requirements. Keep experimental engine deps
  isolated from the app. Reviewed primary docs and actual crate APIs.
- Stream downloads with a 32 KiB buffer into private temporary files. Enforce
  signed size before writes and hash before unpacking. Reject unsafe release
  paths, malformed hashes, zero/oversized payloads and credential-bearing URLs.
- Atomically sync/persist ready state and propagate failure instead of reporting
  Ready. Hold the interprocess lock from feed fetch through staging. Preserve
  automatic-update preference changes made during a network request.
- Refuse update restart for failed Notes/Developer/Passwords saves, pending
  environment imports and open task sheets/dialogs. Preserve drafts/editors.
- Full gate: 345 tests including 75 UI tests. Native Settings shell reviewed
  light/dark; window was off-screen for real input, so flows use UI tests.
- Isolated Mac engine probes pass development guards; dependency tree has no
  direct GPUI/webview. Added read-only/no-secret Mac/Windows probe workflow.
  This is not a signature/download/install/relaunch test or a production engine.

### 2026-11-16: Finish Workspace customization and organize pending work

- Persist sidebar order, visibility and startup route in layout.json, independent
  of appearance reset. Preserve old installations; fresh installs start with
  Today, Notes, Tasks and Calendar. Unknown/new features remain reachable in More.
- Replace confusing drag controls with direct keyboard-accessible arrows and
  switches. Place More after visible features only when something is hidden;
  leave Settings alone in the footer. Reset supports Undo without changing startup.
- Organize Settings into General, Workspace, Appearance, Features, Privacy & data,
  Keyboard and About. Search reaches hidden-feature preferences. Customize sidebar
  opens Workspace directly. Stable focus survives reordered native settings rows.
- Quality gate passes 337 tests including 74 UI flows: migration/persistence,
  order/hide/all-hidden, startup, reset/Undo, appearance independence, draft/vault
  preservation and running timers. Reviewed window-only light/dark screenshots;
  closed review copies and reopened one real-data app. Windows was not run.
- Review pending work for separate commits. Credential/private-key pattern scan
  found no matches; this is not a formal security audit. Patch website Astro to
  7.4.12 (npm audit: zero known vulnerabilities). Website build remains blocked
  by unpublished release metadata and missing pages; document that explicitly.

### 2026-11-16: Resolve Developer/environment audit findings

- Verified the external review against current code. Preserve failed-validation
  and unapplied-import drafts when changing Passwords items, filters, tags or
  starting another item; the shared pane also refuses to replace a failed draft.
  Correcting the error allows normal save-and-switch behavior.
- Reveal now renders literal values (including multiline content); only the
  masked editing representation is escaped. Single/all-value copies update
  Recently used. Keys trim accidental edge whitespace; values are untouched.
- New Project forms exit on selection, archived-view changes, New environment
  and Escape. The locked Secrets view owns unlock guidance, so no stale banner
  survives unlock. Preserve genuine validation errors instead of clearing all
  errors on every data event.
- Export snapshots while unlocked after confirmation/location selection, then
  writes/syncs in the background. It reports safe specific errors, prevents
  simultaneous exports and still refuses overwrites with private Unix files.
  Lock before the path picker returns prevents the export; an already-started
  explicitly approved export completes its snapshot.
- Remove the extra Developer Secrets scroll container, constrain the pane to
  its available height, size variable lists to their content (six-row cap), and
  expand revealed multiline rows within bounds. Measure actual escaped UTF-8
  output size instead of rejecting ordinary files above approximately 512 KiB.
- Archive-view menu is a real checkable app action (including unchecked native
  accessibility state), settings have Developer search keywords, and project
  deletion emits DeveloperChanged. Sidebar hiding intentionally preserves
  command/shortcut access; it is a visibility setting, not authorization.
- `scripts/check.sh` passes: 329 tests, including 69 native UI integration
  tests. New regressions cover failed navigation, pending imports, key trimming,
  literal multiline reveal, compact one-variable layout, copy usage tracking,
  export collisions/locked pickers, form navigation and archive menu state.

### 2026-11-16: Add Developer projects and encrypted environment sets

- Reuse task ProjectIds and real setup Notes; no second project or vault
  system. Projects can be created or promoted from Tasks, edited, archived
  with Undo, and restored. Hiding Developer in General settings keeps data.
- Add a shared masked variable editor to Developer and Passwords: searchable
  virtual rows, file/paste import with key-only preview, individual/all copy,
  deliberate plaintext export with a location prompt and no overwrite.
  Async imports cannot repopulate a locked or replaced draft.
- Domain parser accepts literal assignments, comments, quotes, multiline
  values and optional export prefixes. Rejects duplicates and oversized
  input with safe errors. Limits: 1 MiB, 512 variables, 64 KiB/value.
- UI testing caught the kit stripping newlines in masked inputs. Values now
  use a documented reversible escaped representation; pasted values escape
  automatically, and copy/export decode them. An 81-variable UI regression
  preserves multiline values through imports, edits, saves and export.
- Ordinary items remain item format v1; environment records use v2 and are
  tested through encryption/reload and encrypted backup recovery. Upgrade
  older clients before using environment items. No crypto primitive changes.
- Owner's visual feedback: replace centered spaced-out project buttons with
  compact left-aligned icon rows, clearer selected state, a narrower list,
  compact tabs and grouped resources. Reviewed native light/dark layouts.
- `scripts/check.sh` passes (324 tests, including 65 UI tests). Tests cover
  v10 upgrades, file persistence/corruption, project navigation/edit/archive,
  literal limits, backup compatibility, export confirmation/0600/no overwrite,
  clipboard clearing, lock cleanup and stale-dialog results. Real clicking
  was unavailable while the Mac's windows were off-screen; flows use real
  native components in headless UI tests.
- `seed_developer` creates a disposable visual-review fixture and refuses
  existing databases. Screenshot/click scripts accept `CALMFOLD_WINDOW_PID`
  so review windows can be targeted without capturing another Calmfold app.

### 2026-11-16: Simplify locked mode control after visual feedback

- Removed the always-visible helper sentence and the native per-tab disabled
  styling that created three separate dark blocks. Keep one standard
  segmented strip, subtly dimmed while active, with the explanation on hover
  only. Domain guards still reject mode changes without resetting the timer.
- The repeated-click regression verifies no toasts, no persistent helper text
  and preserved running/paused state in both themes. `scripts/check.sh` passes
  (309 tests). The owner's running timer was not restarted.

### 2026-11-16: Stop notification stacking on locked focus modes

- Removed the per-click mode-lock toast. Other modes are now disabled while
  running or paused, with a single quiet inline hint; the current mode stays
  selected. Mode choices re-enable and the hint disappears after Finish/Reset.
  Native TabBar uses `FocusKind::ALL` and its labels as the source of truth.
- Extended the real tab-click regression to repeat mode clicks 24 times per
  theme and verify zero notifications, preserved state and hint removal.
  `scripts/check.sh` passes (309 tests). Real-input automation remains
  unreliable here; the changed interaction is covered by the UI test in both
  light and dark. The owner's running session was deliberately left intact.

### 2026-11-16: Quieter focus controls and searchable task selection

- Transport controls (including Start) are icon-only, transparent at rest,
  retaining native hover/focus treatment and shortcut tooltips. Duration is
  one compact menu above the task link and controls, with mode-specific
  presets and inline custom minutes; invalid custom values leave the timer
  unchanged and show the permitted range from core constants.
- Replaced the first-15-task popup with the existing GPUI Kit searchable
  single-select Combobox, a bounded scrollable menu, stable TaskId values,
  task-title truncation, keyboard navigation and explicit empty/error states.
  Selection follows linking, unlinking and task reloads without writes on
  programmatic synchronization. Tasks are filtered in memory; no search IO.
- Added UI tests searching 301 tasks and selecting outside the previous
  cutoff, unlinking, duration presets and invalid/valid custom input.
- `scripts/check.sh` passes: 309 tests including 62 UI tests. Ran the app
  against throwaway data and inspected light/dark Focus screenshots. Real
  scripted clicks still did not activate controls; keyboard and picker flows
  were verified with headless UI tests. Windows was not run on-device.

### 2026-11-16: Protect active focus timers from mode-tab resets

- Mode selection now changes an idle timer only. Running and paused sessions
  retain their countdown, original start, cycle and task link; selecting the
  current tab is a no-op. Attempting another mode explains that Finish or
  Reset is needed. Starting focus from another task cannot overwrite an
  active block either.
- Added a core mode/state matrix regression test and real tab-click UI
  coverage in both themes, plus a real SQLite foreign-key save-failure/retry
  test proving that progress is retained and exactly one session is saved.
- Fixed Focus task-completion Undo to restore InProgress via the legal
  reopen transition; added a UI regression. Cleared existing lint errors in
  the custom-duration blur handler and updater (no updater behavior changes).
- `scripts/check.sh` passes (format, strict Clippy, all workspace tests).
  Headless UI coverage includes running/paused modes, Reset, task linking,
  navigation, save retry and completion Undo.
- Ran the app with throwaway data and visually checked Focus in light and
  dark using window-only screenshots. Closed the review instances; the
  owner's existing app process was left running. Windows was not tested.

### 2026-11-16: Focus / Pomodoro timer

- `core::focus` (`FocusTimer`, `FocusKind`, `FocusPreset`, `FocusSession`,
  `TaskFocusStats`): wall-clock drift-free state machine (`Idle`, `Running`,
  `Paused`) tracking `session_started_at` separately from `segment_started_at`
  so pauses never skew logged start times, plus Pomodoro cycle tracking and
  duration formatting.
- Migration `0010_focus.sql` + `storage::focus`: `focus_sessions` table
  (`ON DELETE SET NULL` on `task_id`), range queries, and per-task focus
  aggregations (Focus blocks only).
- `apps/desktop/src/focus.rs` (`FocusController`): app-wide entity spawning a
  1s tick only while `Running`, registered as a global so task rows and task
  details can start focus directly.
- `views/focus.rs`: calm `ProgressCircle` countdown, 4 cycle dots, linked task
  picker, duration presets (`15m`, `25m`, `50m`, `90m`), gentle completion
  banner (`Start break`, `Mark task done`), and Today's log with undo delete.
- `shell.rs`: quiet title-bar pill (`● 18:42 · Task`) shown only while active
  off the Focus screen, `GoFocus` (`⌘7`), `FocusToggle`, `FocusReset`,
  `FocusFinish`, `FOCUS_SHORTCUTS` in Settings → Keyboard, and midnight reload.
- `task_row.rs` & `task_detail.rs`: `"Start focus"` action and live-updating
  per-task focus totals (`LibraryEvent::FocusChanged`).
- Verified with `cargo fmt --check`, `cargo clippy` (0 warnings in Focus files),
  and `cargo test --workspace` (including `focus_timer` UI test in `ui.rs`).

### 2026-11-11: Build disk fixed

- `[profile.dev]` debug = line-tables-only, dependencies debug = false.
  Clean `cargo build` + `scripts/check.sh` (clippy + all tests): target
  4.0 GB (was ~60 GB). `scripts/clean-build.sh` (cargo-sweep, 7 days)
  keeps it level; run weekly. Local release/perf builds are optional (CI
  builds releases).
- Windows: the title-bar panel toggles `occlude` the drag area, so mouse
  clicks reach them (Windows hit-tests every hitbox and reported caption).
  Today's empty notes hint says Ctrl+N off macOS. Not yet checked on Windows.
- Website work in `website/` (Astro 7.4, real screens via
  `scripts/capture-screens.sh`) is in progress and not committed.

### 2026-11-11: Paused development; cleared generated builds and caches

- At the owner's request, ran `cargo clean` and removed Cargo's registry
  downloads, extracted sources, index and Git dependency caches, plus
  `website/node_modules`. Source, lockfiles, branding and app data were kept.
- Workspace now occupies about 145 MB; disk available space rose from
  about 167 GiB to 230 GiB. Cargo reported removing 87.5 GiB of logical build
  files (about 62 GB as measured by `du` before cleaning).
- On resuming, Cargo will download dependencies and rebuild; run the
  website's package-manager install before working on it. No build/tests
  were run after cleanup, to avoid recreating the removed artifacts.

### 2026-11-09: Renamed to Calmfold (by Nexdark Labs)

- Keepstone → Calmfold everywhere: crates (`calmfold-*`), binary `calmfold`,
  app ID `com.nexdark.calmfold`, menus, themes ("Calmfold Light/Dark"),
  env vars `CALMFOLD_*`, docs. About shows "Made by Nexdark Labs".
- Data: the old `app.Keepstone.Keepstone[-Dev]` folder moves to
  `com.Nexdark.Calmfold[-Dev]` on first launch and `keepstone.sqlite3` is
  renamed (`paths.rs`, tested). Saved theme names map over. Vault backups
  are now `.cfvault`; `.ksvault` still opens.
- Kept on purpose (format identifiers bound into existing data): vault AADs
  `keepstone-vault-key-v1`, `keepstone-vault-recovery-v1`, archive format
  `keepstone-vault-archive`.
- App icon: `assets/brand/source/icon-master.png` → `scripts/gen-icons.py`
  (sizes, `.icns`, `.ico`, favicons + webmanifest). `scripts/bundle-mac.sh`
  builds an unsigned Calmfold.app; Windows embeds the icon via `build.rs`
  (winresource, untested on Windows). About screen shows the icon.
- GitHub repo is still named `keepstone` (rename on GitHub if wanted).

### 2026-11-09: States, birthdays, holiday updates

- Holiday table now from python-holidays 0.108 (Nov 2 2026), generated with
  `uv run scripts/gen-holidays.py` (inline script metadata pins the version).
  Adds 629 states/regions (`S` lines, `IN-TN`); a state only lists days the
  country lacks or that are a day off only there (Pongal in Tamil Nadu).
  Settings → Calendar → Holidays → "States and regions" (searchable).
- Cells use `holidays::short_name` ("Pratihar Shashthi"); the day panel and
  agenda wrap full names; tooltips show everything.
- Birthdays/anniversaries: `core::Occasion` (kind + optional year), migration
  0009, quick add ("Mom's birthday nov 20 1966", "anniversary feb 14 since
  2019"), panel Type (Event/Birthday/Anniversary) + Born/Since year, "turns 60"
  / "5th anniversary" in titles, cake/gift icons, quieter fill, View options
  toggle (`calendar_occasions`). Imported yearly "… birthday" ICS events
  become birthdays.
- Upkeep: `.github/workflows/holidays.yml` opens a PR monthly with the latest
  package's data; a core test fails 3 years before the data ends.

### 2026-11-09: Holidays

- `core::holidays` reads a table generated by `scripts/gen-holidays.py` from
  python-holidays 0.83 (MIT): ~245 countries, 2025–2036, public holidays +
  observances, estimated lunar dates, zone→country map (ADR 0007).
- `apps/desktop/src/holidays.rs`: settings (appearance: `holidays`,
  `holiday_observances`, `holiday_countries`, `holidays_where_you_are`),
  first-launch country from the time zone, travel country added with a label,
  `DayHolidays` per visible range. Calendar reloads when the zone changes.
- Shown as quiet text: month cell header, week/day column header, day panel,
  agenda, Today header and "Next 7 days" day headings. Tooltip and panel say
  "Public holiday in India" / "Observance" / "Date may shift by a day".
- Filters: View options (holidays, observances, where you are, choose
  countries) and Settings → Calendar → Holidays (searchable multi-select).
- Checked in the real window (month view, light, India). Tests: 5 core, UI
  `holidays_show_quietly_follow_settings_and_travel`.
- Regenerate yearly: see the script header.

### 2026-11-09: Calendar import and series ends

- `core::ics::parse_ics` (pure; zone lookup passed in): unfolding, escapes,
  VALARM skipped, DATE/floating/UTC/TZID times, DURATION, RRULE
  (DAILY/WEEKLY+BYDAY/MONTHLY/YEARLY, INTERVAL, UNTIL, COUNT), EXDATE,
  RECURRENCE-ID overrides, CANCELLED. Unsupported rules import their first
  date and are reported. Limits: 20 MB, 20,000 events.
- `Event::until` (`set_until`, migration 0008); panel "Ends" picker
  (placeholder Never, clearable); summary "Every week until Dec 1, 2026".
- `Library::import_ics` skips events already present (title, times, rule).
  `ImportCalendar` action: File menu, ⌘K, Calendar → View options.
- Tests: 6 ICS + 1 until (core), storage round-trip, UI `ics_files_import_once`.

### 2026-11-09: Deleted events

- `views/event_trash.rs`: sheet listing deleted events (Restore, Delete
  forever, Empty trash…, both confirmed). Opened from Calendar → View
  options or ⌘K "Show deleted events" (`ShowDeletedEvents`). Library:
  `deleted_events`, `delete_event_forever` (trashed only), `empty_event_trash`.
  UI test `deleted_events_can_be_restored_or_removed_for_good`.

### 2026-11-09: Change one occurrence of a repeating event

- `core::event::Exception` (Skipped / Moved, keyed by series date) with
  `skip_occurrence`, `move_occurrence`, `restore_exception`,
  `occurrence_on`; `occurrences()` applies them (moved ones can land outside
  their week). Schedule changes clear exceptions; text changes keep them.
  Migration 0007 (`events.exceptions`, text code). `Occurrence` gained
  `series_date` and `moved`.
- Detail panel opens at the clicked occurrence; date/time changes and Delete
  ask "Only this event / All events" (`feedback::ask_repeat_scope`). Dragging
  a repeat moves only that one, with Undo. Tests: 5 core, 1 storage, UI
  `one_occurrence_of_a_repeating_event_can_change_on_its_own`.
- Not checked on screen (real clicks don't reach the window); the flow is
  covered by the UI test.

### 2026-11-09: Encrypted vault backup

- Export vault backup / Restore vault backup (actions, File menu, ⌘K,
  Settings → Passwords → Encrypted backup). Same `.ksvault` format as
  archives, written atomically and verified; restore skips items already in
  the vault. ⌘K also gained Change master password, Create new recovery key,
  Show archived vaults. UI test `encrypted_backup_exports_and_restores_without_duplicates`.

### 2026-11-09: Vault next level + recovery

- Custom fields (text, hidden, website, date, one-time code) on every item;
  templates (Wi-Fi, licence, API key, SSH key, identity, authenticator) under
  New → More, all on the three kinds (no format change).
- Search language (`core::vault::search`): ranked, AND words, never secrets,
  `tag: type: is:fav|weak|reused|old has:code site:`, typo fallback pass
  (letters only, only when nothing matches exactly).
- List: variable-height virtual list with "Recently used" (12+ items) and A–Z
  headers (30+, name sort); Sort by in the … menu; tag chip; sidebar shows the
  top 6 tags and "One-time codes" (live codes in the list).
- Recovery key + Emergency Kit, "Forgot master password?" (recover / archive
  and start over), Archived vaults (ADR 0006, migration 0006).

### 2026-11-09: Password vault

- ADR 0005 (key hierarchy, totp-rs, zxcvbn, EFF list, csv). `core::vault`
  (items, lock machine, generator, health, totp, import), migration 0005,
  `storage/vault.rs` (ciphertext only), `apps/desktop/src/vault.rs` (crypto
  glue, 7 unit tests incl. 2,000-item unlock), `vault_prefs.rs`, screens
  `passwords.rs`, `vault_item.rs`, `vault_generator.rs`, Settings → Passwords.
- External review (GPT) found 4 gaps, all fixed with tests: drafts survived
  locking, generator stayed open after lock, RNG failure fell back to 0, no
  sleep handling (now wall-clock idle + wake detection). Lock screen without
  sleep is still undetectable: documented in `docs/security.md`.
- Dev: `CALMFOLD_DEV_VAULT=<password>` (debug only) unlocks/creates a vault
  with samples; `CALMFOLD_DEV_VAULT_ITEM=<title>` selects one.

### 2026-11-09: Today rebuild

- Sections as data (`today_layout.rs`: key/title/width, resolve/move/rows,
  unit-tested); order + hidden keys in appearance settings. Customize menu on
  Today; Settings → Today reorders with up/down and switches.
- New: at-a-glance strip, schedule with now marker and "Now" badge, habits
  with checks, week card with "habits kept" row, merged next-7-days list.
- Checked in the real window (dark).

### 2026-11-09: Calmer calendar

- Owner feedback: month view noisy (Gym + Standup on every day). Routines now
  fold into dots, cells show titles first, a day panel lists the selected day
  in full (wide windows), View options can hide routines. Week blocks: routines
  quieter, short blocks one line, thinner now line. Checked month + week (dark).

### 2026-11-09: Habits

- Research: Streaks (one big tap per habit), Loop (strength score that
  forgives a missed day), Kadō's EMA spec (α = 0.05). Built our own EMA.
- `core::habit` (schedule, goal, tap, stats, quick add), `core::habit_insights`
  (expected-vs-done rates), migration 0004, `storage/habits.rs`, Library habit
  methods (`HabitsChanged`), screens `habits.rs`, `habit_insights_view.rs`,
  `habit_detail.rs`, `habit_widgets.rs`. Sidebar badge = habits left today.
- First design was a wall of check boxes; redesigned to dots + one check, in
  one card. Checked in the real window (dark): Today and Insights tabs.

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
- Preview rewrites `[[links]]` to `calmfold://note/…` Markdown links
  (`core::links::to_markdown_links`); clicking opens the note or creates it.
- Title matching folds Unicode case in Rust (fixes the `lower()` review item).
- Task detail sheet (`views/task_detail.rs`): click a task or "Details…".
  Title/notes autosave, also on blur and when the sheet closes.
- 9 new tests (core links, storage suggestions, 3 UI flows).

### 2026-11-08: Foundation

- Chose GPUI via gpui-kit 0.9.0 (ADR 0001), SQLite + FTS5 (ADR 0002),
  Argon2id + XChaCha20-Poly1305 (ADR 0003). Name: Calmfold.
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
