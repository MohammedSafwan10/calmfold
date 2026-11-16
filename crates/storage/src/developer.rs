//! Developer project metadata. Secrets are stored only in sealed vault items.
use crate::codec::{opt_ts_from_sql, ts_from_sql, ts_to_sql, uuid_from_sql};
use crate::{StorageError, StorageResult, Store};
use calmfold_core::developer::{DeveloperProject, ProjectLink};
use calmfold_core::{Note, NoteId, Project, ProjectId};
use rusqlite::{OptionalExtension, params};

impl Store {
    /// Saves the project and metadata atomically; an optional empty setup note
    /// is inserted in the same transaction so a failure cannot orphan it.
    pub fn save_developer_project(
        &mut self,
        workspace: &DeveloperProject,
        setup_note: Option<&Note>,
    ) -> StorageResult<()> {
        let links = serde_json::to_string(workspace.links())
            .map_err(|_| StorageError::Corrupt("developer links"))?;
        let p = workspace.project();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO projects (id,name,created_at,updated_at) VALUES (?1,?2,?3,?4)
            ON CONFLICT(id) DO UPDATE SET name=excluded.name,updated_at=excluded.updated_at",
            params![
                p.id().to_string(),
                p.name(),
                ts_to_sql(p.created_at()),
                ts_to_sql(p.updated_at())
            ],
        )?;
        if let Some(note) = setup_note {
            // Only a newly created, empty note belongs on this path. Existing
            // note bodies, FTS links and tags always use the note repository.
            if !note.body().is_empty() {
                return Err(StorageError::Corrupt("setup note"));
            }
            tx.execute("INSERT INTO notes (id,title,body,pinned,created_at,updated_at,deleted_at) VALUES (?1,?2,'',0,?3,?4,NULL)",
                params![note.id().to_string(),note.title(),ts_to_sql(note.created_at()),ts_to_sql(note.updated_at())])?;
        }
        tx.execute("INSERT INTO developer_projects (project_id,links,folder,setup_note_id,created_at,updated_at,deleted_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project_id) DO UPDATE SET
            links=excluded.links,folder=excluded.folder,setup_note_id=excluded.setup_note_id,updated_at=excluded.updated_at,deleted_at=excluded.deleted_at",
            params![p.id().to_string(),links,workspace.folder(),workspace.setup_note().map(|id|id.to_string()),ts_to_sql(p.created_at()),ts_to_sql(workspace.updated_at()),workspace.archived_at().map(ts_to_sql)])?;
        tx.commit()?;
        Ok(())
    }
    pub fn developer_projects(&self, archived: bool) -> StorageResult<Vec<DeveloperProject>> {
        let mut stmt = self.conn.prepare("SELECT p.id,p.name,p.created_at,p.updated_at,d.links,d.folder,d.setup_note_id,d.updated_at,d.deleted_at
            FROM developer_projects d JOIN projects p ON p.id=d.project_id WHERE (d.deleted_at IS NOT NULL)=?1 ORDER BY p.name COLLATE NOCASE,p.id")?;
        let rows = stmt.query_map([archived], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<i64>>(8)?,
            ))
        })?;
        rows.map(|r| {
            let (id, name, created, updated, links, folder, note, modified, deleted) = r?;
            let id = ProjectId::from_uuid(uuid_from_sql(&id, "developer project id")?);
            let project = Project::from_parts(
                id,
                name,
                ts_from_sql(created, "project created")?,
                ts_from_sql(updated, "project updated")?,
            );
            let links: Vec<ProjectLink> = serde_json::from_str(&links)
                .map_err(|_| StorageError::Corrupt("developer links"))?;
            let note = note
                .map(|n| uuid_from_sql(&n, "setup note").map(NoteId::from_uuid))
                .transpose()?;
            DeveloperProject::from_parts(
                project,
                links,
                folder,
                note,
                ts_from_sql(modified, "developer updated")?,
                opt_ts_from_sql(deleted, "developer archived")?,
            )
            .map_err(|_| StorageError::Corrupt("developer project"))
        })
        .collect()
    }
    pub fn developer_project(&self, id: ProjectId) -> StorageResult<Option<DeveloperProject>> {
        // Metadata collections are small; reuse the validated decode path.
        let archived: Option<bool> = self
            .conn
            .query_row(
                "SELECT deleted_at IS NOT NULL FROM developer_projects WHERE project_id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        match archived {
            Some(archived) => Ok(self
                .developer_projects(archived)?
                .into_iter()
                .find(|p| p.id() == id)),
            None => Ok(None),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use calmfold_core::{Task, Timestamp};
    #[test]
    fn workspace_reuses_project_and_keeps_notes_tasks_when_archived() {
        let mut s = Store::open_in_memory().expect("store");
        let now = Timestamp::UNIX_EPOCH;
        let p = Project::new("Calmfold", now).expect("project");
        let mut w = DeveloperProject::new(p.clone());
        let note = Note::new("Setup", "", now).expect("note");
        w.set_setup_note(note.id(), now);
        w.update(
            vec![ProjectLink::new("Repo", "https://example.com").expect("link")],
            "/tmp",
            now,
        )
        .expect("details");
        s.save_developer_project(&w, Some(&note)).expect("save");
        assert_eq!(s.developer_project(p.id()).expect("read"), Some(w.clone()));
        let mut task = Task::new("Build", now).expect("task");
        task.set_project(Some(p.id()), now).expect("project");
        s.save_task(&task).expect("task save");
        w.set_archived(true, now);
        s.save_developer_project(&w, None).expect("archive");
        assert!(s.developer_projects(false).expect("active").is_empty());
        assert_eq!(s.developer_projects(true).expect("archived").len(), 1);
        assert!(s.note(note.id()).expect("note").is_some());
        assert_eq!(
            s.task(task.id()).expect("task").expect("exists").project(),
            Some(p.id())
        );
    }
    #[test]
    fn metadata_survives_reopen_and_corruption_errors_hide_stored_values() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("app.sqlite3");
        let mut store = Store::open(&path).expect("store");
        let mut project =
            DeveloperProject::new(Project::new("Website", Timestamp::UNIX_EPOCH).expect("project"));
        project
            .update(
                vec![ProjectLink::new("Repository", "https://example.com/repo").expect("link")],
                "/tmp/project",
                Timestamp::UNIX_EPOCH,
            )
            .expect("update");
        store.save_developer_project(&project, None).expect("save");
        drop(store);
        let store = Store::open(&path).expect("reopen");
        assert_eq!(
            store.developer_project(project.id()).expect("read"),
            Some(project.clone())
        );
        store
            .conn
            .execute(
                "UPDATE developer_projects SET links=?1",
                ["[invalid-sensitive-data]"],
            )
            .expect("corrupt");
        let error = store.developer_projects(false).expect_err("invalid");
        assert!(!error.to_string().contains("sensitive-data"));
    }
}
