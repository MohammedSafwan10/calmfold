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

use crate::recurrence::Recurrence;
use crate::text::check_body;
use crate::{Date, DomainError, DomainResult, ProjectId, TaskId, Timestamp, normalize_title};

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
    recurrence: Option<Recurrence>,
    /// Set for subtasks (a checklist item inside another task).
    parent: Option<TaskId>,
    project: Option<ProjectId>,
}

/// Every stored field of a task, for storage to read and write.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct TaskRecord {
    pub id: TaskId,
    pub title: String,
    pub notes: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub due: Option<Date>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub deleted_at: Option<Timestamp>,
    pub recurrence: Option<Recurrence>,
    pub parent: Option<TaskId>,
    pub project: Option<ProjectId>,
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
            recurrence: None,
            parent: None,
            project: None,
        })
    }

    /// A checklist item inside `parent`.
    pub fn new_subtask(parent: TaskId, title: &str, now: Timestamp) -> DomainResult<Self> {
        let mut task = Self::new(title, now)?;
        task.parent = Some(parent);
        Ok(task)
    }

    /// The next instance of a repeating task, or `None` if it doesn't repeat.
    /// Carries title, notes, priority, project and the rule; due date moves on.
    #[must_use]
    pub fn next_occurrence(&self, today: Date, now: Timestamp) -> Option<Self> {
        let rule = self.recurrence?;
        let base = self.due.unwrap_or(today);
        Some(Self {
            id: TaskId::new(),
            title: self.title.clone(),
            notes: self.notes.clone(),
            status: TaskStatus::Todo,
            priority: self.priority,
            due: Some(rule.next_due(base, today)),
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
            recurrence: Some(rule),
            parent: None,
            project: self.project,
        })
    }

    /// Stops this instance from repeating (the next one carries the rule).
    pub fn clear_recurrence_quietly(&mut self) {
        self.recurrence = None;
    }

    pub fn set_recurrence(
        &mut self,
        rule: Option<Recurrence>,
        today: Date,
        now: Timestamp,
    ) -> DomainResult<bool> {
        self.ensure_active()?;
        if rule == self.recurrence {
            return Ok(false);
        }
        self.recurrence = rule;
        // A repeating task needs a date to repeat from.
        if let (Some(rule), None) = (rule, self.due) {
            self.due = Some(rule.first_on_or_after(today));
        }
        self.touch(now);
        Ok(true)
    }

    pub fn set_project(
        &mut self,
        project: Option<ProjectId>,
        now: Timestamp,
    ) -> DomainResult<bool> {
        self.ensure_active()?;
        if project == self.project {
            return Ok(false);
        }
        self.project = project;
        self.touch(now);
        Ok(true)
    }

    #[must_use]
    pub fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
    #[must_use]
    pub fn parent(&self) -> Option<TaskId> {
        self.parent
    }
    #[must_use]
    pub fn project(&self) -> Option<ProjectId> {
        self.project
    }

    /// All stored fields.
    #[must_use]
    pub fn to_record(&self) -> TaskRecord {
        TaskRecord {
            id: self.id,
            title: self.title.clone(),
            notes: self.notes.clone(),
            status: self.status,
            priority: self.priority,
            due: self.due,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at,
            deleted_at: self.deleted_at,
            recurrence: self.recurrence,
            parent: self.parent,
            project: self.project,
        }
    }

    /// Rebuilds a task from storage. Repairs the `completed_at` invariant if
    /// the stored row violates it, so a corrupted row cannot poison the UI.
    #[must_use]
    pub fn from_record(r: TaskRecord) -> Self {
        let completed_at = match r.status {
            TaskStatus::Done => Some(r.completed_at.unwrap_or(r.updated_at)),
            _ => None,
        };
        Self {
            id: r.id,
            title: r.title,
            notes: r.notes,
            status: r.status,
            priority: r.priority,
            due: r.due,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at,
            deleted_at: r.deleted_at,
            recurrence: r.recurrence,
            parent: r.parent,
            project: r.project,
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
    fn from_record_repairs_completed_at_invariant() {
        let mut record = Task::new("A", t(1)).expect("task").to_record();
        record.updated_at = t(2);
        record.completed_at = Some(t(2));
        assert_eq!(
            Task::from_record(record.clone()).completed_at(),
            None,
            "open task can't be completed"
        );
        record.status = Done;
        record.completed_at = None;
        assert_eq!(Task::from_record(record).completed_at(), Some(t(2)));
    }

    #[test]
    fn repeating_tasks_produce_the_next_instance() {
        let today = date("2026-11-08");
        let mut task = Task::new("Water plants", t(1)).expect("task");
        assert!(task.next_occurrence(today, t(2)).is_none());
        task.set_recurrence(Some(Recurrence::DAILY), today, t(2))
            .expect("repeat");
        assert_eq!(task.due(), Some(today), "repeating needs a date");
        task.set_priority(Priority::High, t(2)).expect("prio");

        let next = task.next_occurrence(today, t(3)).expect("next");
        assert_ne!(next.id(), task.id());
        assert_eq!(next.due(), Some(date("2026-11-09")));
        assert_eq!(next.status(), Todo);
        assert_eq!(
            (next.title(), next.priority(), next.recurrence()),
            ("Water plants", Priority::High, Some(Recurrence::DAILY))
        );
    }

    #[test]
    fn subtasks_know_their_parent() {
        let parent = Task::new("Move house", t(1)).expect("task");
        let sub = Task::new_subtask(parent.id(), "Book van", t(1)).expect("sub");
        assert_eq!(sub.parent(), Some(parent.id()));
        assert_eq!(Task::from_record(sub.to_record()), sub, "record round trip");
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
