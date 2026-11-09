//! The app-wide data model: one `Entity<Library>` wraps the [`Store`].
//!
//! Views never touch SQLite directly. They call methods here, which apply
//! domain rules, persist, and emit a [`LibraryEvent`] so every interested
//! view can refresh. Writes are synchronous: each is a single small SQLite
//! transaction (well under a millisecond in WAL mode). Move to a background
//! writer if profiling ever shows frame drops.

use anyhow::{Context as _, Result};
use gpui_kit::{Context, EventEmitter};
use std::collections::HashMap;
use std::ops::Range;
use std::time::SystemTime;

use keepstone_core::vault::lock::LockState;
use keepstone_core::vault::{ItemFields, VaultItem};
use keepstone_core::{Exception, VaultItemId};
use keepstone_crypto::{KdfParams, SecretString};

use crate::vault::{
    ArchiveFile, KdfInput, RECOVERY_KDF, VaultFilter, VaultSession, VaultSort, derive,
};
use keepstone_core::vault::recovery;
use keepstone_crypto::Zeroizing;

use keepstone_core::{
    Date, DateTime, Event, EventId, Habit, HabitId, Note, NoteId, Occurrence, Priority, Project,
    ProjectId, Recurrence, Schedule, Stats, Task, TaskId, TaskStatus, clock, parse_event,
    parse_habit, parse_quick_add,
};
use keepstone_storage::{NoteSummary, Progress, Store, TaskCounts, TaskFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryEvent {
    NotesChanged,
    TasksChanged,
    EventsChanged,
    HabitsChanged,
    /// Locked, unlocked, or an item changed.
    VaultChanged,
}

pub struct Library {
    store: Store,
    vault: VaultSession,
    /// Argon2 cost for new vaults and password changes (tests lower it).
    kdf: KdfParams,
    /// Last time the user did anything while the vault was unlocked. Wall
    /// clock on purpose: `Instant` stops while macOS sleeps, so time asleep
    /// would not count as idle.
    vault_activity: SystemTime,
    /// Where "Start a new vault" keeps the old encrypted vault.
    archive_dir: std::path::PathBuf,
}

impl EventEmitter<LibraryEvent> for Library {}

impl Library {
    #[must_use]
    pub fn new(store: Store) -> Self {
        let vault = VaultSession::load(&store).unwrap_or_else(|err| {
            eprintln!("keepstone: couldn't read the vault header: {err:#}");
            // Locked, never "set up": setup must not overwrite a header it couldn't read.
            VaultSession::locked()
        });
        Self {
            store,
            vault,
            kdf: KdfParams::RECOMMENDED,
            vault_activity: SystemTime::now(),
            archive_dir: std::env::temp_dir().join("keepstone-vault-archives"),
        }
    }

    // ---- Notes -----------------------------------------------------------

    pub fn notes(&self, trashed: bool) -> Result<Vec<NoteSummary>> {
        Ok(self.store.list_notes(trashed)?)
    }

    pub fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<NoteSummary>> {
        Ok(self.store.search_notes(query, limit)?)
    }

    pub fn note(&self, id: NoteId) -> Result<Option<Note>> {
        Ok(self.store.note(id)?)
    }

    pub fn backlinks(&self, title: &str) -> Result<Vec<NoteSummary>> {
        Ok(self.store.backlinks(title)?)
    }

    pub fn find_note_by_title(&self, title: &str) -> Result<Option<NoteId>> {
        Ok(self.store.find_note_by_title(title)?)
    }

    pub fn suggest_note_titles(&self, query: &str, limit: usize) -> Result<Vec<(NoteId, String)>> {
        Ok(self.store.suggest_note_titles(query, limit)?)
    }

    /// The note a `[[link]]` points to, created (empty) if it doesn't exist yet,
    /// like Obsidian. Trashed notes are not link targets.
    pub fn open_or_create_by_title(
        &mut self,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<NoteId> {
        match self.store.find_note_by_title(title)? {
            Some(id) => Ok(id),
            None => self.create_note(title, cx),
        }
    }

    pub fn note_tags(&self) -> Vec<(String, usize)> {
        self.store.note_tags().unwrap_or_default()
    }

    pub fn notes_with_tag(&self, tag: &str) -> Result<Vec<NoteSummary>> {
        Ok(self.store.notes_with_tag(tag)?)
    }

    pub fn note_ids_with_tag(&self, tag: &str) -> Vec<NoteId> {
        self.store.note_ids_with_tag(tag).unwrap_or_default()
    }

    pub fn unlinked_mentions(&self, id: NoteId, title: &str) -> Vec<NoteSummary> {
        self.store
            .unlinked_mentions(id, title, 20)
            .unwrap_or_default()
    }

    /// Turns the first plain mention of `title` in note `source` into `[[title]]`.
    pub fn link_mention(
        &mut self,
        source: NoteId,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<bool> {
        self.update_note(source, cx, |note| {
            let body = note.body();
            let lower = body.to_lowercase();
            let needle = title.to_lowercase();
            let Some(start) = lower
                .find(&needle)
                .filter(|&i| body.is_char_boundary(i) && body.is_char_boundary(i + needle.len()))
            else {
                return Ok(false);
            };
            let end = start + needle.len();
            let new_body = format!(
                "{}[[{}]]{}",
                &body[..start],
                &body[start..end],
                &body[end..]
            );
            let title = note.title().to_owned();
            Ok(note.edit(&title, &new_body, clock::now())?)
        })
    }

    /// The note titled with `date` (YYYY-MM-DD, like Obsidian), created if needed.
    pub fn open_daily_note(&mut self, date: Date, cx: &mut Context<Self>) -> Result<NoteId> {
        let title = daily_note_title(date);
        if let Some(id) = self.store.find_note_by_title(&title)? {
            return Ok(id);
        }
        let note = Note::new(
            &title,
            format!("## {}\n\n", date.strftime("%A, %B %-d")),
            clock::now(),
        )?;
        self.store
            .save_note(&note)
            .context("Couldn't create today's note")?;
        cx.emit(LibraryEvent::NotesChanged);
        Ok(note.id())
    }

    pub fn daily_note_exists(&self, date: Date) -> bool {
        self.store
            .find_note_by_title(&daily_note_title(date))
            .ok()
            .flatten()
            .is_some()
    }

    /// Writes every active note to `dir` as `Title.md`. Returns how many.
    pub fn export_notes(&self, dir: &std::path::Path) -> Result<usize> {
        let notes = self.store.all_notes()?;
        crate::markdown_io::export(&notes, dir)
    }

    /// Imports `.md` files under `dir`. Notes whose title already exists are
    /// skipped. Returns (imported, skipped).
    pub fn import_notes(
        &mut self,
        dir: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<(usize, usize)> {
        let files = crate::markdown_io::read_dir(dir)?;
        let (mut imported, mut skipped) = (0, 0);
        for (title, body) in files {
            if self.store.find_note_by_title(&title)?.is_some() {
                skipped += 1;
                continue;
            }
            match Note::new(&title, body, clock::now()) {
                Ok(note) => {
                    self.store.save_note(&note)?;
                    imported += 1;
                }
                Err(_) => skipped += 1,
            }
        }
        if imported > 0 {
            cx.emit(LibraryEvent::NotesChanged);
        }
        Ok((imported, skipped))
    }

    pub fn note_count(&self) -> Result<usize> {
        Ok(self.store.count_notes()?)
    }

    pub fn create_note(&mut self, title: &str, cx: &mut Context<Self>) -> Result<NoteId> {
        let note = Note::new(title, "", clock::now())?;
        self.store
            .save_note(&note)
            .context("Couldn't create the note")?;
        cx.emit(LibraryEvent::NotesChanged);
        Ok(note.id())
    }

    /// Saves an edit. Returns `false` (and writes nothing) if the content is unchanged.
    pub fn edit_note(
        &mut self,
        id: NoteId,
        title: &str,
        body: &str,
        cx: &mut Context<Self>,
    ) -> Result<bool> {
        self.update_note(id, cx, |note| Ok(note.edit(title, body, clock::now())?))
    }

    pub fn set_note_pinned(
        &mut self,
        id: NoteId,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_note(id, cx, |note| {
            note.set_pinned(pinned, clock::now())?;
            Ok(true)
        })
        .map(|_| ())
    }

    pub fn trash_note(&mut self, id: NoteId, cx: &mut Context<Self>) -> Result<()> {
        self.update_note(id, cx, |note| {
            note.trash(clock::now());
            Ok(true)
        })
        .map(|_| ())
    }

    pub fn restore_note(&mut self, id: NoteId, cx: &mut Context<Self>) -> Result<()> {
        self.update_note(id, cx, |note| {
            note.restore(clock::now());
            Ok(true)
        })
        .map(|_| ())
    }

    pub fn delete_note_permanently(&mut self, id: NoteId, cx: &mut Context<Self>) -> Result<()> {
        self.store.delete_note_permanently(id)?;
        cx.emit(LibraryEvent::NotesChanged);
        Ok(())
    }

    pub fn empty_note_trash(&mut self, cx: &mut Context<Self>) -> Result<usize> {
        let n = self.store.empty_note_trash()?;
        cx.emit(LibraryEvent::NotesChanged);
        Ok(n)
    }

    fn update_note(
        &mut self,
        id: NoteId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Note) -> Result<bool>,
    ) -> Result<bool> {
        let mut note = self.store.note(id)?.context("That note no longer exists")?;
        if !change(&mut note)? {
            return Ok(false);
        }
        self.store
            .save_note(&note)
            .context("Couldn't save the note")?;
        cx.emit(LibraryEvent::NotesChanged);
        Ok(true)
    }

    // ---- Tasks -----------------------------------------------------------

    pub fn tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        Ok(self.store.list_tasks(filter)?)
    }

    /// One task by ID, or `None` if it was deleted (or can't be read).
    pub fn tasks_by_id(&self, id: TaskId) -> Option<Task> {
        self.store.task(id).ok().flatten()
    }

    pub fn set_task_notes(
        &mut self,
        id: TaskId,
        notes: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.set_notes(notes, clock::now())?))
    }

    pub fn task_counts(&self) -> Result<TaskCounts> {
        let today = clock::today();
        let tz = jiff::tz::TimeZone::system();
        let start = today.to_zoned(tz.clone())?.timestamp();
        let end = today.tomorrow()?.to_zoned(tz)?.timestamp();
        Ok(self.store.task_counts(today, start, end)?)
    }

    pub fn create_task(
        &mut self,
        title: &str,
        due: Option<Date>,
        cx: &mut Context<Self>,
    ) -> Result<TaskId> {
        let now = clock::now();
        let mut task = Task::new(title, now)?;
        task.set_due(due, now)?;
        self.store
            .save_task(&task)
            .context("Couldn't create the task")?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(task.id())
    }

    /// Checkbox: completes or reopens. Completing a repeating task also
    /// creates its next instance, in the same transaction.
    pub fn toggle_task(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        let mut task = self.store.task(id)?.context("That task no longer exists")?;
        if task.status().is_open() {
            self.complete_task(task, cx)
        } else {
            if task.toggle_done(clock::now())? {
                self.store
                    .save_task(&task)
                    .context("Couldn't save the task")?;
                cx.emit(LibraryEvent::TasksChanged);
            }
            Ok(())
        }
    }

    fn complete_task(&mut self, mut task: Task, cx: &mut Context<Self>) -> Result<()> {
        let now = clock::now();
        if !task.complete(now)? {
            return Ok(());
        }
        let next = task.next_occurrence(clock::today(), now);
        task.clear_recurrence_quietly();
        match &next {
            Some(next) => self.store.save_tasks(&[&task, next]),
            None => self.store.save_task(&task),
        }
        .context("Couldn't save the task")?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(())
    }

    /// Creates a task from quick-add text ("Pay rent tomorrow !high #Home").
    /// `default_due` applies when the text has no date; `project` is the
    /// current project view (a `#Project` in the text wins if it exists).
    pub fn quick_add_task(
        &mut self,
        input: &str,
        default_due: Option<Date>,
        project: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) -> Result<TaskId> {
        let now = clock::now();
        let today = clock::today();
        let parsed = parse_quick_add(input, today);
        let mut title = parsed.title.clone();
        let mut project = project;
        if let Some(name) = &parsed.project {
            match self.store.find_project_by_name(name)? {
                Some(p) => project = Some(p.id()),
                // Unknown project: keep the word in the title rather than guess.
                None => title = format!("{title} #{}", name.replace(' ', "_")),
            }
        }
        let mut task = Task::new(&title, now)?;
        task.set_due(parsed.due.or(default_due), now)?;
        if let Some(p) = parsed.priority {
            task.set_priority(p, now)?;
        }
        task.set_recurrence(parsed.recurrence, today, now)?;
        task.set_project(project, now)?;
        self.store
            .save_task(&task)
            .context("Couldn't create the task")?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(task.id())
    }

    pub fn set_task_recurrence(
        &mut self,
        id: TaskId,
        rule: Option<Recurrence>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_task(id, cx, |t| {
            Ok(t.set_recurrence(rule, clock::today(), clock::now())?)
        })
    }

    pub fn set_task_project(
        &mut self,
        id: TaskId,
        project: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.set_project(project, clock::now())?))
    }

    // ---- Subtasks ----

    pub fn subtasks(&self, parent: TaskId) -> Vec<Task> {
        self.store.subtasks(parent).unwrap_or_default()
    }

    pub fn subtask_progress(&self) -> HashMap<TaskId, Progress> {
        self.store.subtask_progress().unwrap_or_default()
    }

    pub fn add_subtask(
        &mut self,
        parent: TaskId,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<TaskId> {
        let task = Task::new_subtask(parent, title, clock::now())?;
        self.store
            .save_task(&task)
            .context("Couldn't add the subtask")?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(task.id())
    }

    pub fn delete_subtask(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        self.store.delete_task_permanently(id)?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(())
    }

    // ---- Projects ----

    pub fn projects(&self) -> Vec<(Project, usize)> {
        self.store.projects().unwrap_or_default()
    }

    pub fn project(&self, id: ProjectId) -> Option<Project> {
        self.store.project(id).ok().flatten()
    }

    pub fn create_project(&mut self, name: &str, cx: &mut Context<Self>) -> Result<ProjectId> {
        let project = Project::new(name, clock::now())?;
        self.store
            .save_project(&project)
            .context("Couldn't create the project")?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(project.id())
    }

    pub fn rename_project(
        &mut self,
        id: ProjectId,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let mut project = self
            .store
            .project(id)?
            .context("That project no longer exists")?;
        if project.rename(name, clock::now())? {
            self.store.save_project(&project)?;
            cx.emit(LibraryEvent::TasksChanged);
        }
        Ok(())
    }

    /// Deletes the project; its tasks stay (without a project).
    pub fn delete_project(&mut self, id: ProjectId, cx: &mut Context<Self>) -> Result<()> {
        self.store.delete_project(id)?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(())
    }

    // ---- Stats ----

    /// Tasks completed on each of the last `days` local days, oldest first.
    pub fn completions_by_day(&self, days: i64) -> Vec<(Date, usize)> {
        let today = clock::today();
        let tz = jiff::tz::TimeZone::system();
        let start_day = today
            .checked_sub(jiff::Span::new().days(days - 1))
            .unwrap_or(today);
        let (Ok(start), Ok(end)) = (
            start_day.to_zoned(tz.clone()),
            today.tomorrow().and_then(|d| d.to_zoned(tz.clone())),
        ) else {
            return Vec::new();
        };
        let stamps = self
            .store
            .completed_between(start.timestamp(), end.timestamp())
            .unwrap_or_default();
        (0..days)
            .filter_map(|i| start_day.checked_add(jiff::Span::new().days(i)).ok())
            .map(|day| {
                let count = stamps
                    .iter()
                    .filter(|ts| ts.to_zoned(tz.clone()).date() == day)
                    .count();
                (day, count)
            })
            .collect()
    }

    /// Any status change. Moving to Done goes through the same path as the
    /// checkbox, so repeating tasks always roll over.
    pub fn set_task_status(
        &mut self,
        id: TaskId,
        status: TaskStatus,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if status == TaskStatus::Done {
            let task = self.store.task(id)?.context("That task no longer exists")?;
            if task.status() != TaskStatus::Done {
                return self.complete_task(task, cx);
            }
            return Ok(());
        }
        self.update_task(id, cx, |t| Ok(t.transition(status, clock::now())?))
    }

    pub fn set_task_due(
        &mut self,
        id: TaskId,
        due: Option<Date>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.set_due(due, clock::now())?))
    }

    pub fn set_task_priority(
        &mut self,
        id: TaskId,
        priority: Priority,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.set_priority(priority, clock::now())?))
    }

    pub fn rename_task(&mut self, id: TaskId, title: &str, cx: &mut Context<Self>) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.rename(title, clock::now())?))
    }

    pub fn trash_task(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        self.update_task(id, cx, |t| {
            t.trash(clock::now());
            Ok(true)
        })
    }

    pub fn restore_task(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        self.update_task(id, cx, |t| {
            t.restore(clock::now());
            Ok(true)
        })
    }

    pub fn empty_task_trash(&mut self, cx: &mut Context<Self>) -> Result<usize> {
        let n = self.store.empty_task_trash()?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(n)
    }

    pub fn delete_task_permanently(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        self.store.delete_task_permanently(id)?;
        cx.emit(LibraryEvent::TasksChanged);
        Ok(())
    }

    fn update_task(
        &mut self,
        id: TaskId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Task) -> Result<bool>,
    ) -> Result<()> {
        let mut task = self.store.task(id)?.context("That task no longer exists")?;
        if change(&mut task)? {
            self.store
                .save_task(&task)
                .context("Couldn't save the task")?;
            cx.emit(LibraryEvent::TasksChanged);
        }
        Ok(())
    }

    // ---- Calendar --------------------------------------------------------

    /// Everything a calendar shows for `days` (end exclusive): event
    /// occurrences in time order (all-day first within a day) and tasks due
    /// in the range.
    pub fn calendar(&self, days: Range<Date>) -> Result<CalendarSpan> {
        let events = self.store.events_between(days.clone())?;
        let mut occurrences: Vec<Occurrence> = events
            .iter()
            .flat_map(|e| e.occurrences(days.clone(), MAX_OCCURRENCES_PER_EVENT))
            .collect();
        occurrences.sort_by_key(|o| (o.start.date(), !o.all_day, o.start, o.end));
        let tasks = self.store.tasks_due_between(days.clone())?;
        Ok(CalendarSpan {
            days,
            occurrences,
            events: events.into_iter().map(|e| (e.id(), e)).collect(),
            tasks,
        })
    }

    /// One event by ID, or `None` if it was deleted (or can't be read).
    pub fn event(&self, id: EventId) -> Option<Event> {
        self.store.event(id).ok().flatten()
    }

    /// Creates an event from quick-add text ("Lunch with Sam fri 1pm").
    /// Text without a day lands on `default_day` (the selected day).
    pub fn quick_add_event(
        &mut self,
        input: &str,
        default_day: Date,
        cx: &mut Context<Self>,
    ) -> Result<EventId> {
        let draft = parse_event(input, clock::today()).on_default_day(default_day);
        let now = clock::now();
        let mut event = if draft.all_day {
            Event::new_all_day(&draft.title, draft.start.date(), draft.last_day(), now)?
        } else {
            Event::new_timed(&draft.title, draft.start, draft.end, now)?
        };
        event.set_recurrence(draft.recurrence, now)?;
        self.save_new_event(&event, cx)
    }

    /// A timed event (double-click on the time grid).
    pub fn create_timed_event(
        &mut self,
        title: &str,
        start: DateTime,
        end: DateTime,
        cx: &mut Context<Self>,
    ) -> Result<EventId> {
        let event = Event::new_timed(title, start, end, clock::now())?;
        self.save_new_event(&event, cx)
    }

    /// An all-day event on one day (double-click on a month day).
    pub fn create_all_day_event(
        &mut self,
        title: &str,
        day: Date,
        cx: &mut Context<Self>,
    ) -> Result<EventId> {
        let event = Event::new_all_day(title, day, day, clock::now())?;
        self.save_new_event(&event, cx)
    }

    fn save_new_event(&mut self, event: &Event, cx: &mut Context<Self>) -> Result<EventId> {
        self.store
            .save_event(event)
            .context("Couldn't create the event")?;
        cx.emit(LibraryEvent::EventsChanged);
        Ok(event.id())
    }

    pub fn rename_event(&mut self, id: EventId, title: &str, cx: &mut Context<Self>) -> Result<()> {
        self.update_event(id, cx, |e| e.rename(title, clock::now()))
    }

    pub fn set_event_notes(
        &mut self,
        id: EventId,
        notes: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| e.set_notes(notes, clock::now()))
    }

    pub fn set_event_location(
        &mut self,
        id: EventId,
        location: &str,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| e.set_location(location, clock::now()))
    }

    /// Makes the event timed, `start..end`.
    pub fn set_event_times(
        &mut self,
        id: EventId,
        start: DateTime,
        end: DateTime,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| e.set_times(start, end, clock::now()))
    }

    /// Makes the event all-day on `first..=last`.
    pub fn set_event_all_day(
        &mut self,
        id: EventId,
        first: Date,
        last: Date,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| e.set_all_day_dates(first, last, clock::now()))
    }

    /// Moves the event to start on `day`, keeping its times and length.
    pub fn move_event(&mut self, id: EventId, day: Date, cx: &mut Context<Self>) -> Result<()> {
        self.update_event(id, cx, |e| e.move_to_date(day, clock::now()))
    }

    pub fn set_event_recurrence(
        &mut self,
        id: EventId,
        rule: Option<Recurrence>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| e.set_recurrence(rule, clock::now()))
    }

    pub fn trash_event(&mut self, id: EventId, cx: &mut Context<Self>) -> Result<()> {
        self.update_event(id, cx, |e| e.trash(clock::now()))
    }

    /// "Delete this event only" on a repeating event. Returns what to pass
    /// to [`Library::restore_occurrence`] for Undo.
    pub fn skip_occurrence(
        &mut self,
        id: EventId,
        series_date: Date,
        cx: &mut Context<Self>,
    ) -> Result<Option<Exception>> {
        let mut previous = None;
        self.update_event(id, cx, |e| {
            previous = e.skip_occurrence(series_date, clock::now())?;
            Ok(())
        })?;
        Ok(previous)
    }

    /// "Change this event only": the occurrence on `series_date` happens
    /// `start..end` instead. Returns what Undo needs.
    pub fn move_occurrence(
        &mut self,
        id: EventId,
        series_date: Date,
        start: DateTime,
        end: DateTime,
        cx: &mut Context<Self>,
    ) -> Result<Option<Exception>> {
        let mut previous = None;
        self.update_event(id, cx, |e| {
            previous = e.move_occurrence(series_date, start, end, clock::now())?;
            Ok(())
        })?;
        Ok(previous)
    }

    /// Moves one occurrence to `day`, keeping its times and length (a drag
    /// onto a month or all-day cell). Returns what Undo needs.
    pub fn move_occurrence_to_day(
        &mut self,
        id: EventId,
        series_date: Date,
        day: Date,
        cx: &mut Context<Self>,
    ) -> Result<Option<Exception>> {
        let occ = self
            .event(id)
            .and_then(|e| e.occurrence_on(series_date))
            .context("That event no longer exists")?;
        let shift = day - occ.start.date();
        let start = occ.start.checked_add(shift)?;
        let end = occ.end.checked_add(shift)?;
        self.move_occurrence(id, series_date, start, end, cx)
    }

    /// Undo for [`Library::skip_occurrence`] and [`Library::move_occurrence`].
    pub fn restore_occurrence(
        &mut self,
        id: EventId,
        series_date: Date,
        previous: Option<Exception>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_event(id, cx, |e| {
            e.restore_exception(series_date, previous, clock::now())
        })
    }

    pub fn restore_event(&mut self, id: EventId, cx: &mut Context<Self>) -> Result<()> {
        let mut event = self
            .store
            .event(id)?
            .context("That event no longer exists")?;
        event.restore(clock::now());
        self.store
            .save_event(&event)
            .context("Couldn't save the event")?;
        cx.emit(LibraryEvent::EventsChanged);
        Ok(())
    }

    fn update_event(
        &mut self,
        id: EventId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Event) -> keepstone_core::DomainResult<()>,
    ) -> Result<()> {
        let mut event = self
            .store
            .event(id)?
            .context("That event no longer exists")?;
        change(&mut event)?;
        self.store
            .save_event(&event)
            .context("Couldn't save the event")?;
        cx.emit(LibraryEvent::EventsChanged);
        Ok(())
    }
}

