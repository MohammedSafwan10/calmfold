//! Templates: a short "New" menu that still covers common secrets.
//!
//! There are only three item kinds (login, card, secure note). Everything
//! else is a template: a kind plus suggested custom fields. A Wi-Fi password
//! is a secure note with "Network" and "Password" fields, an API key a note
//! with "Key" and "Website", and so on. Adding a template never changes the
//! storage format.

use serde::{Deserialize, Serialize};

use crate::vault::{CustomField, FieldKind, ItemFields, ItemKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Template {
    Login,
    Card,
    Note,
    Wifi,
    License,
    ApiKey,
    SshKey,
    Identity,
    /// A login that only holds a one-time code (no password).
    Authenticator,
}

impl Template {
    /// The main three, shown first in "New".
    pub const PRIMARY: [Self; 3] = [Self::Login, Self::Card, Self::Note];
    /// Under "More" in "New".
    pub const MORE: [Self; 6] = [
        Self::Authenticator,
        Self::Wifi,
        Self::License,
        Self::ApiKey,
        Self::SshKey,
        Self::Identity,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Card => "Card",
            Self::Note => "Secure note",
            Self::Wifi => "Wi-Fi",
            Self::License => "Software licence",
            Self::ApiKey => "API key",
            Self::SshKey => "SSH key",
            Self::Identity => "Identity",
            Self::Authenticator => "Authenticator",
        }
    }

    #[must_use]
    pub fn kind(self) -> ItemKind {
        match self {
            Self::Login | Self::Authenticator => ItemKind::Login,
            Self::Card => ItemKind::Card,
            _ => ItemKind::Note,
        }
    }

    /// Empty fields for a new item from this template.
    #[must_use]
    pub fn fields(self) -> ItemFields {
        let field = |kind: FieldKind, label: &str| CustomField {
            kind,
            label: label.to_owned(),
            value: String::new(),
        };
        let custom = match self {
            Self::Wifi => vec![
                field(FieldKind::Text, "Network name"),
                field(FieldKind::Secret, "Password"),
                field(FieldKind::Text, "Security"),
            ],
            Self::License => vec![
                field(FieldKind::Secret, "Licence key"),
                field(FieldKind::Text, "Licensed to"),
                field(FieldKind::Text, "Version"),
                field(FieldKind::Date, "Purchased"),
            ],
            Self::ApiKey => vec![
                field(FieldKind::Secret, "Key"),
                field(FieldKind::Url, "Website"),
                field(FieldKind::Date, "Expires"),
            ],
            Self::SshKey => vec![
                field(FieldKind::Secret, "Private key"),
                field(FieldKind::Text, "Public key"),
                field(FieldKind::Secret, "Passphrase"),
            ],
            Self::Identity => vec![
                field(FieldKind::Text, "Full name"),
                field(FieldKind::Text, "Email"),
                field(FieldKind::Text, "Phone"),
                field(FieldKind::Text, "Address"),
                field(FieldKind::Date, "Date of birth"),
                field(FieldKind::Secret, "ID number"),
            ],
            _ => Vec::new(),
        };
        ItemFields {
            custom,
            template: Some(self),
            ..ItemFields::new(self.kind())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_map_onto_three_kinds() {
        assert_eq!(Template::Wifi.kind(), ItemKind::Note);
        assert_eq!(Template::Authenticator.kind(), ItemKind::Login);
        let wifi = Template::Wifi.fields();
        assert_eq!(wifi.custom.len(), 3);
        assert_eq!(wifi.custom[1].kind, FieldKind::Secret);
        assert_eq!(wifi.template, Some(Template::Wifi));
        assert!(Template::Login.fields().custom.is_empty());
    }
}
