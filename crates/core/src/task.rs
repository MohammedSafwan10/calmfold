//! Tasks and their status state machine.
//!
//! ```text
//!            start                complete
//!   Todo ──────────▶ InProgress ──────────▶ Done
//!    │  ◀──────────     │                    │
//!    │     pause        │ cancel             │ reopen
//!    │ complete         ▼                    ▼
//!    ├──────────────▶ Done          Todo ◀───┘
//!    │ cancel
//!    └──────────────▶ Cancelled ──reopen──▶ Todo
//! ```
//!
//! Invariant: `completed_at.is_some()` exactly when `status == Done`.
//! Trash is orthogonal to status: a trashed task keeps its status but rejects
//! every change until restored.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::text::check_body;
use crate::{Date, DomainError, DomainResult, TaskId, Timestamp, normalize_title};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub const ALL: [Self; 4] = [Self::Todo, Self::InProgress, Self::Done, Self::Cancelled];

    /// Whether moving from `self` to `to` is allowed. Same-state moves are
    /// allowed and treated as no-ops.
    #[must_use]
    pub fn can_transition_to(self, to: Self) -> bool {
        use TaskStatus::{Cancelled, Done, InProgress, Todo};
        self == to
            || matches!(
                (self, to),
                (Todo, InProgress | Done | Cancelled)
                    | (InProgress, Todo | Done | Cancelled)
                    | (Done | Cancelled, Todo)
            )
    }

    /// Open tasks still need doing.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(self, Self::Todo | Self::InProgress)
    }

    /// Stable string used in storage. Never change existing values.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == s)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "To do",
            Self::InProgress => "In progress",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    None,
    Low,
    Medium,
    High,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::None, Self::Low, Self::Medium, Self::High];

    /// Stable integer used in storage. Never change existing values.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        match self {
            Self::None => 0,
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
        }
    }

    #[must_use]
    pub fn from_i64(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_i64() == value)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No priority",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    id: TaskId,
    title: String,
    notes: String,
    status: TaskStatus,
    priority: Priority,
    due: Option<Date>,
    created_at: Timestamp,
    updated_at: Timestamp,
    completed_at: Option<Timestamp>,
    deleted_at: Option<Timestamp>,
}

impl Task {
    pub fn new(title: &str, now: Timestamp) -> DomainResult<Self> {
        Ok(Self {
            id: TaskId::new(),
            title: normalize_title(title)?,
            notes: String::new(),
            status: TaskStatus::Todo,
            priority: Priority::None,
            due: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        })
    }

