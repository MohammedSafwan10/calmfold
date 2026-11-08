use keepstone_core::{Date, Priority, Task, TaskId, TaskStatus};
use rusqlite::{OptionalExtension, Row, params};

use crate::codec::{
    date_from_sql, date_to_sql, opt_ts_from_sql, ts_from_sql, ts_to_sql, uuid_from_sql,
};
use crate::{StorageError, StorageResult, Store};

/// Which tasks a list shows. Dates are passed in so results are deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    /// Open tasks due today or earlier (includes overdue).
    Today(Date),
    /// Open tasks due after the given day.
    Upcoming(Date),
    /// Every open task.
    Open,
    /// Done or cancelled, most recently finished first.
    Closed,
    /// Trashed tasks.
    Trash,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskCounts {
    pub open: usize,
    pub due_today: usize,
    pub overdue: usize,
    pub completed_today: usize,
}

const COLUMNS: &str =
    "id, title, notes, status, priority, due, created_at, updated_at, completed_at, deleted_at";

/// Open tasks: dated first (soonest first), then by priority, then oldest first.
const OPEN_ORDER: &str = "ORDER BY due IS NULL, due, priority DESC, created_at";

impl Store {
    pub fn save_task(&mut self, task: &Task) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO tasks (id, title, notes, status, priority, due, created_at, updated_at, completed_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (id) DO UPDATE SET
                title = excluded.title,
                notes = excluded.notes,
                status = excluded.status,
                priority = excluded.priority,
                due = excluded.due,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                deleted_at = excluded.deleted_at",
            params![
                task.id().to_string(),
                task.title(),
                task.notes(),
                task.status().as_str(),
                task.priority().as_i64(),
                task.due().map(date_to_sql),
                ts_to_sql(task.created_at()),
                ts_to_sql(task.updated_at()),
                task.completed_at().map(ts_to_sql),
                task.deleted_at().map(ts_to_sql),
            ],
        )?;
        Ok(())
    }

    pub fn task(&self, id: TaskId) -> StorageResult<Option<Task>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM tasks WHERE id = ?1"),
                [id.to_string()],
                raw_task,
            )
            .optional()?
            .map(RawTask::into_task)
            .transpose()
    }

    pub fn list_tasks(&self, filter: TaskFilter) -> StorageResult<Vec<Task>> {
        const OPEN: &str = "deleted_at IS NULL AND status IN ('todo', 'in_progress')";
        let (sql, day) = match filter {
            TaskFilter::Today(day) => (
                format!("SELECT {COLUMNS} FROM tasks WHERE {OPEN} AND due <= ?1 {OPEN_ORDER}"),
                Some(day),
            ),
            TaskFilter::Upcoming(day) => (
                format!("SELECT {COLUMNS} FROM tasks WHERE {OPEN} AND due > ?1 {OPEN_ORDER}"),
                Some(day),
            ),
            TaskFilter::Open => (
                format!("SELECT {COLUMNS} FROM tasks WHERE {OPEN} {OPEN_ORDER}"),
                None,
            ),
            TaskFilter::Closed => (
                format!(
                    "SELECT {COLUMNS} FROM tasks
                     WHERE deleted_at IS NULL AND status IN ('done', 'cancelled')
                     ORDER BY coalesce(completed_at, updated_at) DESC"
                ),
                None,
            ),
            TaskFilter::Trash => (
                format!(
                    "SELECT {COLUMNS} FROM tasks WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
                ),
                None,
            ),
        };
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = match day {
            Some(day) => stmt
                .query_map([date_to_sql(day)], raw_task)?
                .collect::<Vec<_>>(),
            None => stmt.query_map([], raw_task)?.collect::<Vec<_>>(),
        };
        rows.into_iter()
            .map(|row| row.map_err(StorageError::from)?.into_task())
            .collect()
    }

    /// Dashboard counters. `day_start`/`day_end` bound "today" in local time, as UTC timestamps.
    pub fn task_counts(
        &self,
        today: Date,
        day_start: keepstone_core::Timestamp,
        day_end: keepstone_core::Timestamp,
    ) -> StorageResult<TaskCounts> {
        let today = date_to_sql(today);
        let (open, due_today, overdue, completed_today): (i64, i64, i64, i64) =
            self.conn.query_row(
                "SELECT
                    count(*) FILTER (WHERE status IN ('todo', 'in_progress')),
                    count(*) FILTER (WHERE status IN ('todo', 'in_progress') AND due = ?1),
                    count(*) FILTER (WHERE status IN ('todo', 'in_progress') AND due < ?1),
                    count(*) FILTER (WHERE status = 'done' AND completed_at >= ?2 AND completed_at < ?3)
                 FROM tasks WHERE deleted_at IS NULL",
                params![today, ts_to_sql(day_start), ts_to_sql(day_end)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        let n = |v: i64| usize::try_from(v).unwrap_or(0);
        Ok(TaskCounts {
            open: n(open),
            due_today: n(due_today),
            overdue: n(overdue),
            completed_today: n(completed_today),
        })
    }

    pub fn delete_task_permanently(&mut self, id: TaskId) -> StorageResult<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM tasks WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    pub fn empty_task_trash(&mut self) -> StorageResult<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM tasks WHERE deleted_at IS NOT NULL", [])?)
    }
}

