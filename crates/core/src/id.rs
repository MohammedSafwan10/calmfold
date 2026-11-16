use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Defines a typed identifier backed by a UUID v7.
///
/// UUID v7 is time-ordered, so IDs sort by creation time and can be generated
/// on any device without coordination, which sync needs later.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new, time-ordered identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a [`crate::Note`].
    NoteId
);
define_id!(
    /// Identifies a [`crate::Task`].
    TaskId
);
define_id!(
    /// Identifies a [`crate::Project`].
    ProjectId
);
define_id!(
    /// Identifies a [`crate::Event`].
    EventId
);
define_id!(
    /// Identifies a [`crate::Habit`].
    HabitId
);
define_id!(
    /// Identifies a [`crate::FocusSession`].
    FocusSessionId
);
define_id!(
    /// Identifies a [`crate::vault::VaultItem`].
    VaultItemId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_time_ordered() {
        let a = NoteId::new();
        let b = NoteId::new();
        assert_ne!(a, b);
        assert!(a < b, "UUID v7 must sort by creation order");
    }

    #[test]
    fn id_round_trips_through_string_and_json() {
        let id = TaskId::new();
        let parsed: TaskId = id.to_string().parse().expect("valid uuid");
        assert_eq!(id, parsed);

        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, format!("\"{id}\""));
        let back: TaskId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }

    #[test]
    fn invalid_id_string_is_rejected() {
        assert!("not-a-uuid".parse::<NoteId>().is_err());
    }
}
