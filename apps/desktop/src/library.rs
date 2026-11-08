//! The app-wide data model: one `Entity<Library>` wraps the [`Store`].
//!
//! Views never touch SQLite directly. They call methods here, which apply
//! domain rules, persist, and emit a [`LibraryEvent`] so every interested
//! view can refresh. Writes are synchronous: each is a single small SQLite
//! transaction (well under a millisecond in WAL mode). Move to a background
//! writer if profiling ever shows frame drops.

use anyhow::{Context as _, Result};
use gpui_kit::{Context, EventEmitter};
use keepstone_core::{Date, Note, NoteId, Priority, Task, TaskId, TaskStatus, clock};
use keepstone_storage::{NoteSummary, Store, TaskCounts, TaskFilter};

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

    pub fn toggle_task(&mut self, id: TaskId, cx: &mut Context<Self>) -> Result<()> {
        self.update_task(id, cx, |t| Ok(t.toggle_done(clock::now())?))
    }

    pub fn set_task_status(
        &mut self,
        id: TaskId,
        status: TaskStatus,
        cx: &mut Context<Self>,
    ) -> Result<()> {
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
