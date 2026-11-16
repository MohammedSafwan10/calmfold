use rusqlite::{Connection, Transaction};
use rusqlite_migration::{HookResult, M, Migrations};

use crate::StorageResult;

/// Ordered list of migrations. Append only; never edit or reorder entries.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/0001_init.sql")),
        M::up_with_hook(
            include_str!("../migrations/0002_tags_projects_subtasks.sql"),
            backfill_tags,
        ),
        M::up(include_str!("../migrations/0003_events.sql")),
        M::up(include_str!("../migrations/0004_habits.sql")),
        M::up(include_str!("../migrations/0005_vault.sql")),
        M::up(include_str!("../migrations/0006_vault_recovery.sql")),
        M::up(include_str!("../migrations/0007_event_exceptions.sql")),
        M::up(include_str!("../migrations/0008_event_until.sql")),
        M::up(include_str!("../migrations/0009_event_occasion.sql")),
        M::up(include_str!("../migrations/0010_focus.sql")),
        M::up(include_str!("../migrations/0011_developer.sql")),
    ])
}

pub(crate) fn migrate(conn: &mut Connection) -> StorageResult<()> {
    migrations().to_latest(conn)?;
    Ok(())
}

/// v2: index `#tags` of notes written before tags existed.
fn backfill_tags(tx: &Transaction) -> HookResult {
    let notes: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, body FROM notes")?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let mut insert =
        tx.prepare("INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?1, ?2)")?;
    for (id, body) in notes {
        for tag in calmfold_core::tags::extract_tags(&body) {
            insert.execute(rusqlite::params![id, tag])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().expect("migrations validate");
    }

    #[test]
    fn upgrading_v10_preserves_projects_and_related_data() {
        let mut conn = Connection::open_in_memory().expect("db");
        migrations().to_version(&mut conn, 10).expect("v10");
        conn.execute(
            "INSERT INTO projects (id,name,created_at,updated_at) VALUES ('p1','Existing',0,0)",
            [],
        )
        .expect("project");
        conn.execute("INSERT INTO tasks (id,title,status,project_id,created_at,updated_at) VALUES ('t1','Build','todo','p1',0,0)", []).expect("task");
        conn.execute("INSERT INTO notes (id,title,body,created_at,updated_at) VALUES ('n1','Setup','Keep me',0,0)", []).expect("note");
        migrate(&mut conn).expect("upgrade");
        let (name, project, body): (String,String,String) = conn.query_row("SELECT p.name,t.project_id,n.body FROM projects p,tasks t,notes n WHERE p.id='p1' AND t.id='t1' AND n.id='n1'", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).expect("preserved");
        assert_eq!(
            (name.as_str(), project.as_str(), body.as_str()),
            ("Existing", "p1", "Keep me")
        );
        let count: i64 = conn
            .query_row("SELECT count(*) FROM developer_projects", [], |r| r.get(0))
            .expect("table");
        assert_eq!(count, 0);
    }

    #[test]
    fn upgrading_a_v1_database_keeps_data_and_backfills_tags() {
        let mut conn = Connection::open_in_memory().expect("db");
        Migrations::new(vec![M::up(include_str!("../migrations/0001_init.sql"))])
            .to_latest(&mut conn)
            .expect("v1");
        conn.execute(
            "INSERT INTO notes (id, title, body, created_at, updated_at) VALUES ('n1', 'Car', 'Service #car #Ideas', 0, 0)",
            [],
        )
        .expect("insert");
        conn.execute(
            "INSERT INTO tasks (id, title, status, created_at, updated_at) VALUES ('t1', 'Old task', 'todo', 0, 0)",
            [],
        )
        .expect("insert");
        migrate(&mut conn).expect("upgrade");
        let tags: Vec<String> = conn
            .prepare("SELECT tag FROM note_tags ORDER BY tag")
            .expect("prep")
            .query_map([], |r| r.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        assert_eq!(tags, ["car", "ideas"]);
        let (title, rec): (String, Option<String>) = conn
            .query_row(
                "SELECT title, recurrence FROM tasks WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("task");
        assert_eq!((title.as_str(), rec), ("Old task", None));
    }
}
