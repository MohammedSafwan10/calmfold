//! Projects group tasks (sidebar: Tasks → project).

use serde::{Deserialize, Serialize};

use crate::{DomainResult, ProjectId, Timestamp, normalize_title};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    id: ProjectId,
    name: String,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl Project {
    pub fn new(name: &str, now: Timestamp) -> DomainResult<Self> {
        Ok(Self {
            id: ProjectId::new(),
            name: normalize_title(name)?,
            created_at: now,
            updated_at: now,
        })
    }

    #[doc(hidden)]
    #[must_use]
    pub fn from_parts(
        id: ProjectId,
        name: String,
        created_at: Timestamp,
        updated_at: Timestamp,
    ) -> Self {
        Self {
            id,
            name,
            created_at,
            updated_at,
        }
    }

    pub fn rename(&mut self, name: &str, now: Timestamp) -> DomainResult<bool> {
        let name = normalize_title(name)?;
        if name == self.name {
            return Ok(false);
        }
        self.name = name;
        self.updated_at = now.max(self.updated_at);
        Ok(true)
    }

    #[must_use]
    pub fn id(&self) -> ProjectId {
        self.id
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainError;

    #[test]
    fn names_are_validated() {
        let now = Timestamp::UNIX_EPOCH;
        assert_eq!(
            Project::new("  ", now).map(|_| ()),
            Err(DomainError::EmptyTitle)
        );
        let mut p = Project::new(" Home  repairs ", now).expect("project");
        assert_eq!(p.name(), "Home repairs");
        assert_eq!(p.rename("Home repairs", now), Ok(false));
        assert_eq!(p.rename("House", now), Ok(true));
    }
}