impl Library {
    // ---- Vault -----------------------------------------------------------

    pub fn vault(&self) -> &VaultSession {
        &self.vault
    }

    pub fn vault_state(&self) -> LockState {
        self.vault.state()
    }

    pub fn vault_is_unlocked(&self) -> bool {
        self.vault.is_unlocked()
    }

    #[doc(hidden)]
    pub fn set_kdf_params_for_tests(&mut self, params: KdfParams) {
        self.kdf = params;
    }

    #[doc(hidden)]
    pub fn backdate_vault_activity_for_tests(&mut self, by: std::time::Duration) {
        if let Some(earlier) = SystemTime::now().checked_sub(by) {
            self.vault_activity = earlier;
        }
    }

    /// Records activity, postponing auto-lock.
    pub fn touch_vault(&mut self) {
        self.vault_activity = SystemTime::now();
    }

    /// Time since the last activity. A clock moved backwards counts as
    /// long idle, so the vault locks rather than stays open.
    pub fn vault_idle(&self) -> std::time::Duration {
        SystemTime::now()
            .duration_since(self.vault_activity)
            .unwrap_or(std::time::Duration::MAX)
    }

    pub fn set_archive_dir(&mut self, dir: std::path::PathBuf) {
        self.archive_dir = dir;
    }

