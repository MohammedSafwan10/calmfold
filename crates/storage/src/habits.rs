//! Habits and daily check-ins.

use std::collections::HashMap;
use std::ops::Range;

use ottero_core::habit::Schedule;
use ottero_core::{Date, Habit, HabitId, HabitRecord};
use rusqlite::{OptionalExtension, Row, params};

use crate::codec::{date_to_sql, opt_ts_from_sql, ts_from_sql, ts_to_sql, uuid_from_sql};
use crate::{StorageError, StorageResult, Store};

const COLUMNS: &str = "id, name, schedule, target, created_at, updated_at, archived_at";

impl Store {
    /// Inserts or updates a habit. New habits go to the end of the list.
    pub fn save_habit(&mut self, habit: &Habit) -> StorageResult<()> {
        let r = habit.to_record();
        self.conn.execute(
            "INSERT INTO habits (id, name, schedule, target, position, created_at, updated_at, archived_at)
             VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(position), 0) + 1 FROM habits), ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET
                name = excluded.name,
                schedule = excluded.schedule,
                target = excluded.target,
                updated_at = excluded.updated_at,
                archived_at = excluded.archived_at",
            params![
                r.id.to_string(),
                r.name,
                r.schedule.to_code(),
                r.target,
                ts_to_sql(r.created_at),
                ts_to_sql(r.updated_at),
                r.archived_at.map(ts_to_sql),
            ],
        )?;
        Ok(())
    }

    pub fn habit(&self, id: HabitId) -> StorageResult<Option<Habit>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM habits WHERE id = ?1"),
                [id.to_string()],
                raw_habit,
            )
            .optional()?
            .map(RawHabit::into_habit)
            .transpose()
    }

    /// Active (or archived) habits in list order.
    pub fn habits(&self, archived: bool) -> StorageResult<Vec<Habit>> {
        let filter = if archived { "IS NOT NULL" } else { "IS NULL" };
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {COLUMNS} FROM habits WHERE archived_at {filter} ORDER BY position, created_at"
        ))?;
        let rows = stmt.query_map([], raw_habit)?.collect::<Vec<_>>();
        rows.into_iter()
            .map(|row| row.map_err(StorageError::from)?.into_habit())
            .collect()
    }

    /// Moves `id` to `index` in the active list (drag to reorder).
    pub fn move_habit(&mut self, id: HabitId, index: usize) -> StorageResult<()> {
        let mut ids: Vec<HabitId> = self.habits(false)?.iter().map(Habit::id).collect();
        let Some(from) = ids.iter().position(|h| *h == id) else {
            return Ok(());
        };
        let moved = ids.remove(from);
        ids.insert(index.min(ids.len()), moved);
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = ids.iter().enumerate().try_for_each(|(i, id)| {
            self.conn
                .execute(
                    "UPDATE habits SET position = ?1 WHERE id = ?2",
                    params![i64::try_from(i).unwrap_or(i64::MAX), id.to_string()],
                )
                .map(|_| ())
        });
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(_) => self.conn.execute_batch("ROLLBACK")?,
        }
        Ok(result?)
    }

    pub fn delete_habit(&mut self, id: HabitId) -> StorageResult<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM habits WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    /// Sets the check-in count of one day; 0 removes it.
    pub fn set_checkin(&mut self, id: HabitId, day: Date, count: u16) -> StorageResult<()> {
        if count == 0 {
            self.conn.execute(
                "DELETE FROM habit_checkins WHERE habit_id = ?1 AND day = ?2",
                params![id.to_string(), date_to_sql(day)],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO habit_checkins (habit_id, day, count) VALUES (?1, ?2, ?3)
                 ON CONFLICT (habit_id, day) DO UPDATE SET count = excluded.count",
                params![id.to_string(), date_to_sql(day), count],
            )?;
        }
        Ok(())
    }

    /// Check-in counts per habit for days in `days` (end exclusive).
    pub fn checkins(
        &self,
        days: Range<Date>,
    ) -> StorageResult<HashMap<HabitId, HashMap<Date, u16>>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT habit_id, day, count FROM habit_checkins WHERE day >= ?1 AND day < ?2",
        )?;
        let rows = stmt.query_map([date_to_sql(days.start), date_to_sql(days.end)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut out: HashMap<HabitId, HashMap<Date, u16>> = HashMap::new();
        for row in rows {
            let (id, day, count) = row?;
            let id = HabitId::from_uuid(uuid_from_sql(&id, "habit_checkins.habit_id")?);
            let day: Date = day
                .parse()
                .map_err(|_| StorageError::Corrupt("habit_checkins.day"))?;
            let count =
                u16::try_from(count).map_err(|_| StorageError::Corrupt("habit_checkins.count"))?;
            out.entry(id).or_default().insert(day, count);
        }
        Ok(out)
    }

    /// The earliest check-in of each habit (backfilled days before creation count).
    pub fn first_checkins(&self) -> StorageResult<HashMap<HabitId, Date>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT habit_id, min(day) FROM habit_checkins GROUP BY habit_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (id, day) = row?;
            Ok((
                HabitId::from_uuid(uuid_from_sql(&id, "habit_checkins.habit_id")?),
                day.parse()
                    .map_err(|_| StorageError::Corrupt("habit_checkins.day"))?,
            ))
        })
        .collect()
    }
}

