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
pub use id::{EventId, NoteId, ProjectId, TaskId};
pub use jiff::{Timestamp, civil::Date};
pub use note::Note;
pub use project::Project;
pub use quick_add::{QuickAdd, parse_quick_add};
pub use recurrence::Recurrence;
pub use task::{Priority, Task, TaskRecord, TaskStatus};
pub use text::{MAX_BODY_BYTES, MAX_TITLE_CHARS, normalize_title};
