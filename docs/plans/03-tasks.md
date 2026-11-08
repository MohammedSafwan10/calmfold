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

1. [x] Task detail sheet: title, status (allowed transitions only), due (DatePicker + quick picks), priority, notes, trash
2. [x] Natural-language quick add ("pay rent tomorrow !high #Home"); dates only at the end
3. [x] Recurring tasks (`every day/weekday/monday/2 weeks`, monthly, yearly; complete → next instance)
4. [x] Subtasks as steps in the detail panel; progress on the row
5. [x] Projects (sidebar children from data; rename, delete keeps tasks)
6. [ ] Reorder by drag
7. [ ] Link tasks to notes (`[[note]]` in task notes; task mentions in backlinks)
8. [ ] Reminders (system notifications)
