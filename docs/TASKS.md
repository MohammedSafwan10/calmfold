# Keepstone work queue

The owner's ordered backlog (2026-11-08). Work top to bottom. Mark `[x]` only
when the item is built, tested at the right layer, looked at in light and
dark, documented, and `scripts/check.sh` passes. Each finished item is its own
commit. Read this file first when resuming.

Quality bar: AGENTS.md. Design rules: `.kiro/skills/gpui-kit-design-guides`.
Calm, organized, not noisy; keyboard path; empty/error states; one source of truth.

## 1. Editor performance (measured, not eyeballed)

- [x] `scripts/perf.sh`: real keystrokes/scroll through the HID tap, GPUI
      profiler (`--features perf`) reports input→present latency p50/p95/p99,
      draw time, late frames, missed refreshes, slow views/tasks.
- [x] Baseline numbers for 1.5 KB / 50 KB / 500 KB / 2 MB notes, typing and scrolling.
- [x] Fix what the numbers show: 7 root-cause patches in the vendored editor
      (`vendor/zorite-editor/PATCHES.md`). 50 KB typing p95 341 → 23 ms, 0 missed refreshes.
- [x] Before/after in `docs/perf.md`. Up to 50 KB: no dropped frames.
- [ ] 500 KB+ at 60 fps needs incremental layout in the editor (reuse unchanged
      lines across edits); today 500 KB ≈ 30 fps, 2 MB ≈ 7 fps. Do after the
      feature phases unless a real note gets that big.

## 2. Editor features left over

- [ ] Find in note (⌘F): find bar, next/prev (Enter/⇧Enter), match count, Esc,
      scroll match into view (`EditorState::set_search`, `find_in_source`).
- [ ] Code-block syntax colors (`set_code_highlighter`, theme-aware).
- [ ] Mermaid diagrams rendered (`set_block_mermaid_provider`), raw on caret.
- [ ] Spell-check (macOS native via `os-spellcheck`, squiggles + suggestions), with a Settings toggle.

## 3. Settings → Appearance, themes, typography

- [ ] Reorganize Appearance into calm groups (Theme, Text, Layout, Motion) with live preview.
- [ ] More themes (e.g. Keepstone Light/Dark, Graphite, Paper, Nord, Solarized,
      Rosé Pine, Gruvbox, Catppuccin), all from theme JSON, light/dark pairs,
      "match system" picks the pair. Smooth switching, no flash.
- [ ] Typography: UI font, reading font for notes, sizes, line height, all persisted and tested.

## 4. Calendar

- [ ] Research native calendar UX (Fantastical, Apple Calendar, Notion Calendar, Cron).
- [ ] Domain: events (all-day, timed, recurring), storage + migration, tests.
- [ ] UI: month / week / day / agenda, keyboard nav, quick add with natural language,
      event detail, tasks with due dates shown, today line. Empty/error states.

## 5. Habits

- [ ] Research (Streaks, Loop Habit Tracker, Things-style calm).
- [ ] Domain: habit, schedule (daily / weekdays / n per week), check-ins, streaks, tests.
- [ ] UI: list with week strip, quick check, history heatmap, reminders later.

## 6. Today dashboard, next level

- [ ] Rebuild after calendar + habits: today's agenda, habits strip, tasks, daily
      note, week chart, organized and calm; sections reusable so more can be added
      later (customizable order/visibility in Settings).

## 7. Password vault

- [ ] Research (1Password, Bitwarden, KeePassXC UX and security).
- [ ] Vault per `docs/plans/05-vault.md` + `docs/security.md`: setup, unlock/lock
      state machine, auto-lock, items (login, card, note), password generator,
      copy with clipboard clear, search. Elegant, calm UI.

## Done

(Items move here with the commit hash.)