    /// Creates the vault and its recovery key. Argon2 runs in the
    /// background; resolves to the recovery key to show the user once.
    pub fn set_up_vault(
        &mut self,
        password: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<Zeroizing<String>>> {
        let inputs =
            KdfInput::fresh(self.kdf).and_then(|a| Ok((a, KdfInput::fresh(RECOVERY_KDF)?)));
        let ((input, rinput), recovery) = match inputs.and_then(|i| Ok((i, new_recovery_key()?))) {
            Ok(v) => v,
            Err(err) => return gpui_kit::Task::ready(Err(err)),
        };
        cx.spawn(async move |this, cx| {
            let normalized =
                recovery::normalize(&recovery).context("Couldn't make a recovery key")?;
            let (kek, rkek) = cx
                .background_executor()
                .spawn(async move {
                    let rk = SecretString::from(normalized.to_string());
                    anyhow::Ok((
                        derive(&password, &input.salt, input.params)?,
                        derive(&rk, &rinput.salt, rinput.params)?,
                    ))
                })
                .await?;
            this.update(cx, |lib, cx| {
                lib.vault.finish_setup(
                    &mut lib.store,
                    input,
                    &kek,
                    Some((rinput, &rkek)),
                    clock::now(),
                )?;
                lib.touch_vault();
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(recovery)
            })?
        })
    }

    pub fn vault_has_recovery(&self) -> bool {
        VaultSession::has_recovery(&self.store)
    }

    /// Makes a new recovery key (the old one stops working). Unlocked only.
    pub fn new_recovery_key(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<Zeroizing<String>>> {
        let made = KdfInput::fresh(RECOVERY_KDF).and_then(|i| Ok((i, new_recovery_key()?)));
        let (rinput, recovery) = match made {
            Ok(v) => v,
            Err(err) => return gpui_kit::Task::ready(Err(err)),
        };
        cx.spawn(async move |this, cx| {
            let normalized =
                recovery::normalize(&recovery).context("Couldn't make a recovery key")?;
            let rkek = cx
                .background_executor()
                .spawn(async move {
                    derive(
                        &SecretString::from(normalized.to_string()),
                        &rinput.salt,
                        rinput.params,
                    )
                })
                .await?;
            this.update(cx, |lib, cx| {
                lib.vault
                    .set_recovery(&mut lib.store, rinput, &rkek, clock::now())?;
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(recovery)
            })?
        })
    }

    /// Unlocks with the recovery key and sets `new_password` as the master
    /// password. Resolves to `false` for a wrong recovery key.
    pub fn recover_vault(
        &mut self,
        recovery_key: &str,
        new_password: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<bool>> {
        let Some(normalized) = recovery::normalize(recovery_key) else {
            return gpui_kit::Task::ready(Ok(false));
        };
        let inputs = VaultSession::recovery_kdf(&self.store)
            .and_then(|r| Ok((r, KdfInput::fresh(self.kdf)?)));
        let (rinput, new_input) = match inputs {
            Ok(v) => v,
            Err(err) => return gpui_kit::Task::ready(Err(err)),
        };
        cx.spawn(async move |this, cx| {
            let (rkek, new_kek) = cx
                .background_executor()
                .spawn(async move {
                    let rkek = derive(
                        &SecretString::from(normalized.to_string()),
                        &rinput.salt,
                        rinput.params,
                    );
                    (
                        rkek,
                        derive(&new_password, &new_input.salt, new_input.params),
                    )
                })
                .await;
            let new_kek = new_kek?;
            this.update(cx, |lib, cx| {
                let ok =
                    lib.vault
                        .recover(&mut lib.store, rkek, new_input, &new_kek, clock::now())?;
                if ok {
                    lib.touch_vault();
                }
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(ok)
            })?
        })
    }

    /// Archives the encrypted vault to the archive folder and empties it.
    /// Returns the archive's path.
    pub fn start_new_vault(&mut self, cx: &mut Context<Self>) -> Result<std::path::PathBuf> {
        let now = clock::now();
        let stamp = now
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d-%H%M%S")
            .to_string();
        let path = self.archive_dir.join(format!("vault-{stamp}.ksvault"));
        self.vault.archive_and_reset(&mut self.store, &path, now)?;
        cx.emit(LibraryEvent::VaultChanged);
        Ok(path)
    }

    /// Writes an encrypted backup of the (unlocked) vault to `path`. It opens
    /// with the current master password via "Restore from backup".
    pub fn export_vault(&self, path: &std::path::Path) -> Result<()> {
        self.vault.export(&self.store, path, clock::now())?;
        Ok(())
    }

    /// Archived vaults, newest first: (path, archived at, item count).
    pub fn vault_archives(&self) -> Vec<(std::path::PathBuf, keepstone_core::Timestamp, usize)> {
        let Ok(entries) = std::fs::read_dir(&self.archive_dir) else {
            return Vec::new();
        };
        let mut out: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "ksvault"))
            .filter_map(|p| {
                let a = ArchiveFile::read(&p).ok()?;
                Some((p, a.archived_at(), a.item_count()))
            })
            .collect();
        out.sort_by_key(|(_, t, _)| std::cmp::Reverse(*t));
        out
    }

    /// Opens an archive with its old master password and adds its items to
    /// the current (unlocked) vault. Resolves to `None` for a wrong password.
    pub fn import_vault_archive(
        &mut self,
        path: std::path::PathBuf,
        password: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<Option<usize>>> {
        if !self.vault.is_unlocked() {
            return gpui_kit::Task::ready(Err(keepstone_core::DomainError::VaultState {
                state: self.vault.state().label(),
            }
            .into()));
        }
        cx.spawn(async move |this, cx| {
            let items = cx
                .background_executor()
                .spawn(async move {
                    let archive = ArchiveFile::read(&path)?;
                    let k = archive.kdf()?;
                    let kek = derive(&password, &k.salt, k.params)?;
                    VaultSession::open_archive(&archive, &kek)
                })
                .await?;
            let Some(items) = items else {
                return Ok(None);
            };
            this.update(cx, |lib, cx| {
                // New IDs, so importing twice can't overwrite live items.
                // Items identical to one already here are skipped, so
                // restoring the same backup twice doesn't double the vault.
                let now = clock::now();
                let copies: Vec<VaultItem> = items
                    .iter()
                    .filter(|i| {
                        !lib.vault
                            .items()
                            .iter()
                            .any(|live| !live.is_trashed() && live.fields() == i.fields())
                    })
                    .filter_map(|i| {
                        let mut copy = VaultItem::new(i.fields(), now).ok()?;
                        if i.is_favorite() {
                            copy.set_favorite(true, now).ok()?;
                        }
                        Some(copy)
                    })
                    .collect();
                let n = copies.len();
                lib.vault.put(&mut lib.store, copies)?;
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(Some(n))
            })?
        })
    }

    /// Records that a value was copied from `id` ("Recently used").
    pub fn mark_vault_used(&mut self, id: VaultItemId, cx: &mut Context<Self>) {
        let now = clock::now();
        let result = self.vault.update(&mut self.store, id, |i| {
            i.mark_used(now);
            Ok(true)
        });
        if result.is_ok() {
            self.touch_vault();
            cx.emit(LibraryEvent::VaultChanged);
        }
    }

    pub fn vault_tags(&self) -> Vec<(String, usize)> {
        self.vault.tags()
    }

    pub fn vault_recent(&self, n: usize) -> Vec<VaultItemId> {
        self.vault.recent(n)
    }

    /// Unlocks; resolves to `false` for a wrong password.
    pub fn unlock_vault(
        &mut self,
        password: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<bool>> {
        let input = match self.vault.start_unlock(&self.store) {
            Ok(i) => i,
            Err(err) => return gpui_kit::Task::ready(Err(err)),
        };
        cx.emit(LibraryEvent::VaultChanged);
        cx.spawn(async move |this, cx| {
            let kek = cx
                .background_executor()
                .spawn(async move { derive(&password, &input.salt, input.params) })
                .await;
            this.update(cx, |lib, cx| {
                let ok = lib.vault.finish_unlock(&lib.store, kek)?;
                if ok {
                    lib.touch_vault();
                }
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(ok)
            })?
        })
    }

    pub fn lock_vault(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.vault.state(),
            LockState::Unlocked | LockState::Unlocking
        ) {
            self.vault.lock();
            cx.emit(LibraryEvent::VaultChanged);
        }
    }

    /// Changes the master password; resolves to `false` if `current` is wrong.
    pub fn change_master_password(
        &mut self,
        current: SecretString,
        new: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<bool>> {
        let (current_input, new_input) = match (
            VaultSession::current_kdf(&self.store),
            KdfInput::fresh(self.kdf),
        ) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return gpui_kit::Task::ready(Err(e)),
        };
        cx.spawn(async move |this, cx| {
            let (old_kek, new_kek) = cx
                .background_executor()
                .spawn(async move {
                    anyhow::Ok((
                        derive(&current, &current_input.salt, current_input.params)?,
                        derive(&new, &new_input.salt, new_input.params)?,
                    ))
                })
                .await?;
            this.update(cx, |lib, cx| {
                let ok = lib.vault.change_password(
                    &mut lib.store,
                    &old_kek,
                    new_input,
                    &new_kek,
                    clock::now(),
                )?;
                cx.emit(LibraryEvent::VaultChanged);
                anyhow::Ok(ok)
            })?
        })
    }

    pub fn vault_list(
        &mut self,
        filter: VaultFilter,
        tag: Option<&str>,
        query: &str,
        sort: VaultSort,
    ) -> Vec<VaultItemId> {
        self.vault.list(filter, tag, query, sort, clock::now())
    }

    pub fn vault_item(&self, id: VaultItemId) -> Option<VaultItem> {
        self.vault.item(id).cloned()
    }

    pub fn create_vault_item(
        &mut self,
        fields: &ItemFields,
        cx: &mut Context<Self>,
    ) -> Result<VaultItemId> {
        let id = self.vault.create(&mut self.store, fields, clock::now())?;
        self.touch_vault();
        cx.emit(LibraryEvent::VaultChanged);
        Ok(id)
    }

    pub fn update_vault_item(
        &mut self,
        id: VaultItemId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut VaultItem) -> keepstone_core::DomainResult<bool>,
    ) -> Result<()> {
        self.vault.update(&mut self.store, id, change)?;
        self.touch_vault();
        cx.emit(LibraryEvent::VaultChanged);
        Ok(())
    }

    pub fn duplicate_vault_item(
        &mut self,
        id: VaultItemId,
        cx: &mut Context<Self>,
    ) -> Result<VaultItemId> {
        let copy = self
            .vault
            .item(id)
            .context("That item no longer exists")?
            .duplicate(clock::now())?;
        let new_id = copy.id();
        self.vault.put(&mut self.store, vec![copy])?;
        cx.emit(LibraryEvent::VaultChanged);
        Ok(new_id)
    }

    pub fn delete_vault_items(
        &mut self,
        ids: &[VaultItemId],
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.vault.delete(&mut self.store, ids)?;
        cx.emit(LibraryEvent::VaultChanged);
        Ok(())
    }

    /// Imports parsed rows; returns how many were added.
    pub fn import_vault_items(
        &mut self,
        rows: Vec<(ItemFields, bool)>,
        cx: &mut Context<Self>,
    ) -> Result<usize> {
        let now = clock::now();
        let mut items = Vec::with_capacity(rows.len());
        for (fields, favorite) in rows {
            if let Ok(mut item) = VaultItem::new(&fields, now) {
                if favorite {
                    item.set_favorite(true, now)?;
                }
                items.push(item);
            }
        }
        let n = items.len();
        self.vault.put(&mut self.store, items)?;
        cx.emit(LibraryEvent::VaultChanged);
        Ok(n)
    }

    pub fn vault_health(&mut self) -> keepstone_core::vault::health::Report {
        self.vault.health(clock::now())
    }

    pub fn vault_strength(
        &mut self,
        id: VaultItemId,
    ) -> Option<keepstone_core::vault::health::Strength> {
        self.vault.strength_of(id)
    }

    // ---- Habits ----------------------------------------------------------

    /// Habits with today's state, the last 7 days and their statistics.
    pub fn habits(&self, archived: bool) -> Result<Vec<HabitSummary>> {
        let today = clock::today();
        let habits = self.store.habits(archived)?;
        let from = today
            .checked_add(jiff::Span::new().days(-keepstone_core::habit::HISTORY_DAYS))
            .unwrap_or(today);
        let mut checkins = self.store.checkins(from..today.tomorrow()?)?;
        let firsts = self.store.first_checkins()?;
        let tz = jiff::tz::TimeZone::system();
        Ok(habits
            .into_iter()
            .map(|habit| {
                let counts = checkins.remove(&habit.id()).unwrap_or_default();
                let created = habit.created_at().to_zoned(tz.clone()).date();
                let first = firsts
                    .get(&habit.id())
                    .map_or(created, |d| (*d).min(created));
                let stats = habit.stats(&counts, first, today);
                HabitSummary {
                    today_count: counts.get(&today).copied().unwrap_or(0),
                    due_today: habit.schedule().is_due(today),
                    stats,
                    first,
                    counts,
                    habit,
                }
            })
            .collect())
    }

    pub fn habit(&self, id: HabitId) -> Option<HabitSummary> {
        let archived = self.store.habit(id).ok().flatten()?.is_archived();
        self.habits(archived)
            .ok()?
            .into_iter()
            .find(|h| h.habit.id() == id)
    }

    /// Creates a habit from quick-add text ("Run 3x a week").
    pub fn quick_add_habit(&mut self, input: &str, cx: &mut Context<Self>) -> Result<HabitId> {
        let draft = parse_habit(input);
        let now = clock::now();
        let mut habit = Habit::new(&draft.name, draft.schedule, now)?;
        habit.set_target(draft.target, now)?;
        self.store
            .save_habit(&habit)
            .context("Couldn't create the habit")?;
        cx.emit(LibraryEvent::HabitsChanged);
        Ok(habit.id())
    }

    /// One tap on a day: counts up to the goal, then clears.
    pub fn tap_habit(&mut self, id: HabitId, day: Date, cx: &mut Context<Self>) -> Result<u16> {
        let habit = self
            .store
            .habit(id)?
            .context("That habit no longer exists")?;
        let current = self
            .store
            .checkins(day..day.tomorrow()?)?
            .get(&id)
            .and_then(|c| c.get(&day).copied())
            .unwrap_or(0);
        let next = habit.tap(current, day, clock::today())?;
        self.set_habit_count(id, day, next, cx)?;
        Ok(next)
    }

    pub fn set_habit_count(
        &mut self,
        id: HabitId,
        day: Date,
        count: u16,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let habit = self
            .store
            .habit(id)?
            .context("That habit no longer exists")?;
        if habit.is_archived() {
            return Err(keepstone_core::DomainError::Archived.into());
        }
        if day > clock::today() {
            return Err(keepstone_core::DomainError::FutureCheckIn.into());
        }
        self.store
            .set_checkin(id, day, count.min(habit.target()))
            .context("Couldn't save the check-in")?;
        cx.emit(LibraryEvent::HabitsChanged);
        Ok(())
    }

    pub fn rename_habit(&mut self, id: HabitId, name: &str, cx: &mut Context<Self>) -> Result<()> {
        self.update_habit(id, cx, |h| h.rename(name, clock::now()))
    }

    pub fn set_habit_schedule(
        &mut self,
        id: HabitId,
        schedule: Schedule,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_habit(id, cx, |h| h.set_schedule(schedule, clock::now()))
    }

    pub fn set_habit_target(
        &mut self,
        id: HabitId,
        target: u16,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.update_habit(id, cx, |h| h.set_target(target, clock::now()))
    }

    pub fn archive_habit(&mut self, id: HabitId, cx: &mut Context<Self>) -> Result<()> {
        self.update_habit(id, cx, |h| h.archive(clock::now()))
    }

    pub fn unarchive_habit(&mut self, id: HabitId, cx: &mut Context<Self>) -> Result<()> {
        self.update_habit(id, cx, |h| {
            h.unarchive(clock::now());
            Ok(())
        })
    }

    /// Deletes a habit and its whole history (callers confirm first).
    pub fn delete_habit(&mut self, id: HabitId, cx: &mut Context<Self>) -> Result<()> {
        self.store.delete_habit(id)?;
        cx.emit(LibraryEvent::HabitsChanged);
        Ok(())
    }

    pub fn move_habit(&mut self, id: HabitId, index: usize, cx: &mut Context<Self>) -> Result<()> {
        self.store
            .move_habit(id, index)
            .context("Couldn't reorder habits")?;
        cx.emit(LibraryEvent::HabitsChanged);
        Ok(())
    }

    fn update_habit(
        &mut self,
        id: HabitId,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Habit) -> keepstone_core::DomainResult<()>,
    ) -> Result<()> {
        let mut habit = self
            .store
            .habit(id)?
            .context("That habit no longer exists")?;
        change(&mut habit)?;
        self.store
            .save_habit(&habit)
            .context("Couldn't save the habit")?;
        cx.emit(LibraryEvent::HabitsChanged);
        Ok(())
    }
}