struct RawTask {
    id: String,
    title: String,
    notes: String,
    status: String,
    priority: i64,
    due: Option<String>,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
    deleted_at: Option<i64>,
}

impl RawTask {
    fn into_task(self) -> StorageResult<Task> {
        Ok(Task::from_parts(
            TaskId::from_uuid(uuid_from_sql(&self.id, "tasks.id")?),
            self.title,
            self.notes,
            TaskStatus::parse(&self.status).ok_or(StorageError::Corrupt("tasks.status"))?,
            Priority::from_i64(self.priority).ok_or(StorageError::Corrupt("tasks.priority"))?,
            date_from_sql(self.due)?,
            ts_from_sql(self.created_at, "tasks.created_at")?,
            ts_from_sql(self.updated_at, "tasks.updated_at")?,
            opt_ts_from_sql(self.completed_at, "tasks.completed_at")?,
            opt_ts_from_sql(self.deleted_at, "tasks.deleted_at")?,
        ))
    }
}

fn raw_task(row: &Row<'_>) -> rusqlite::Result<RawTask> {
    Ok(RawTask {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        due: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        completed_at: row.get(8)?,
        deleted_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use keepstone_core::{Timestamp, clock};

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("ts")
    }

    fn date(s: &str) -> Date {
        s.parse().expect("date")
    }

    fn titles(tasks: &[Task]) -> Vec<&str> {
        tasks.iter().map(Task::title).collect()
    }

    #[test]
    fn save_and_load_round_trip_including_updates() {
        let mut store = Store::open_in_memory().expect("store");
        let now = clock::now();
        let mut task = Task::new("Pay rent", now).expect("task");
        task.set_due(Some(date("2026-12-01")), now).expect("due");
        task.set_priority(Priority::High, now).expect("prio");
        store.save_task(&task).expect("save");
        assert_eq!(store.task(task.id()).expect("load").as_ref(), Some(&task));

        task.complete(now).expect("done");
        store.save_task(&task).expect("save");
        assert_eq!(store.task(task.id()).expect("load"), Some(task));
    }

    #[test]
    fn filters_split_tasks_by_due_date_and_status() {
        let mut store = Store::open_in_memory().expect("store");
        let today = date("2026-11-08");
        let mk = |title: &str, due: Option<&str>, prio: Priority, secs: i64| {
            let mut t = Task::new(title, at(secs)).expect("task");
            t.set_due(due.map(date), at(secs)).expect("due");
            t.set_priority(prio, at(secs)).expect("prio");
            t
        };
        let overdue = mk("Overdue", Some("2026-11-01"), Priority::None, 1);
        let today_low = mk("Today low", Some("2026-11-08"), Priority::Low, 2);
        let today_high = mk("Today high", Some("2026-11-08"), Priority::High, 3);
        let later = mk("Later", Some("2026-11-20"), Priority::None, 4);
        let undated = mk("Someday", None, Priority::None, 5);
        let mut done = mk("Done", Some("2026-11-08"), Priority::None, 6);
        done.complete(at(7)).expect("done");
        let mut trashed = mk("Trashed", Some("2026-11-08"), Priority::None, 8);
        trashed.trash(at(9));
        for t in [
            &overdue,
            &today_low,
            &today_high,
            &later,
            &undated,
            &done,
            &trashed,
        ] {
            store.save_task(t).expect("save");
        }

        assert_eq!(
            titles(&store.list_tasks(TaskFilter::Today(today)).expect("today")),
            ["Overdue", "Today high", "Today low"]
        );
        assert_eq!(
            titles(
                &store
                    .list_tasks(TaskFilter::Upcoming(today))
                    .expect("upcoming")
            ),
            ["Later"]
        );
        assert_eq!(
            titles(&store.list_tasks(TaskFilter::Open).expect("open")),
            ["Overdue", "Today high", "Today low", "Later", "Someday"]
        );
        assert_eq!(
            titles(&store.list_tasks(TaskFilter::Closed).expect("closed")),
            ["Done"]
        );
        assert_eq!(
            titles(&store.list_tasks(TaskFilter::Trash).expect("trash")),
            ["Trashed"]
        );

        let counts = store.task_counts(today, at(0), at(100)).expect("counts");
        assert_eq!(
            counts,
            TaskCounts {
                open: 5,
                due_today: 2,
                overdue: 1,
                completed_today: 1
            }
        );
    }

    #[test]
    fn database_rejects_invalid_status_values() {
        let store = Store::open_in_memory().expect("store");
        let result = store.conn.execute(
            "INSERT INTO tasks (id, title, status, created_at, updated_at) VALUES ('x', 'y', 'bogus', 0, 0)",
            [],
        );
        assert!(
            result.is_err(),
            "CHECK constraint must reject unknown status"
        );
    }

    #[test]
    fn corrupt_rows_surface_as_errors_not_panics() {
        let store = Store::open_in_memory().expect("store");
        store
            .conn
            .execute(
                "INSERT INTO tasks (id, title, status, due, created_at, updated_at)
                 VALUES (?1, 'y', 'todo', 'not-a-date', 0, 0)",
                [TaskId::new().to_string()],
            )
            .expect("insert");
        assert!(matches!(
            store.list_tasks(TaskFilter::Open),
            Err(StorageError::Corrupt("tasks.due"))
        ));
    }

    #[test]
    fn empty_trash_and_permanent_delete() {
        let mut store = Store::open_in_memory().expect("store");
        let keep = Task::new("Keep", at(1)).expect("task");
        let mut gone = Task::new("Gone", at(1)).expect("task");
        gone.trash(at(2));
        store.save_task(&keep).expect("save");
        store.save_task(&gone).expect("save");
        assert_eq!(store.empty_task_trash().expect("empty"), 1);
        assert!(store.delete_task_permanently(keep.id()).expect("delete"));
        assert!(
            !store
                .delete_task_permanently(keep.id())
                .expect("delete again")
        );
    }

    #[test]
    fn file_database_persists_across_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/keepstone.sqlite3");
        let task = Task::new("Persist me", clock::now()).expect("task");
        {
            let mut store = Store::open(&path).expect("open");
            store.save_task(&task).expect("save");
        }
        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.task(task.id()).expect("load"), Some(task));
        assert_eq!(store.schema_version().expect("version"), 1);
    }
}