struct RawHabit {
    id: String,
    name: String,
    schedule: String,
    target: i64,
    created_at: i64,
    updated_at: i64,
    archived_at: Option<i64>,
}

impl RawHabit {
    fn into_habit(self) -> StorageResult<Habit> {
        Ok(Habit::from_record(HabitRecord {
            id: HabitId::from_uuid(uuid_from_sql(&self.id, "habits.id")?),
            name: self.name,
            schedule: Schedule::from_code(&self.schedule)
                .ok_or(StorageError::Corrupt("habits.schedule"))?,
            target: u16::try_from(self.target)
                .map_err(|_| StorageError::Corrupt("habits.target"))?,
            created_at: ts_from_sql(self.created_at, "habits.created_at")?,
            updated_at: ts_from_sql(self.updated_at, "habits.updated_at")?,
            archived_at: opt_ts_from_sql(self.archived_at, "habits.archived_at")?,
        }))
    }
}

fn raw_habit(row: &Row<'_>) -> rusqlite::Result<RawHabit> {
    Ok(RawHabit {
        id: row.get(0)?,
        name: row.get(1)?,
        schedule: row.get(2)?,
        target: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        archived_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottero_core::{Timestamp, Weekdays};

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("ts")
    }
    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }
    fn names(habits: &[Habit]) -> Vec<&str> {
        habits.iter().map(Habit::name).collect()
    }

    #[test]
    fn habits_round_trip_order_and_archive() {
        let mut store = Store::open_in_memory().expect("store");
        let mut read = Habit::new("Read", Schedule::Daily, at(1)).expect("habit");
        let gym = Habit::new("Gym", Schedule::Days(Weekdays::WORKDAYS), at(2)).expect("habit");
        let mut water = Habit::new("Water", Schedule::PerWeek(3), at(3)).expect("habit");
        water.set_target(8, at(3)).expect("target");
        for h in [&read, &gym, &water] {
            store.save_habit(h).expect("save");
        }
        assert_eq!(store.habit(water.id()).expect("load"), Some(water.clone()));
        assert_eq!(
            names(&store.habits(false).expect("list")),
            ["Read", "Gym", "Water"]
        );

        store.move_habit(water.id(), 0).expect("move");
        assert_eq!(
            names(&store.habits(false).expect("list")),
            ["Water", "Read", "Gym"]
        );

        read.archive(at(4)).expect("archive");
        store.save_habit(&read).expect("save");
        assert_eq!(names(&store.habits(false).expect("list")), ["Water", "Gym"]);
        assert_eq!(names(&store.habits(true).expect("archived")), ["Read"]);
        assert_eq!(
            names(&store.habits(false).expect("list")),
            ["Water", "Gym"],
            "saving keeps the position"
        );
    }

    #[test]
    fn checkins_set_clear_and_cascade() {
        let mut store = Store::open_in_memory().expect("store");
        let h = Habit::new("Read", Schedule::Daily, at(1)).expect("habit");
        store.save_habit(&h).expect("save");
        store
            .set_checkin(h.id(), d("2026-11-08"), 1)
            .expect("check");
        store
            .set_checkin(h.id(), d("2026-11-09"), 3)
            .expect("check");
        store
            .set_checkin(h.id(), d("2026-11-09"), 2)
            .expect("update");
        let all = store
            .checkins(d("2026-11-01")..d("2026-12-01"))
            .expect("load");
        assert_eq!(all[&h.id()][&d("2026-11-09")], 2);
        assert_eq!(
            store.first_checkins().expect("first")[&h.id()],
            d("2026-11-08")
        );

        store
            .set_checkin(h.id(), d("2026-11-08"), 0)
            .expect("clear");
        let all = store
            .checkins(d("2026-11-01")..d("2026-12-01"))
            .expect("load");
        assert_eq!(all[&h.id()].len(), 1);
        let none = store
            .checkins(d("2026-10-01")..d("2026-11-09"))
            .expect("range");
        assert!(none.is_empty(), "end is exclusive");

        assert!(store.delete_habit(h.id()).expect("delete"));
        assert!(
            store
                .checkins(d("2026-11-01")..d("2026-12-01"))
                .expect("load")
                .is_empty()
        );
    }

    #[test]
    fn corrupt_schedules_surface_as_errors() {
        let store = Store::open_in_memory().expect("store");
        store
            .conn
            .execute(
                "INSERT INTO habits (id, name, schedule, created_at, updated_at) VALUES (?1, 'x', 'bogus', 0, 0)",
                [HabitId::new().to_string()],
            )
            .expect("insert");
        assert!(matches!(
            store.habits(false),
            Err(StorageError::Corrupt("habits.schedule"))
        ));
    }
}
