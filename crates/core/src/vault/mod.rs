//! The password vault's domain: items, the lock state machine, generators,
//! one-time codes, password health and import.
//!
//! Plaintext lives only here and in memory while the vault is unlocked. An
//! item is serialized with [`VaultItem::to_json`] and sealed by the app with
//! the vault key (see `docs/decisions/0005-vault-dependencies.md`); storage
//! only ever sees ciphertext. Secret-bearing types zero their memory on drop
//! and print `[redacted]` in `Debug`.

pub mod generator;
pub mod health;
pub mod import;
pub mod lock;
pub mod totp;

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::{DomainError, DomainResult};
use crate::id::VaultItemId;
use crate::text::normalize_title;

/// Field limits (keep one paste from bloating the vault).
pub const MAX_FIELD_CHARS: usize = 4_096;
pub const MAX_NOTES_BYTES: usize = 64 * 1024;
pub const MAX_URLS: usize = 20;
pub const MAX_TAGS: usize = 20;
/// Previous passwords kept per item.
pub const PASSWORD_HISTORY: usize = 10;
/// Serialized format version (bump with a migration in `from_json`).
const FORMAT: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    #[default]
    Login,
    Card,
    Note,
}

impl ItemKind {
    pub const ALL: [Self; 3] = [Self::Login, Self::Card, Self::Note];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Card => "Card",
            Self::Note => "Secure note",
        }
    }

    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            Self::Login => "Logins",
            Self::Card => "Cards",
            Self::Note => "Secure notes",
        }
    }
}

/// What the user edits. Every string is secret: a stored item zeroes its
/// fields on drop ([`VaultItem`] is `ZeroizeOnDrop`); editors call
/// `zeroize()` on drafts they throw away. (Not `ZeroizeOnDrop` itself so
/// drafts can use struct-update syntax.)
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Zeroize)]
#[serde(default)]
pub struct ItemFields {
    #[zeroize(skip)]
    pub kind: ItemKind,
    pub title: String,
    // Login
    pub username: String,
    pub password: String,
    pub urls: Vec<String>,
    /// `otpauth://` link or a base32 secret.
    pub totp: String,
    // Card
    pub cardholder: String,
    pub number: String,
    /// `MM/YY`.
    pub expiry: String,
    pub cvv: String,
    pub pin: String,
    // All kinds
    pub notes: String,
    pub tags: Vec<String>,
}

impl fmt::Debug for ItemFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ItemFields")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl ItemFields {
    #[must_use]
    pub fn new(kind: ItemKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    /// Trims and checks every field. URLs and tags drop blanks and duplicates.
    fn normalized(&self) -> DomainResult<Self> {
        let mut f = self.clone();
        f.title = normalize_title(&f.title)?;
        for field in [
            &mut f.username,
            &mut f.totp,
            &mut f.cardholder,
            &mut f.expiry,
            &mut f.pin,
        ] {
            let trimmed = field.trim().to_owned();
            *field = trimmed;
        }
        // Card numbers and CVVs keep only digits and spaces the user typed.
        f.number = f.number.split_whitespace().collect::<Vec<_>>().join(" ");
        f.cvv = f.cvv.trim().to_owned();
        let too_long = [
            &f.username,
            &f.password,
            &f.totp,
            &f.cardholder,
            &f.number,
            &f.expiry,
            &f.cvv,
            &f.pin,
        ]
        .iter()
        .any(|s| s.chars().count() > MAX_FIELD_CHARS);
        if too_long {
            return Err(DomainError::TitleTooLong {
                max: MAX_FIELD_CHARS,
            });
        }
        if f.notes.len() > MAX_NOTES_BYTES {
            return Err(DomainError::BodyTooLarge {
                max_bytes: MAX_NOTES_BYTES,
            });
        }
        f.urls = dedup(f.urls.iter().map(|u| u.trim().to_owned()), MAX_URLS);
        f.tags = dedup(
            f.tags
                .iter()
                .map(|t| t.trim().trim_start_matches('#').to_lowercase()),
            MAX_TAGS,
        );
        if !f.totp.is_empty() && totp::parse(&f.totp).is_err() {
            return Err(DomainError::InvalidOneTimeCode);
        }
        Ok(f)
    }
}

fn dedup(values: impl Iterator<Item = String>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if !v.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&v)) {
            out.push(v);
        }
    }
    out.truncate(max);
    out
}

