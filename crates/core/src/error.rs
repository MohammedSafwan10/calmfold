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
    #[error("That date isn't part of this repeating event.")]
    NoSuchOccurrence,
    #[error("Pick at least one day.")]
    InvalidSchedule,
    #[error("The daily goal must be between 1 and {max}.")]
    InvalidTarget { max: u16 },
    #[error("You can't check in for a day that hasn't happened yet.")]
    FutureCheckIn,
    #[error("This habit is archived. Restore it to change it.")]
    Archived,
    #[error("That isn't a valid one-time password setup key or otpauth:// link.")]
    InvalidOneTimeCode,
    /// Decrypted data didn't parse. Never says what was inside.
    #[error("A vault item couldn't be read.")]
    Corrupt,
    #[error("That action isn't possible while the vault is {state}.")]
    VaultState { state: &'static str },
}

pub type DomainResult<T> = Result<T, DomainError>;
