//! Developer workspaces extend existing task projects; they never own secrets.
//! Archiving hides the workspace only. Its tasks, notes and vault items remain.
use crate::{DomainError, DomainResult, NoteId, Project, ProjectId, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLink {
    label: String,
    url: String,
}
impl ProjectLink {
    pub fn new(label: &str, target: &str) -> DomainResult<Self> {
        let label = crate::normalize_title(label)?;
        let url = url::Url::parse(target.trim()).map_err(|_| DomainError::InvalidProjectLink)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || target.len() > 4096
        {
            return Err(DomainError::InvalidProjectLink);
        }
        Ok(Self {
            label,
            url: url.to_string(),
        })
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeveloperProject {
    project: Project,
    links: Vec<ProjectLink>,
    folder: String,
    setup_note: Option<NoteId>,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}
impl DeveloperProject {
    pub fn new(project: Project) -> Self {
        let updated_at = project.updated_at();
        Self {
            project,
            links: vec![],
            folder: String::new(),
            setup_note: None,
            updated_at,
            archived_at: None,
        }
    }
    pub fn from_parts(
        project: Project,
        links: Vec<ProjectLink>,
        folder: String,
        setup_note: Option<NoteId>,
        updated_at: Timestamp,
        archived_at: Option<Timestamp>,
    ) -> DomainResult<Self> {
        let mut this = Self::new(project);
        this.update(links, &folder, updated_at)?;
        this.updated_at = updated_at.max(this.updated_at);
        this.setup_note = setup_note;
        this.archived_at = archived_at;
        Ok(this)
    }
    pub fn update(
        &mut self,
        links: Vec<ProjectLink>,
        folder: &str,
        now: Timestamp,
    ) -> DomainResult<bool> {
        if self.is_archived() {
            return Err(DomainError::Archived);
        }
        if links.len() > 12 || folder.len() > 4096 || folder.contains(['\0', '\n', '\r']) {
            return Err(DomainError::BodyTooLarge { max_bytes: 4096 });
        }
        // Deserialized links are validated too; storage never bypasses URL rules.
        let links = links
            .iter()
            .map(|link| ProjectLink::new(&link.label, &link.url))
            .collect::<DomainResult<Vec<_>>>()?;
        let folder = folder.trim().to_owned();
        if links == self.links && folder == self.folder {
            return Ok(false);
        }
        self.links = links;
        self.folder = folder;
        self.updated_at = now.max(self.updated_at);
        Ok(true)
    }
    pub fn set_setup_note(&mut self, id: NoteId, now: Timestamp) {
        self.setup_note = Some(id);
        self.updated_at = now.max(self.updated_at);
    }
    pub fn set_archived(&mut self, archived: bool, now: Timestamp) {
        self.archived_at = archived.then_some(now);
        self.updated_at = now.max(self.updated_at);
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn id(&self) -> ProjectId {
        self.project.id()
    }
    pub fn links(&self) -> &[ProjectLink] {
        &self.links
    }
    pub fn folder(&self) -> &str {
        &self.folder
    }
    pub fn setup_note(&self) -> Option<NoteId> {
        self.setup_note
    }
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    pub fn archived_at(&self) -> Option<Timestamp> {
        self.archived_at
    }
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_reject_commands_credentials_and_invalid_schemes() {
        for target in [
            "javascript:alert(1)",
            "file:///tmp",
            "https://user:password@example.com",
            "not a link",
            "ssh://example.com",
        ] {
            let e = ProjectLink::new("Repo", target).expect_err("reject");
            assert!(!e.to_string().contains("password"));
        }
        assert!(ProjectLink::new("Repo", "https://github.com/example/repo").is_ok());
    }
    #[test]
    fn archive_keeps_project_identity_and_prevents_edits() {
        let now = Timestamp::UNIX_EPOCH;
        let project = Project::new("Ottero", now).expect("project");
        let mut w = DeveloperProject::new(project.clone());
        w.set_archived(true, now);
        assert_eq!(w.update(vec![], "/tmp", now), Err(DomainError::Archived));
        w.set_archived(false, now);
        w.update(vec![], "/tmp", now).expect("restore");
        assert_eq!(w.id(), project.id());
        assert_eq!(w.folder(), "/tmp");
        let later = Timestamp::from_second(100).expect("timestamp");
        let empty =
            DeveloperProject::from_parts(project, vec![], String::new(), None, later, Some(later))
                .expect("decode empty metadata");
        assert_eq!(empty.updated_at(), later);
    }
}