/// A password the item used before, kept so a change can be undone.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct PastPassword {
    pub password: String,
    /// When it was replaced (Unix milliseconds).
    #[zeroize(skip)]
    pub replaced_at: i64,
}

impl fmt::Debug for PastPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PastPassword([redacted])")
    }
}

/// One vault item. State machine: `Active ──trash──▶ Trashed ──restore──▶ Active`;
/// trashed items can't be edited.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct VaultItem {
    #[zeroize(skip)]
    v: u8,
    #[zeroize(skip)]
    id: VaultItemId,
    fields: ItemFields,
    #[zeroize(skip)]
    favorite: bool,
    history: Vec<PastPassword>,
    /// Unix milliseconds.
    #[zeroize(skip)]
    created_at: i64,
    #[zeroize(skip)]
    updated_at: i64,
    /// When the password last changed (for "old password" health).
    #[zeroize(skip)]
    password_changed_at: i64,
    #[zeroize(skip)]
    trashed_at: Option<i64>,
}

impl fmt::Debug for VaultItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultItem")
            .field("id", &self.id)
            .field("kind", &self.fields.kind)
            .finish_non_exhaustive()
    }
}

fn ms(t: Timestamp) -> i64 {
    t.as_millisecond()
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

impl VaultItem {
    pub fn new(fields: &ItemFields, now: Timestamp) -> DomainResult<Self> {
        Ok(Self {
            v: FORMAT,
            id: VaultItemId::new(),
            fields: fields.normalized()?,
            favorite: false,
            history: Vec::new(),
            created_at: ms(now),
            updated_at: ms(now),
            password_changed_at: ms(now),
            trashed_at: None,
        })
    }

    #[must_use]
    pub fn id(&self) -> VaultItemId {
        self.id
    }
    #[must_use]
    pub fn fields(&self) -> &ItemFields {
        &self.fields
    }
    #[must_use]
    pub fn kind(&self) -> ItemKind {
        self.fields.kind
    }
    #[must_use]
    pub fn title(&self) -> &str {
        &self.fields.title
    }
    #[must_use]
    pub fn is_favorite(&self) -> bool {
        self.favorite
    }
    #[must_use]
    pub fn history(&self) -> &[PastPassword] {
        &self.history
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        ts(self.created_at)
    }
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        ts(self.updated_at)
    }
    #[must_use]
    pub fn password_changed_at(&self) -> Timestamp {
        ts(self.password_changed_at)
    }
    #[must_use]
    pub fn trashed_at(&self) -> Option<Timestamp> {
        self.trashed_at.map(ts)
    }
    #[must_use]
    pub fn is_trashed(&self) -> bool {
        self.trashed_at.is_some()
    }

    fn editable(&self) -> DomainResult<()> {
        if self.is_trashed() {
            return Err(DomainError::Deleted);
        }
        Ok(())
    }

    /// Replaces the fields. A changed password goes to the history first.
    /// Returns whether anything changed.
    pub fn update(&mut self, fields: &ItemFields, now: Timestamp) -> DomainResult<bool> {
        self.editable()?;
        let next = fields.normalized()?;
        if next == self.fields {
            return Ok(false);
        }
        if next.password != self.fields.password {
            if !self.fields.password.is_empty() {
                self.history.insert(
                    0,
                    PastPassword {
                        password: self.fields.password.clone(),
                        replaced_at: ms(now),
                    },
                );
                self.history.truncate(PASSWORD_HISTORY);
            }
            self.password_changed_at = ms(now);
        }
        self.fields = next;
        self.updated_at = ms(now);
        Ok(true)
    }

    pub fn set_favorite(&mut self, favorite: bool, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        if self.favorite != favorite {
            self.favorite = favorite;
            self.updated_at = ms(now);
        }
        Ok(())
    }

    pub fn trash(&mut self, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.trashed_at = Some(ms(now));
        self.updated_at = ms(now);
        Ok(())
    }

    pub fn restore(&mut self, now: Timestamp) {
        if self.trashed_at.take().is_some() {
            self.updated_at = ms(now);
        }
    }

    /// A copy under a new ID ("Duplicate"), titled "… copy".
    pub fn duplicate(&self, now: Timestamp) -> DomainResult<Self> {
        let mut fields = self.fields.clone();
        fields.title = format!("{} copy", self.fields.title);
        let mut copy = Self::new(&fields, now)?;
        copy.favorite = self.favorite;
        Ok(copy)
    }

    /// Plaintext JSON, for sealing. Zeroed when dropped.
    pub fn to_json(&self) -> DomainResult<Zeroizing<Vec<u8>>> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|_| DomainError::Corrupt)
    }

    /// Reads an item opened from storage. `id` is the row's ID (also the
    /// AAD it was sealed with); a mismatch means the blob was moved.
    pub fn from_json(json: &[u8], id: VaultItemId) -> DomainResult<Self> {
        let item: Self = serde_json::from_slice(json).map_err(|_| DomainError::Corrupt)?;
        if item.id != id || item.v == 0 || item.v > FORMAT {
            return Err(DomainError::Corrupt);
        }
        Ok(item)
    }

    /// The second line in lists. Never reveals a password, a full card
    /// number or a note's text.
    #[must_use]
    pub fn subtitle(&self) -> String {
        let f = &self.fields;
        match f.kind {
            ItemKind::Login if !f.username.is_empty() => f.username.clone(),
            ItemKind::Login => f.urls.first().map(|u| host(u)).unwrap_or_default(),
            ItemKind::Card => {
                let digits: String = f.number.chars().filter(char::is_ascii_digit).collect();
                let last4 = digits
                    .get(digits.len().saturating_sub(4)..)
                    .unwrap_or_default();
                match (card_brand(&digits), last4.is_empty()) {
                    (_, true) => f.cardholder.clone(),
                    (Some(brand), false) => format!("{brand} •••• {last4}"),
                    (None, false) => format!("•••• {last4}"),
                }
            }
            ItemKind::Note => "Secure note".to_owned(),
        }
    }

    /// Search score for `query` (higher is better), or `None` if it doesn't
    /// match. Looks at the title, username, URLs and tags, never at secrets.
    #[must_use]
    pub fn matches(&self, query: &str) -> Option<u8> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Some(1);
        }
        let title = self.fields.title.to_lowercase();
        if title.starts_with(&q) {
            return Some(4);
        }
        if title.split_whitespace().any(|w| w.starts_with(&q)) {
            return Some(3);
        }
        if title.contains(&q) {
            return Some(2);
        }
        let f = &self.fields;
        let other = f.username.to_lowercase().contains(&q)
            || f.urls.iter().any(|u| host(u).to_lowercase().contains(&q))
            || f.tags
                .iter()
                .any(|t| t.starts_with(q.trim_start_matches('#')))
            || f.cardholder.to_lowercase().contains(&q);
        other.then_some(1)
    }
}

