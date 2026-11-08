use thiserror::Error;

use crate::task::TaskStatus;

/// A rule in the domain layer was violated. Messages are safe to show to users.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("Title can't be empty.")]
    EmptyTitle,
    #[error("Title is longer than {max} characters.")]
    TitleTooLong { max: usize },
    #[error("Content is larger than {max_bytes} bytes.")]
    BodyTooLarge { max_bytes: usize },
    #[error("A task can't move from {from} to {to}.")]
    InvalidTransition { from: TaskStatus, to: TaskStatus },
    #[error("This item is in the trash.")]
    Deleted,
    #[error("The end must be after the start, and an event can last at most a year.")]
    InvalidTimeRange,
}

pub type DomainResult<T> = Result<T, DomainError>;
