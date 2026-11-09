//! Keepstone domain layer.
//!
//! Pure data types and business rules. This crate performs no IO, owns no
//! clock, and knows nothing about SQLite or the UI. Callers pass the current
//! time in, which keeps every rule deterministic and testable.

pub mod clock;
mod error;
pub mod event;
pub mod event_quick_add;
pub mod format;
pub mod habit;
mod id;
pub mod links;
pub mod note;
pub mod project;
pub mod quick_add;
pub mod recurrence;
pub mod tags;
pub mod task;
mod text;

pub use error::{DomainError, DomainResult};
pub use event::{Event, EventRecord, Occurrence};
pub use event_quick_add::{EventDraft, parse_event};
pub use habit::{Habit, HabitDraft, HabitRecord, Schedule, Stats, parse_habit};
pub use id::{EventId, HabitId, NoteId, ProjectId, TaskId};
pub use jiff::{
    Timestamp,
    civil::{Date, DateTime, Time},
};
pub use note::Note;
pub use project::Project;
pub use quick_add::{QuickAdd, parse_quick_add};
pub use recurrence::{Recurrence, Weekdays};
pub use task::{Priority, Task, TaskRecord, TaskStatus};
pub use text::{MAX_BODY_BYTES, MAX_TITLE_CHARS, normalize_title};
