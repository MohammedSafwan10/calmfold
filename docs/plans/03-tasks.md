# 03 Tasks

## Done

- [x] Task model + status state machine (exhaustively tested), priority, due date
- [x] Filters: Today (incl. overdue), Upcoming, All open, Done, Trash
- [x] Quick add (Enter); tasks added in Today are due today
- [x] Row: checkbox, due label (overdue in danger color), priority, status
- [x] Row menu: due presets, priority, start, cancel, move to trash (Undo)
- [x] Trash: restore, empty trash (confirmed)
- [x] Sidebar child links per filter, due-now badge

## Next

1. [ ] Task detail sheet (notes field, date picker via `DatePicker`)
2. [ ] Natural-language quick add ("pay rent tomorrow !high")
3. [ ] Recurring tasks (rule stored as RRULE subset; complete → next instance)
4. [ ] Subtasks (parent_id, progress)
5. [ ] Projects / lists (sidebar children generated from data)
6. [ ] Reorder by drag
7. [ ] Link tasks to notes (`[[note]]` in task notes; task mentions in backlinks)
8. [ ] Reminders (system notifications)