/// A habit with what the screens show about it.
#[derive(Debug, Clone)]
pub struct HabitSummary {
    pub habit: Habit,
    pub today_count: u16,
    pub due_today: bool,
    pub stats: Stats,
    /// First day that counts (creation, or an earlier backfilled check-in).
    pub first: Date,
    /// Check-in counts for the history window.
    pub counts: HashMap<Date, u16>,
}

impl HabitSummary {
    #[must_use]
    pub fn done_today(&self) -> bool {
        self.today_count >= self.habit.target()
    }

    #[must_use]
    pub fn count_on(&self, day: Date) -> u16 {
        self.counts.get(&day).copied().unwrap_or(0)
    }

    /// Done this week (Monday-based), for "N times a week" habits.
    #[must_use]
    pub fn done_this_week(&self, today: Date) -> usize {
        let monday = today
            .checked_add(
                jiff::Span::new().days(-i64::from(today.weekday().to_monday_zero_offset())),
            )
            .unwrap_or(today);
        self.counts
            .iter()
            .filter(|(d, c)| **d >= monday && **d <= today && **c >= self.habit.target())
            .count()
    }
}

/// Repeats expanded per event and range; bounds work for daily repeats over a long agenda.
const MAX_OCCURRENCES_PER_EVENT: usize = 500;

/// What a calendar range shows.
#[derive(Debug, Clone, Default)]
pub struct CalendarSpan {
    pub days: Range<Date>,
    pub occurrences: Vec<Occurrence>,
    pub events: HashMap<EventId, Event>,
    pub tasks: Vec<Task>,
}

impl CalendarSpan {
    /// Occurrences that touch `day`, in display order.
    pub fn on_day(&self, day: Date) -> impl Iterator<Item = &Occurrence> {
        self.occurrences
            .iter()
            .filter(move |o| o.days().contains(&day))
    }

    /// Tasks due on `day`.
    pub fn tasks_on(&self, day: Date) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(move |t| t.due() == Some(day))
    }
}

/// Daily note title: ISO date, the same as Obsidian's default.
#[must_use]
pub fn daily_note_title(date: Date) -> String {
    date.to_string()
}

/// A fresh recovery key, formatted for the user.
fn new_recovery_key() -> Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0u8; recovery::KEY_BYTES]);
    keepstone_crypto::random_bytes(&mut bytes[..]).context("Couldn't get secure random numbers")?;
    Ok(recovery::format(&bytes))
}
