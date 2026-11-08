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

use keepstone_core::{
    Date, Note, NoteId, Priority, Project, ProjectId, Recurrence, Task, TaskId, TaskStatus, clock,
    parse_quick_add,
};
use keepstone_storage::{NoteSummary, Progress, Store, TaskCounts, TaskFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryEvent {
    NotesChanged,
    TasksChanged,
}

pub struct Library {
    store: Store,
}

impl EventEmitter<LibraryEvent> for Library {}

impl Library {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
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
}

/// Daily note title: ISO date, the same as Obsidian's default.
#[must_use]
pub fn daily_note_title(date: Date) -> String {
    date.to_string()
}