    /// Rebuilds a task from storage. Repairs the `completed_at` invariant if
    /// the stored row violates it, so a corrupted row cannot poison the UI.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_parts(
        id: TaskId,
        title: String,
        notes: String,
        status: TaskStatus,
        priority: Priority,
        due: Option<Date>,
        created_at: Timestamp,
        updated_at: Timestamp,
        completed_at: Option<Timestamp>,
        deleted_at: Option<Timestamp>,
    ) -> Self {
        let completed_at = match status {
            TaskStatus::Done => Some(completed_at.unwrap_or(updated_at)),
            _ => None,
        };
        Self {
            id,
            title,
            notes,
            status,
            priority,
            due,
            created_at,
            updated_at,
            completed_at,
            deleted_at,
        }
    }

    /// Moves to `to` if the state machine allows it. Returns `Ok(false)` for a
    /// same-state no-op.
    pub fn transition(&mut self, to: TaskStatus, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        let from = self.status;
        if from == to {
            return Ok(false);
        }
        if !from.can_transition_to(to) {
            return Err(DomainError::InvalidTransition { from, to });
        }
        self.status = to;
        self.completed_at = (to == TaskStatus::Done).then_some(now);
        self.touch(now);
        Ok(true)
    }

    pub fn complete(&mut self, now: Timestamp) -> DomainResult<bool> {
        self.transition(TaskStatus::Done, now)
    }

    pub fn reopen(&mut self, now: Timestamp) -> DomainResult<bool> {
        self.transition(TaskStatus::Todo, now)
    }

    /// Checkbox behaviour: open tasks complete, closed tasks reopen.
    pub fn toggle_done(&mut self, now: Timestamp) -> DomainResult<bool> {
        if self.status.is_open() {
            self.complete(now)
        } else {
            self.reopen(now)
        }
    }

    pub fn rename(&mut self, title: &str, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        let title = normalize_title(title)?;
        if title == self.title {
            return Ok(false);
        }
        self.title = title;
        self.touch(now);
        Ok(true)
    }

    pub fn set_notes(&mut self, notes: &str, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        check_body(notes)?;
        if notes == self.notes {
            return Ok(false);
        }
        notes.clone_into(&mut self.notes);
        self.touch(now);
        Ok(true)
    }

    pub fn set_priority(&mut self, priority: Priority, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        if priority == self.priority {
            return Ok(false);
        }
        self.priority = priority;
        self.touch(now);
        Ok(true)
    }

    pub fn set_due(&mut self, due: Option<Date>, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        if due == self.due {
            return Ok(false);
        }
        self.due = due;
        self.touch(now);
        Ok(true)
    }

    pub fn trash(&mut self, now: Timestamp) {
        if self.deleted_at.is_none() {
            self.deleted_at = Some(now);
            self.touch(now);
        }
    }

    pub fn restore(&mut self, now: Timestamp) {
        if self.deleted_at.take().is_some() {
            self.touch(now);
        }
    }

    #[must_use]
    pub fn is_overdue(&self, today: Date) -> bool {
        self.status.is_open() && self.due.is_some_and(|due| due < today)
    }

    #[must_use]
    pub fn is_due_on_or_before(&self, day: Date) -> bool {
        self.due.is_some_and(|due| due <= day)
    }

    #[must_use]
    pub fn id(&self) -> TaskId {
        self.id
    }
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.notes
    }
    #[must_use]
    pub fn status(&self) -> TaskStatus {
        self.status
    }
    #[must_use]
    pub fn priority(&self) -> Priority {
        self.priority
    }
    #[must_use]
    pub fn due(&self) -> Option<Date> {
        self.due
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    #[must_use]
    pub fn completed_at(&self) -> Option<Timestamp> {
        self.completed_at
    }
    #[must_use]
    pub fn deleted_at(&self) -> Option<Timestamp> {
        self.deleted_at
    }
    #[must_use]
    pub fn is_trashed(&self) -> bool {
        self.deleted_at.is_some()
    }

    fn ensure_active(&self) -> DomainResult<()> {
        if self.is_trashed() {
            Err(DomainError::Deleted)
        } else {
            Ok(())
        }
    }

    fn touch(&mut self, now: Timestamp) {
        self.updated_at = now.max(self.updated_at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskStatus::{Cancelled, Done, InProgress, Todo};

    fn t(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("valid timestamp")
    }

    fn date(s: &str) -> Date {
        s.parse().expect("valid date")
    }

    /// The complete transition table. If you change the state machine, update
    /// this table and the diagram in the module docs together.
    #[test]
    fn transition_table_is_exact() {
        let allowed = [
            (Todo, InProgress),
            (Todo, Done),
            (Todo, Cancelled),
            (InProgress, Todo),
            (InProgress, Done),
            (InProgress, Cancelled),
            (Done, Todo),
            (Cancelled, Todo),
        ];
        for from in TaskStatus::ALL {
            for to in TaskStatus::ALL {
                let expected = from == to || allowed.contains(&(from, to));
                assert_eq!(from.can_transition_to(to), expected, "{from:?} -> {to:?}");
            }
        }
    }

    #[test]
    fn completed_at_tracks_done_status() {
        let mut task = Task::new("Write plan", t(1)).expect("task");
        assert_eq!(task.completed_at(), None);

        assert_eq!(task.complete(t(2)), Ok(true));
        assert_eq!(task.status(), Done);
        assert_eq!(task.completed_at(), Some(t(2)));

        assert_eq!(task.reopen(t(3)), Ok(true));
        assert_eq!(task.completed_at(), None);
    }

    #[test]
    fn invalid_transitions_are_rejected_without_side_effects() {
        let mut task = Task::new("A", t(1)).expect("task");
        task.complete(t(2)).expect("complete");
        let before = task.clone();
        assert_eq!(
            task.transition(Cancelled, t(3)),
            Err(DomainError::InvalidTransition {
                from: Done,
                to: Cancelled
            })
        );
        assert_eq!(task, before);
    }

    #[test]
    fn same_state_transition_is_a_no_op() {
        let mut task = Task::new("A", t(1)).expect("task");
        assert_eq!(task.transition(Todo, t(5)), Ok(false));
        assert_eq!(task.updated_at(), t(1));
    }

    #[test]
    fn toggle_done_flips_between_open_and_done() {
        let mut task = Task::new("A", t(1)).expect("task");
        task.transition(InProgress, t(2)).expect("start");
        task.toggle_done(t(3)).expect("toggle");
        assert_eq!(task.status(), Done);
        task.toggle_done(t(4)).expect("toggle");
        assert_eq!(task.status(), Todo);

        task.transition(Cancelled, t(5)).expect("cancel");
        task.toggle_done(t(6)).expect("toggle");
        assert_eq!(task.status(), Todo, "toggling a cancelled task reopens it");
    }

    #[test]
    fn trashed_task_rejects_all_changes() {
        let mut task = Task::new("A", t(1)).expect("task");
        task.trash(t(2));
        assert_eq!(task.complete(t(3)), Err(DomainError::Deleted));
        assert_eq!(task.rename("B", t(3)), Err(DomainError::Deleted));
        assert_eq!(
            task.set_priority(Priority::High, t(3)),
            Err(DomainError::Deleted)
        );
        assert_eq!(
            task.set_due(Some(date("2026-01-01")), t(3)),
            Err(DomainError::Deleted)
        );
        task.restore(t(4));
        assert_eq!(task.complete(t(5)), Ok(true));
    }

    #[test]
    fn from_parts_repairs_completed_at_invariant() {
        let id = TaskId::new();
        let open_with_stamp = Task::from_parts(
            id,
            "A".into(),
            String::new(),
            Todo,
            Priority::None,
            None,
            t(1),
            t(2),
            Some(t(2)),
            None,
        );
        assert_eq!(open_with_stamp.completed_at(), None);

        let done_without_stamp = Task::from_parts(
            id,
            "A".into(),
            String::new(),
            Done,
            Priority::None,
            None,
            t(1),
            t(2),
            None,
            None,
        );
        assert_eq!(done_without_stamp.completed_at(), Some(t(2)));
    }

    #[test]
    fn overdue_only_applies_to_open_tasks() {
        let mut task = Task::new("A", t(1)).expect("task");
        task.set_due(Some(date("2026-11-01")), t(2)).expect("due");
        assert!(task.is_overdue(date("2026-11-08")));
        assert!(!task.is_overdue(date("2026-11-01")));
        task.complete(t(3)).expect("complete");
        assert!(!task.is_overdue(date("2026-11-08")));
    }

    #[test]
    fn storage_codes_round_trip() {
        for status in TaskStatus::ALL {
            assert_eq!(TaskStatus::parse(status.as_str()), Some(status));
        }
        for priority in Priority::ALL {
            assert_eq!(Priority::from_i64(priority.as_i64()), Some(priority));
        }
        assert_eq!(TaskStatus::parse("nope"), None);
        assert_eq!(Priority::from_i64(99), None);
    }

    #[test]
    fn rename_validates_title() {
        let mut task = Task::new("A", t(1)).expect("task");
        assert_eq!(task.rename("  ", t(2)), Err(DomainError::EmptyTitle));
        assert_eq!(task.title(), "A");
    }
}
