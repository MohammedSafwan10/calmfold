//! Keepstone domain layer.
//!
//! Pure data types and business rules. This crate performs no IO, owns no
//! clock, and knows nothing about SQLite or the UI. Callers pass the current
//! time in, which keeps every rule deterministic and testable.

pub mod clock;
mod error;
mod id;
pub mod links;
pub mod note;
pub mod task;
mod text;

pub use error::{DomainError, DomainResult};
pub use id::{NoteId, TaskId};
pub use jiff::{Timestamp, civil::Date};
pub use note::Note;
pub use task::{Priority, Task, TaskStatus};
pub use text::{MAX_BODY_BYTES, MAX_TITLE_CHARS, normalize_title};