/// `https://accounts.google.com/login?x` → `accounts.google.com`.
#[must_use]
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    host.trim_start_matches("www.").to_owned()
}

/// A URL that can be opened in the browser: only http(s), with a host.
#[must_use]
pub fn openable_url(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_lowercase();
    let full = if lower.starts_with("https://") || lower.starts_with("http://") {
        url.to_owned()
    } else if url.contains("://") {
        return None;
    } else {
        // A bare host ("example.com/login"); anything with its own scheme
        // ("javascript:", "mailto:") is not a site.
        let head = url.split(['/', '?', '#']).next().unwrap_or(url);
        if head
            .split(':')
            .nth(1)
            .is_some_and(|p| !p.chars().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        format!("https://{url}")
    };
    let h = host(&full);
    let name = h.split(':').next().unwrap_or(&h);
    let valid = !name.is_empty()
        && (name.contains('.') || name == "localhost")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(full)
}

/// Visa / Mastercard / Amex / Discover from the leading digits.
#[must_use]
pub fn card_brand(digits: &str) -> Option<&'static str> {
    let two: u32 = digits.get(..2)?.parse().ok()?;
    let four: u32 = digits.get(..4).and_then(|d| d.parse().ok()).unwrap_or(0);
    match two {
        _ if digits.starts_with('4') => Some("Visa"),
        51..=55 => Some("Mastercard"),
        22..=27 if (2221..=2720).contains(&four) => Some("Mastercard"),
        34 | 37 => Some("Amex"),
        _ if digits.starts_with("6011") || digits.starts_with("65") => Some("Discover"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::from_second(1_700_000_000).expect("ts")
    }

    fn login(title: &str, user: &str, pass: &str) -> ItemFields {
        ItemFields {
            title: title.into(),
            username: user.into(),
            password: pass.into(),
            urls: vec!["https://github.com/login".into()],
            ..ItemFields::new(ItemKind::Login)
        }
    }

    #[test]
    fn items_normalize_and_keep_password_history() {
        let mut f = login("  GitHub ", "sam", "old-pass");
        f.tags = vec!["#Work".into(), "work".into(), " ".into()];
        f.urls.push("https://github.com/login".into());
        let mut item = VaultItem::new(&f, now()).expect("item");
        assert_eq!(item.title(), "GitHub");
        assert_eq!(item.fields().tags, ["work"]);
        assert_eq!(item.fields().urls.len(), 1);

        let mut next = item.fields().clone();
        next.password = "new-pass".into();
        let later = Timestamp::from_second(1_700_000_100).expect("ts");
        assert!(item.update(&next, later).expect("update"));
        assert_eq!(item.history().len(), 1);
        assert_eq!(item.history()[0].password, "old-pass");
        assert_eq!(item.password_changed_at(), later);
        assert!(!item.update(&next, later).expect("same"), "no-op edit");
        assert!(
            VaultItem::new(&login(" ", "", ""), now()).is_err(),
            "title required"
        );
    }

    #[test]
    fn json_round_trips_and_rejects_moved_blobs() {
        let item = VaultItem::new(&login("GitHub", "sam", "pw"), now()).expect("item");
        let json = item.to_json().expect("json");
        assert_eq!(VaultItem::from_json(&json, item.id()).expect("read"), item);
        assert_eq!(
            VaultItem::from_json(&json, VaultItemId::new()).err(),
            Some(DomainError::Corrupt)
        );
        assert!(VaultItem::from_json(b"{not json", item.id()).is_err());
    }

    #[test]
    fn debug_never_prints_secrets() {
        let item = VaultItem::new(&login("GitHub", "sam", "hunter2"), now()).expect("item");
        let printed = format!("{item:?} {:?}", item.fields());
        assert!(!printed.contains("hunter2") && !printed.contains("sam"));
    }

    #[test]
    fn subtitles_never_reveal_secrets() {
        let item = VaultItem::new(&login("GitHub", "sam", "pw"), now()).expect("item");
        assert_eq!(item.subtitle(), "sam");
        let card = ItemFields {
            title: "Visa".into(),
            number: "4111 1111 1111 1234".into(),
            ..ItemFields::new(ItemKind::Card)
        };
        let card = VaultItem::new(&card, now()).expect("card");
        assert_eq!(card.subtitle(), "Visa •••• 1234");
        let note = ItemFields {
            title: "Wi-Fi".into(),
            notes: "secret".into(),
            ..ItemFields::new(ItemKind::Note)
        };
        assert_eq!(
            VaultItem::new(&note, now()).expect("note").subtitle(),
            "Secure note"
        );
    }

    #[test]
    fn search_ranks_title_matches_first() {
        let item = VaultItem::new(&login("GitHub Work", "sam@x.io", "pw"), now()).expect("item");
        assert_eq!(item.matches("git"), Some(4));
        assert_eq!(item.matches("work"), Some(3));
        assert_eq!(item.matches("hub"), Some(2));
        assert_eq!(item.matches("sam@"), Some(1));
        assert_eq!(item.matches("pw"), None, "never searches passwords");
    }

    #[test]
    fn urls_hosts_and_brands() {
        assert_eq!(host("https://www.example.com/a?b"), "example.com");
        assert_eq!(host("user@mail.example.com"), "mail.example.com");
        assert_eq!(
            openable_url("example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(openable_url("javascript:alert(1)"), None);
        assert_eq!(openable_url("file:///etc/passwd"), None);
        assert_eq!(
            openable_url("localhost:8080/x").as_deref(),
            Some("https://localhost:8080/x")
        );
        assert_eq!(openable_url("https://evil\"x.com"), None);
        assert_eq!(card_brand("5500000000000004"), Some("Mastercard"));
        assert_eq!(card_brand("378282246310005"), Some("Amex"));
        assert_eq!(card_brand("1234"), None);
    }

    #[test]
    fn trashed_items_are_frozen_and_bad_totp_is_refused() {
        let mut item = VaultItem::new(&login("A", "", ""), now()).expect("item");
        item.trash(now()).expect("trash");
        assert_eq!(
            item.set_favorite(true, now()).err(),
            Some(DomainError::Deleted)
        );
        item.restore(now());
        assert!(item.set_favorite(true, now()).is_ok());
        let mut f = login("A", "", "");
        f.totp = "not base32 !!".into();
        assert_eq!(
            VaultItem::new(&f, now()).err(),
            Some(DomainError::InvalidOneTimeCode)
        );
    }
}
