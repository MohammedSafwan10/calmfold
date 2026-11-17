//! The unlocked vault in memory, and the crypto glue between the domain
//! (`ottero_core::vault`), the crypto crate and storage.
//!
//! Key hierarchy (ADR 0005): master password → Argon2id → KEK → unwraps the
//! random vault key → seals each item (AAD = item id). The slow step,
//! [`derive`], is a free function so callers run it on a background thread;
//! everything else here is fast and runs on the main thread.
//!
//! State follows `ottero_core::vault::lock::next`. Locking drops the vault
//! key and every decrypted item (both zero their memory on drop).

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use ottero_core::vault::health::{self, Report, Strength};
use ottero_core::vault::lock::{LockEvent, LockState, next};
use ottero_core::vault::search::Query;
use ottero_core::vault::{ItemFields, ItemKind, VaultItem};
use ottero_core::{DomainError, Timestamp, VaultItemId};
use ottero_crypto::{
    CryptoError, KdfParams, Salt, SecretKey, SecretString, Zeroizing, derive_key, open, seal,
};
use ottero_storage::{SealedItem, Store, VaultMeta};

/// Associated data binding the wrapped vault key to its purpose.
// Format identifiers from the Keepstone days, kept through the Calmfold and
// Ottero renames. Never change them: existing vaults and archives are bound
// to these exact bytes.
const KEY_AAD: &[u8] = b"keepstone-vault-key-v1";
/// The same vault key wrapped by the recovery key (ADR 0006).
const RECOVERY_AAD: &[u8] = b"keepstone-vault-recovery-v1";
/// Argon2 cost for recovery keys. The key has 160 random bits, so it needs
/// no stretching for strength; the minimum cost keeps recovery quick.
pub const RECOVERY_KDF: KdfParams = KdfParams::MINIMUM;
const META_VERSION: u32 = 1;

/// Runs Argon2id. Slow by design: call from a background thread.
pub fn derive(password: &SecretString, salt: &Salt, params: KdfParams) -> Result<SecretKey> {
    derive_key(password, salt, params).map_err(|_| anyhow::anyhow!("Couldn't derive the key."))
}

/// What setup and password changes need to derive on the background thread.
#[derive(Clone, Copy)]
pub struct KdfInput {
    pub salt: Salt,
    pub params: KdfParams,
}

impl KdfInput {
    /// A fresh salt with `params` (the app uses `KdfParams::RECOMMENDED`).
    pub fn fresh(params: KdfParams) -> Result<Self> {
        Ok(Self {
            salt: Salt::generate().context("Couldn't get random bytes")?,
            params,
        })
    }
}

/// Item list filter (the sidebar children of Passwords).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VaultFilter {
    #[default]
    All,
    Favorites,
    Kind(ItemKind),
    /// Items with a one-time code (the authenticator view).
    Codes,
    /// Items flagged by password health.
    Health,
    Trash,
}

/// List order when not searching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultSort {
    #[default]
    Name,
    RecentlyUsed,
    RecentlyEdited,
    Created,
}

impl VaultSort {
    pub const ALL: [Self; 4] = [
        Self::Name,
        Self::RecentlyUsed,
        Self::RecentlyEdited,
        Self::Created,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::RecentlyUsed => "Recently used",
            Self::RecentlyEdited => "Recently edited",
            Self::Created => "Date created",
        }
    }
}

pub struct VaultSession {
    state: LockState,
    key: Option<SecretKey>,
    items: Vec<VaultItem>,
    /// Items that failed to decrypt (shown as a warning, never deleted).
    unreadable: usize,
    /// Strength per item, invalidated when the password changes.
    strength: HashMap<VaultItemId, (u64, Strength)>,
}

impl VaultSession {
    /// The state on launch: no header → setup, otherwise locked.
    pub fn load(store: &Store) -> Result<Self> {
        let state = if store.vault_meta()?.is_some() {
            LockState::Locked
        } else {
            LockState::Uninitialized
        };
        Ok(Self {
            state,
            key: None,
            items: Vec::new(),
            unreadable: 0,
            strength: HashMap::new(),
        })
    }

    /// A locked session (used when the header can't be read).
    #[must_use]
    pub fn locked() -> Self {
        Self {
            state: LockState::Locked,
            key: None,
            items: Vec::new(),
            unreadable: 0,
            strength: HashMap::new(),
        }
    }

    #[must_use]
    pub fn state(&self) -> LockState {
        self.state
    }

    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.state == LockState::Unlocked
    }

    #[must_use]
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    fn transition(&mut self, event: LockEvent) -> Result<()> {
        match next(self.state, event) {
            Some(to) => {
                self.state = to;
                Ok(())
            }
            None => Err(DomainError::VaultState {
                state: self.state.label(),
            }
            .into()),
        }
    }

    fn key(&self) -> Result<&SecretKey> {
        self.key.as_ref().ok_or_else(|| {
            DomainError::VaultState {
                state: self.state.label(),
            }
            .into()
        })
    }

    // ---- Setup / unlock / lock ------------------------------------------

    /// Finishes setup with the KEK derived from the new master password and
    /// the one derived from the new recovery key.
    pub fn finish_setup(
        &mut self,
        store: &mut Store,
        input: KdfInput,
        kek: &SecretKey,
        recovery: Option<(KdfInput, &SecretKey)>,
        now: Timestamp,
    ) -> Result<()> {
        if store.vault_meta()?.is_some() {
            bail!("A vault already exists.");
        }
        let vault_key = SecretKey::generate().context("Couldn't get random bytes")?;
        let wrapped =
            seal(kek, vault_key.expose(), KEY_AAD).context("Couldn't encrypt the vault key")?;
        let (recovery_salt, recovery_wrapped) = match recovery {
            Some((input, rkek)) => (
                Some(input.salt.0.to_vec()),
                Some(
                    seal(rkek, vault_key.expose(), RECOVERY_AAD)
                        .context("Couldn't encrypt the vault key")?,
                ),
            ),
            None => (None, None),
        };
        let at = now.as_millisecond();
        store.save_vault_meta(&VaultMeta {
            version: META_VERSION,
            salt: input.salt.0.to_vec(),
            kdf_memory_kib: input.params.memory_kib,
            kdf_iterations: input.params.iterations,
            kdf_parallelism: input.params.parallelism,
            wrapped_key: wrapped,
            created_at: at,
            updated_at: at,
            recovery_salt,
            recovery_wrapped,
        })?;
        self.transition(LockEvent::SetUp)?;
        self.key = Some(vault_key);
        self.items.clear();
        Ok(())
    }

    /// Whether a recovery key was set up for this vault.
    pub fn has_recovery(store: &Store) -> bool {
        store
            .vault_meta()
            .ok()
            .flatten()
            .is_some_and(|m| m.recovery_wrapped.is_some() && m.recovery_salt.is_some())
    }

    /// The recovery key's KDF input (fixed cost: the key itself is random).
    pub fn recovery_kdf(store: &Store) -> Result<KdfInput> {
        let meta = store.vault_meta()?.context("No vault is set up")?;
        let salt: [u8; Salt::LEN] = meta
            .recovery_salt
            .as_deref()
            .context("This vault has no recovery key")?
            .try_into()
            .map_err(|_| DomainError::Corrupt)?;
        Ok(KdfInput {
            salt: Salt(salt),
            params: RECOVERY_KDF,
        })
    }

    /// Replaces the recovery key (the old one stops working). Unlocked only.
    pub fn set_recovery(
        &mut self,
        store: &mut Store,
        input: KdfInput,
        rkek: &SecretKey,
        now: Timestamp,
    ) -> Result<()> {
        let key = self.key()?;
        let mut meta = store.vault_meta()?.context("No vault is set up")?;
        meta.recovery_salt = Some(input.salt.0.to_vec());
        meta.recovery_wrapped =
            Some(seal(rkek, key.expose(), RECOVERY_AAD).context("Couldn't encrypt the vault key")?);
        meta.updated_at = now.as_millisecond();
        store.save_vault_meta(&meta)?;
        Ok(())
    }

    /// Opens the vault with the recovery key's KEK and sets a new master
    /// password. `Ok(false)`: wrong recovery key, still locked.
    pub fn recover(
        &mut self,
        store: &mut Store,
        rkek: Result<SecretKey>,
        new_input: KdfInput,
        new_kek: &SecretKey,
        now: Timestamp,
    ) -> Result<bool> {
        if self.state != LockState::Locked {
            return Err(DomainError::VaultState {
                state: self.state.label(),
            }
            .into());
        }
        let mut meta = store.vault_meta()?.context("No vault is set up")?;
        let opened = rkek
            .ok()
            .and_then(|rkek| open(&rkek, meta.recovery_wrapped.as_deref()?, RECOVERY_AAD).ok());
        let Some(bytes) = opened.filter(|b| b.len() == SecretKey::LEN) else {
            return Ok(false);
        };
        let mut raw = Zeroizing::new([0u8; SecretKey::LEN]);
        raw.copy_from_slice(&bytes);
        let key = SecretKey::from_bytes(&mut raw);
        meta.wrapped_key =
            seal(new_kek, key.expose(), KEY_AAD).context("Couldn't encrypt the vault key")?;
        meta.salt = new_input.salt.0.to_vec();
        meta.kdf_memory_kib = new_input.params.memory_kib;
        meta.kdf_iterations = new_input.params.iterations;
        meta.kdf_parallelism = new_input.params.parallelism;
        meta.updated_at = now.as_millisecond();
        store.save_vault_meta(&meta)?;
        self.transition(LockEvent::StartUnlock)?;
        self.load_items(store, &key)?;
        self.key = Some(key);
        self.transition(LockEvent::Unlocked)?;
        Ok(true)
    }

    /// "Start a new vault": writes the current encrypted vault to `archive`
    /// (still encrypted; opens later with its old password), then empties
    /// the vault so setup runs again. Locked only, and never deletes before
    /// the archive is safely on disk.
    pub fn archive_and_reset(
        &mut self,
        store: &mut Store,
        archive: &std::path::Path,
        now: Timestamp,
    ) -> Result<()> {
        if self.state != LockState::Locked {
            return Err(DomainError::VaultState {
                state: self.state.label(),
            }
            .into());
        }
        write_archive(store, archive, now)?;
        store.reset_vault()?;
        self.transition(LockEvent::Reset)?;
        Ok(())
    }

    /// "Export encrypted backup": the same file format as an archive,
    /// written while unlocked and left in place. It opens with the master
    /// password the vault has now. Returns the number of sealed items.
    pub fn export(&self, store: &Store, path: &std::path::Path, now: Timestamp) -> Result<usize> {
        if self.state != LockState::Unlocked {
            return Err(DomainError::VaultState {
                state: self.state.label(),
            }
            .into());
        }
        write_archive(store, path, now)
    }

    /// Decrypts an archived vault with its old master password's KEK and
    /// returns its live items (to import into the current vault).
    pub fn open_archive(archive: &ArchiveFile, kek: &SecretKey) -> Result<Option<Vec<VaultItem>>> {
        let Ok(bytes) = open(kek, &archive.meta()?.wrapped_key, KEY_AAD) else {
            return Ok(None);
        };
        if bytes.len() != SecretKey::LEN {
            return Ok(None);
        }
        let mut raw = Zeroizing::new([0u8; SecretKey::LEN]);
        raw.copy_from_slice(&bytes);
        let key = SecretKey::from_bytes(&mut raw);
        let mut out = Vec::new();
        for row in archive.sealed()? {
            if let Ok(json) = open(&key, &row.blob, row.id.to_string().as_bytes())
                && let Ok(item) = VaultItem::from_json(&json, row.id)
                && !item.is_trashed()
            {
                out.push(item);
            }
        }
        Ok(Some(out))
    }

    /// Where to derive from for unlocking (moves the vault to `Unlocking`).
    pub fn start_unlock(&mut self, store: &Store) -> Result<KdfInput> {
        let meta = store.vault_meta()?.context("No vault is set up")?;
        let input = kdf_input(&meta)?;
        self.transition(LockEvent::StartUnlock)?;
        Ok(input)
    }

    /// Finishes unlocking with the derived KEK. A wrong password returns
    /// `Ok(false)` and the vault stays locked.
    pub fn finish_unlock(&mut self, store: &Store, kek: Result<SecretKey>) -> Result<bool> {
        if self.state != LockState::Unlocking {
            // Locked again (or quit) while deriving: ignore the stale result.
            return Ok(false);
        }
        let opened = kek.ok().and_then(|kek| {
            let meta = store.vault_meta().ok().flatten()?;
            open(&kek, &meta.wrapped_key, KEY_AAD).ok()
        });
        let Some(bytes) = opened.filter(|b| b.len() == SecretKey::LEN) else {
            self.transition(LockEvent::Failed)?;
            return Ok(false);
        };
        let mut raw = Zeroizing::new([0u8; SecretKey::LEN]);
        raw.copy_from_slice(&bytes);
        let key = SecretKey::from_bytes(&mut raw);
        self.load_items(store, &key)?;
        self.key = Some(key);
        self.transition(LockEvent::Unlocked)?;
        Ok(true)
    }

    fn load_items(&mut self, store: &Store, key: &SecretKey) -> Result<()> {
        let sealed = store.vault_items()?;
        let mut items = Vec::with_capacity(sealed.len());
        let mut unreadable = 0;
        for row in sealed {
            let item = open(key, &row.blob, row.id.to_string().as_bytes())
                .map_err(|_| DomainError::Corrupt)
                .and_then(|json| VaultItem::from_json(&json, row.id));
            match item {
                Ok(item) => items.push(item),
                Err(_) => unreadable += 1,
            }
        }
        sort(&mut items);
        self.items = items;
        self.unreadable = unreadable;
        self.strength.clear();
        Ok(())
    }

    /// Drops the key and every decrypted item.
    pub fn lock(&mut self) {
        if self.transition(LockEvent::Lock).is_ok() {
            self.key = None;
            self.items.clear();
            self.strength.clear();
            self.unreadable = 0;
        }
    }

    /// Checks the current password (the KEK derived from it) and re-wraps
    /// the vault key under `new_kek`. Items are untouched.
    pub fn change_password(
        &mut self,
        store: &mut Store,
        current_kek: &SecretKey,
        new_input: KdfInput,
        new_kek: &SecretKey,
        now: Timestamp,
    ) -> Result<bool> {
        let key = self.key()?;
        let mut meta = store.vault_meta()?.context("No vault is set up")?;
        if open(current_kek, &meta.wrapped_key, KEY_AAD).is_err() {
            return Ok(false);
        }
        meta.wrapped_key =
            seal(new_kek, key.expose(), KEY_AAD).context("Couldn't encrypt the vault key")?;
        meta.salt = new_input.salt.0.to_vec();
        meta.kdf_memory_kib = new_input.params.memory_kib;
        meta.kdf_iterations = new_input.params.iterations;
        meta.kdf_parallelism = new_input.params.parallelism;
        meta.updated_at = now.as_millisecond();
        store.save_vault_meta(&meta)?;
        Ok(true)
    }

    /// The header's KDF input (to verify the current password).
    pub fn current_kdf(store: &Store) -> Result<KdfInput> {
        kdf_input(&store.vault_meta()?.context("No vault is set up")?)
    }

    // ---- Items ------------------------------------------------------------

    #[must_use]
    pub fn items(&self) -> &[VaultItem] {
        &self.items
    }

    #[must_use]
    pub fn item(&self, id: VaultItemId) -> Option<&VaultItem> {
        self.items.iter().find(|i| i.id() == id)
    }

    fn seal_item(&self, item: &VaultItem) -> Result<SealedItem> {
        let json = item.to_json()?;
        let blob = seal(self.key()?, &json, item.id().to_string().as_bytes())
            .map_err(|_: CryptoError| anyhow::anyhow!("Couldn't encrypt the item"))?;
        Ok(SealedItem {
            id: item.id(),
            blob,
            updated_at: item.updated_at().as_millisecond(),
        })
    }

    /// Saves new or changed items in one transaction and updates memory.
    pub fn put(&mut self, store: &mut Store, items: Vec<VaultItem>) -> Result<()> {
        let sealed = items
            .iter()
            .map(|i| self.seal_item(i))
            .collect::<Result<Vec<_>>>()?;
        store
            .save_vault_items(&sealed)
            .context("Couldn't save to the vault")?;
        for item in items {
            self.strength.remove(&item.id());
            match self.items.iter_mut().find(|i| i.id() == item.id()) {
                Some(slot) => *slot = item,
                None => self.items.push(item),
            }
        }
        sort(&mut self.items);
        Ok(())
    }

    pub fn create(
        &mut self,
        store: &mut Store,
        fields: &ItemFields,
        now: Timestamp,
    ) -> Result<VaultItemId> {
        self.key()?;
        let item = VaultItem::new(fields, now)?;
        let id = item.id();
        self.put(store, vec![item])?;
        Ok(id)
    }

    /// Applies `change` to one item and saves it if it changed.
    pub fn update(
        &mut self,
        store: &mut Store,
        id: VaultItemId,
        change: impl FnOnce(&mut VaultItem) -> ottero_core::DomainResult<bool>,
    ) -> Result<()> {
        let mut item = self.item(id).context("That item no longer exists")?.clone();
        if change(&mut item)? {
            self.put(store, vec![item])?;
        }
        Ok(())
    }

    /// Deletes items for good (Trash → Delete, Empty trash).
    pub fn delete(&mut self, store: &mut Store, ids: &[VaultItemId]) -> Result<()> {
        self.key()?;
        store.delete_vault_items(ids)?;
        self.items.retain(|i| !ids.contains(&i.id()));
        Ok(())
    }

    /// Items for `filter` (and optional `tag`) matching `query` (search
    /// language in `core::vault::search`). With text, best matches first;
    /// otherwise in `sort` order.
    pub fn list(
        &mut self,
        filter: VaultFilter,
        tag: Option<&str>,
        query: &str,
        sort: VaultSort,
        now: Timestamp,
    ) -> Vec<VaultItemId> {
        let exact = self.list_with(filter, tag, Query::parse(query), sort, now);
        let q = Query::parse(query);
        if !exact.is_empty() || q.words.is_empty() {
            return exact;
        }
        // Nothing matched exactly: allow one typo per word.
        self.list_with(filter, tag, q.with_typos(), sort, now)
    }

    fn list_with(
        &mut self,
        filter: VaultFilter,
        tag: Option<&str>,
        q: Query,
        sort: VaultSort,
        now: Timestamp,
    ) -> Vec<VaultItemId> {
        let needs_health = filter == VaultFilter::Health || q.weak || q.reused || q.old;
        let report = needs_health.then(|| self.health(now));
        let in_report = |id: VaultItemId, weak: bool, reused: bool, old: bool| {
            report.as_ref().is_some_and(|r| {
                (weak && r.weak.contains(&id))
                    || (reused && r.reused.iter().any(|g| g.contains(&id)))
                    || (old && r.old.contains(&id))
            })
        };
        let mut scored: Vec<(u8, usize, VaultItemId)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| match filter {
                VaultFilter::Trash => i.is_trashed(),
                _ if i.is_trashed() => false,
                VaultFilter::All => true,
                VaultFilter::Favorites => i.is_favorite(),
                VaultFilter::Kind(k) => i.kind() == k,
                VaultFilter::Codes => i.otp_setup().is_some(),
                VaultFilter::Health => in_report(i.id(), true, true, true),
            })
            .filter(|(_, i)| tag.is_none_or(|t| i.fields().tags.iter().any(|have| have == t)))
            .filter(|(_, i)| {
                let (w, r, o) = (q.weak, q.reused, q.old);
                !(w || r || o) || in_report(i.id(), w, r, o)
            })
            .filter_map(|(ix, i)| q.score(i).map(|s| (s, ix, i.id())))
            .collect();
        let key = |ix: usize| -> i64 {
            let i = &self.items[ix];
            match sort {
                VaultSort::Name => 0,
                VaultSort::RecentlyUsed => -i.used_at().unwrap_or(i.created_at()).as_millisecond(),
                VaultSort::RecentlyEdited => -i.updated_at().as_millisecond(),
                VaultSort::Created => -i.created_at().as_millisecond(),
            }
        };
        let by_text = !q.words.is_empty();
        // `items` is kept in title order, so the index breaks ties by name.
        scored.sort_by(|a, b| {
            let rank = if by_text {
                b.0.cmp(&a.0)
            } else {
                std::cmp::Ordering::Equal
            };
            rank.then(key(a.1).cmp(&key(b.1))).then(a.1.cmp(&b.1))
        });
        scored.into_iter().map(|(_, _, id)| id).collect()
    }

    /// Up to `n` live items most recently copied from (for "Recent").
    #[must_use]
    pub fn recent(&self, n: usize) -> Vec<VaultItemId> {
        let mut used: Vec<&VaultItem> = self
            .items
            .iter()
            .filter(|i| !i.is_trashed() && i.used_at().is_some())
            .collect();
        used.sort_by_key(|i| std::cmp::Reverse(i.used_at()));
        used.into_iter().take(n).map(VaultItem::id).collect()
    }

    /// Tags on live items with their counts, most used first.
    #[must_use]
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for i in self.items.iter().filter(|i| !i.is_trashed()) {
            for t in &i.fields().tags {
                *counts.entry(t.as_str()).or_default() += 1;
            }
        }
        let mut out: Vec<(String, usize)> =
            counts.into_iter().map(|(t, n)| (t.to_owned(), n)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Counts per filter, for sidebar badges.
    #[must_use]
    pub fn count(&self, filter: VaultFilter) -> usize {
        self.items
            .iter()
            .filter(|i| match filter {
                VaultFilter::Trash => i.is_trashed(),
                _ if i.is_trashed() => false,
                VaultFilter::All | VaultFilter::Health => true,
                VaultFilter::Favorites => i.is_favorite(),
                VaultFilter::Kind(k) => i.kind() == k,
                VaultFilter::Codes => i.otp_setup().is_some(),
            })
            .count()
    }

    /// Strength of an item's password (cached).
    pub fn strength_of(&mut self, id: VaultItemId) -> Option<Strength> {
        let item = self.items.iter().find(|i| i.id() == id)?;
        let f = item.fields();
        if f.password.is_empty() {
            return None;
        }
        let fingerprint = fingerprint(&f.password);
        if let Some((fp, s)) = self.strength.get(&id)
            && *fp == fingerprint
        {
            return Some(*s);
        }
        let context: Vec<&str> = [f.title.as_str(), f.username.as_str()]
            .into_iter()
            .chain(f.urls.iter().map(String::as_str))
            .collect();
        let s = health::estimate(&f.password, &context).strength;
        self.strength.insert(id, (fingerprint, s));
        Some(s)
    }

    /// Password health across live logins.
    pub fn health(&mut self, now: Timestamp) -> Report {
        let ids: Vec<VaultItemId> = self.items.iter().map(VaultItem::id).collect();
        let mut strengths: HashMap<VaultItemId, Strength> = HashMap::new();
        for id in ids {
            if let Some(s) = self.strength_of(id) {
                strengths.insert(id, s);
            }
        }
        let refs: Vec<&VaultItem> = self.items.iter().collect();
        health::report(&refs, now, &mut |i| {
            strengths
                .get(&i.id())
                .copied()
                .unwrap_or(Strength::VeryStrong)
        })
    }
}

/// An archived vault on disk (`vault-archives/*.otvault`; Calmfold wrote
/// `*.cfvault` and Keepstone `*.ksvault`, same format): the header and
/// sealed items exactly as they were, as hex. Opening it needs the master
/// password the vault had then.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ArchiveFile {
    format: String,
    version: u32,
    archived_at: i64,
    salt: String,
    kdf_memory_kib: u32,
    kdf_iterations: u32,
    kdf_parallelism: u32,
    wrapped_key: String,
    items: Vec<ArchivedItem>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ArchivedItem {
    id: String,
    blob: String,
    updated_at: i64,
}

// The format name predates both renames; changing it would make every
// existing backup and archive unreadable.
const ARCHIVE_FORMAT: &str = "keepstone-vault-archive";

/// File extensions of backups and archives, newest first: `otvault` (Ottero),
/// `cfvault` (Calmfold), `ksvault` (Keepstone). All hold the same format.
pub const ARCHIVE_EXTENSIONS: [&str; 3] = ["otvault", "cfvault", "ksvault"];

/// Writes the vault's header and sealed items to `path` atomically (temp
/// file, fsync, rename) and proves the file reads back. Nothing is
/// decrypted. Returns the number of sealed items written.
fn write_archive(store: &Store, path: &std::path::Path, now: Timestamp) -> Result<usize> {
    let meta = store.vault_meta()?.context("No vault is set up")?;
    let items = store.vault_items()?;
    let file = ArchiveFile::new(&meta, &items, now);
    let json = serde_json::to_vec_pretty(&file).context("Couldn't write the archive")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("Couldn't create the archive folder")?;
    }
    let tmp = path.with_extension("otvault-tmp");
    {
        use std::io::Write as _;
        let mut f = std::fs::File::create(&tmp).context("Couldn't write the archive")?;
        f.write_all(&json).context("Couldn't write the archive")?;
        f.sync_all().context("Couldn't write the archive")?;
    }
    std::fs::rename(&tmp, path).context("Couldn't write the archive")?;
    let back = ArchiveFile::read(path)?;
    if back.items.len() != items.len() {
        bail!("The archive didn't save correctly; nothing was changed.");
    }
    Ok(items.len())
}

impl ArchiveFile {
    fn new(meta: &VaultMeta, items: &[SealedItem], now: Timestamp) -> Self {
        Self {
            format: ARCHIVE_FORMAT.into(),
            version: 1,
            archived_at: now.as_millisecond(),
            salt: hex(&meta.salt),
            kdf_memory_kib: meta.kdf_memory_kib,
            kdf_iterations: meta.kdf_iterations,
            kdf_parallelism: meta.kdf_parallelism,
            wrapped_key: hex(&meta.wrapped_key),
            items: items
                .iter()
                .map(|i| ArchivedItem {
                    id: i.id.to_string(),
                    blob: hex(&i.blob),
                    updated_at: i.updated_at,
                })
                .collect(),
        }
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read(path).context("Couldn't read the archive")?;
        let file: Self =
            serde_json::from_slice(&text).context("That isn't an Ottero vault archive")?;
        if file.format != ARCHIVE_FORMAT || file.version != 1 {
            bail!("That isn't an Ottero vault archive");
        }
        Ok(file)
    }

    #[must_use]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn archived_at(&self) -> Timestamp {
        Timestamp::from_millisecond(self.archived_at).unwrap_or(Timestamp::UNIX_EPOCH)
    }

    pub fn kdf(&self) -> Result<KdfInput> {
        kdf_input(&self.meta()?)
    }

    fn meta(&self) -> Result<VaultMeta> {
        Ok(VaultMeta {
            version: META_VERSION,
            salt: unhex(&self.salt)?,
            kdf_memory_kib: self.kdf_memory_kib,
            kdf_iterations: self.kdf_iterations,
            kdf_parallelism: self.kdf_parallelism,
            wrapped_key: unhex(&self.wrapped_key)?,
            created_at: self.archived_at,
            updated_at: self.archived_at,
            recovery_salt: None,
            recovery_wrapped: None,
        })
    }

    fn sealed(&self) -> Result<Vec<SealedItem>> {
        self.items
            .iter()
            .map(|i| {
                Ok(SealedItem {
                    id: i.id.parse().map_err(|_| DomainError::Corrupt)?,
                    blob: unhex(&i.blob)?,
                    updated_at: i.updated_at,
                })
            })
            .collect()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(DomainError::Corrupt.into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            s.get(i..i + 2)
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .ok_or_else(|| DomainError::Corrupt.into())
        })
        .collect()
}

fn kdf_input(meta: &VaultMeta) -> Result<KdfInput> {
    let salt: [u8; Salt::LEN] = meta
        .salt
        .as_slice()
        .try_into()
        .map_err(|_| DomainError::Corrupt)?;
    let params = KdfParams {
        memory_kib: meta.kdf_memory_kib,
        iterations: meta.kdf_iterations,
        parallelism: meta.kdf_parallelism,
    };
    params.validate().map_err(|_| DomainError::Corrupt)?;
    Ok(KdfInput {
        salt: Salt(salt),
        params,
    })
}

fn sort(items: &mut [VaultItem]) {
    items.sort_by_cached_key(|i| i.title().to_lowercase());
}

/// A non-reversible tag for "has this password changed" (cache key only;
/// never stored or shown).
fn fingerprint(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottero_crypto::SecretString;

    fn now() -> Timestamp {
        Timestamp::from_second(1_800_000_000).expect("ts")
    }

    /// The minimum KDF cost keeps tests fast; the app uses RECOMMENDED.
    fn input() -> KdfInput {
        KdfInput::fresh(KdfParams::MINIMUM).expect("salt")
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    fn unlocked() -> (Store, VaultSession) {
        let mut store = Store::open_in_memory().expect("store");
        let mut v = VaultSession::load(&store).expect("load");
        assert_eq!(v.state(), LockState::Uninitialized);
        let i = input();
        let kek = derive(&pw("correct horse"), &i.salt, i.params).expect("kek");
        v.finish_setup(&mut store, i, &kek, None, now())
            .expect("setup");
        (store, v)
    }

    fn login(title: &str, password: &str) -> ItemFields {
        ItemFields {
            title: title.into(),
            password: password.into(),
            ..ItemFields::new(ItemKind::Login)
        }
    }

    fn unlock(store: &Store, v: &mut VaultSession, password: &str) -> bool {
        let i = v.start_unlock(store).expect("start");
        v.finish_unlock(store, derive(&pw(password), &i.salt, i.params))
            .expect("finish")
    }

    #[test]
    fn setup_lock_unlock_round_trip() {
        let (mut store, mut v) = unlocked();
        let id = v
            .create(&mut store, &login("GitHub", "s3cret!"), now())
            .expect("create");
        v.lock();
        assert_eq!(v.state(), LockState::Locked);
        assert!(v.items().is_empty(), "nothing decrypted while locked");
        assert!(v.create(&mut store, &login("x", ""), now()).is_err());

        assert!(!unlock(&store, &mut v, "wrong"), "wrong password");
        assert_eq!(v.state(), LockState::Locked);
        assert!(unlock(&store, &mut v, "correct horse"));
        assert_eq!(
            v.item(id).map(|i| i.fields().password.clone()).as_deref(),
            Some("s3cret!")
        );

        // A reopened app starts locked.
        let fresh = VaultSession::load(&store).expect("load");
        assert_eq!(fresh.state(), LockState::Locked);
    }

    #[test]
    fn storage_only_ever_sees_ciphertext() {
        let (mut store, mut v) = unlocked();
        v.create(&mut store, &login("GitHub", "hunter2-unique"), now())
            .expect("create");
        for row in store.vault_items().expect("rows") {
            let text = String::from_utf8_lossy(&row.blob);
            assert!(!text.contains("hunter2") && !text.contains("GitHub"));
        }
    }

    #[test]
    fn environment_secrets_survive_encryption_reload_and_backup() {
        use ottero_core::vault::environment::EnvironmentSecrets;
        let (mut store, mut v) = unlocked();
        let source = (0..200)
            .map(|i| format!("K{i}=secret-value-{i}\n"))
            .collect::<String>();
        let fields = ItemFields {
            title: "Project development".into(),
            environment: Some(EnvironmentSecrets::parse(&source).expect("parse")),
            ..ItemFields::new(ItemKind::Note)
        };
        let id = v.create(&mut store, &fields, now()).expect("create");
        for row in store.vault_items().expect("rows") {
            let text = String::from_utf8_lossy(&row.blob);
            assert!(!text.contains("secret-value") && !text.contains("Project development"));
        }
        v.lock();
        assert!(unlock(&store, &mut v, "correct horse"));
        assert_eq!(
            v.item(id).expect("item").fields().environment,
            fields.environment
        );
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("backup.otvault");
        write_archive(&store, &path, now()).expect("backup");
        let archive = ArchiveFile::read(&path).expect("read");
        let k = archive.kdf().expect("kdf");
        let key = derive(&pw("correct horse"), &k.salt, k.params).expect("derive");
        let items = VaultSession::open_archive(&archive, &key)
            .expect("open")
            .expect("items");
        assert_eq!(items[0].fields().environment, fields.environment);
    }

    #[test]
    fn a_blob_moved_to_another_id_is_unreadable_not_fatal() {
        let (mut store, mut v) = unlocked();
        v.create(&mut store, &login("A", "a"), now()).expect("a");
        let rows = store.vault_items().expect("rows");
        // Copy A's ciphertext under a new id: the AAD no longer matches.
        let moved = SealedItem {
            id: VaultItemId::new(),
            ..rows[0].clone()
        };
        store.save_vault_items(&[moved]).expect("save");
        v.lock();
        assert!(unlock(&store, &mut v, "correct horse"));
        assert_eq!(v.items().len(), 1);
        assert_eq!(v.unreadable(), 1);
    }

    #[test]
    fn stale_unlock_results_are_ignored() {
        let (store, mut v) = unlocked();
        v.lock();
        let i = v.start_unlock(&store).expect("start");
        v.lock(); // user locked again while deriving
        let late = v
            .finish_unlock(&store, derive(&pw("correct horse"), &i.salt, i.params))
            .expect("finish");
        assert!(!late);
        assert_eq!(v.state(), LockState::Locked);
    }

    #[test]
    fn changing_the_master_password_keeps_items() {
        let (mut store, mut v) = unlocked();
        let id = v.create(&mut store, &login("A", "pw"), now()).expect("a");
        let current = VaultSession::current_kdf(&store).expect("kdf");
        let wrong = derive(&pw("nope"), &current.salt, current.params).expect("kek");
        let new_input = input();
        let new_kek = derive(&pw("new master"), &new_input.salt, new_input.params).expect("kek");
        assert!(
            !v.change_password(&mut store, &wrong, new_input, &new_kek, now())
                .expect("try")
        );
        let right = derive(&pw("correct horse"), &current.salt, current.params).expect("kek");
        assert!(
            v.change_password(&mut store, &right, new_input, &new_kek, now())
                .expect("change")
        );
        v.lock();
        assert!(
            !unlock(&store, &mut v, "correct horse"),
            "old password no longer works"
        );
        assert!(unlock(&store, &mut v, "new master"));
        assert!(v.item(id).is_some());
    }

    #[test]
    fn filters_search_trash_and_health() {
        let (mut store, mut v) = unlocked();
        let gh = v
            .create(&mut store, &login("GitHub", "same-Pass-word-81!"), now())
            .expect("gh");
        let gl = v
            .create(&mut store, &login("GitLab", "same-Pass-word-81!"), now())
            .expect("gl");
        let weak = v
            .create(&mut store, &login("Router", "123456"), now())
            .expect("weak");
        v.update(&mut store, gh, |i| {
            i.set_favorite(true, now()).map(|()| true)
        })
        .expect("fav");
        v.update(&mut store, weak, |i| i.trash(now()).map(|()| true))
            .expect("trash");

        assert_eq!(
            v.list(VaultFilter::All, None, "", VaultSort::Name, now()),
            [gh, gl]
        );
        assert_eq!(
            v.list(VaultFilter::Favorites, None, "", VaultSort::Name, now()),
            [gh]
        );
        assert_eq!(
            v.list(VaultFilter::Trash, None, "", VaultSort::Name, now()),
            [weak]
        );
        assert_eq!(
            v.list(VaultFilter::All, None, "lab", VaultSort::Name, now()),
            [gl]
        );
        assert_eq!(v.count(VaultFilter::All), 2);
        let report = v.health(now());
        assert_eq!(report.reused.len(), 1, "GitHub and GitLab share one");
        assert!(report.weak.is_empty(), "trashed items aren't checked");
        v.delete(&mut store, &[weak]).expect("delete");
        assert_eq!(store.vault_items().expect("rows").len(), 2);
    }

    #[test]
    fn a_recovery_key_reopens_the_vault_with_a_new_password() {
        let (mut store, mut v) = unlocked();
        let id = v.create(&mut store, &login("A", "pw"), now()).expect("a");
        let rinput = KdfInput::fresh(RECOVERY_KDF).expect("salt");
        let rkey = pw("ABCDABCDABCDABCDABCDABCDABCDABCD");
        let rkek = derive(&rkey, &rinput.salt, rinput.params).expect("kek");
        v.set_recovery(&mut store, rinput, &rkek, now())
            .expect("recovery");
        assert!(VaultSession::has_recovery(&store));
        v.lock();

        let new_input = input();
        let new_kek =
            derive(&pw("brand new master"), &new_input.salt, new_input.params).expect("kek");
        let ri = VaultSession::recovery_kdf(&store).expect("kdf");
        let wrong = derive(&pw("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"), &ri.salt, ri.params);
        assert!(
            !v.recover(&mut store, wrong, new_input, &new_kek, now())
                .expect("try")
        );
        assert_eq!(v.state(), LockState::Locked);
        let right = derive(&rkey, &ri.salt, ri.params);
        assert!(
            v.recover(&mut store, right, new_input, &new_kek, now())
                .expect("recover")
        );
        assert!(v.item(id).is_some(), "items back");
        v.lock();
        assert!(
            !unlock(&store, &mut v, "correct horse"),
            "forgotten password retired"
        );
        assert!(unlock(&store, &mut v, "brand new master"));
    }

    #[test]
    fn starting_over_archives_first_and_the_archive_opens_later() {
        let (mut store, mut v) = unlocked();
        v.create(&mut store, &login("Old bank", "pw-1"), now())
            .expect("a");
        v.lock();
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("vault-archives/old.cfvault");
        v.archive_and_reset(&mut store, &path, now())
            .expect("reset");
        assert_eq!(v.state(), LockState::Uninitialized);
        assert!(store.vault_meta().expect("meta").is_none());
        let text = std::fs::read_to_string(&path).expect("archive");
        assert!(
            !text.contains("Old bank") && !text.contains("pw-1"),
            "still encrypted"
        );

        let archive = ArchiveFile::read(&path).expect("read");
        let k = archive.kdf().expect("kdf");
        let wrong = derive(&pw("nope"), &k.salt, k.params).expect("kek");
        assert!(
            VaultSession::open_archive(&archive, &wrong)
                .expect("open")
                .is_none()
        );
        let right = derive(&pw("correct horse"), &k.salt, k.params).expect("kek");
        let items = VaultSession::open_archive(&archive, &right)
            .expect("open")
            .expect("items");
        assert_eq!(items[0].title(), "Old bank");
        assert!(unhex("abc").is_err());
    }

    #[test]
    fn sort_tags_recent_and_codes() {
        let (mut store, mut v) = unlocked();
        let mut a = login("Alpha", "x");
        a.tags = vec!["work".into()];
        let a = v.create(&mut store, &a, now()).expect("a");
        let mut b = login("Beta", "y");
        b.totp = "JBSWY3DPEHPK3PXP".into();
        let b = v.create(&mut store, &b, now()).expect("b");
        v.update(&mut store, b, |i| {
            i.mark_used(Timestamp::from_second(1_900_000_000).expect("ts"));
            Ok(true)
        })
        .expect("used");
        assert_eq!(
            v.list(VaultFilter::All, Some("work"), "", VaultSort::Name, now()),
            [a]
        );
        assert_eq!(
            v.list(VaultFilter::Codes, None, "", VaultSort::Name, now()),
            [b]
        );
        assert_eq!(
            v.list(VaultFilter::All, None, "", VaultSort::RecentlyUsed, now())[0],
            b
        );
        assert_eq!(v.recent(5), [b]);
        assert_eq!(v.tags(), [("work".to_owned(), 1)]);
        assert_eq!(
            v.list(VaultFilter::All, None, "has:code", VaultSort::Name, now()),
            [b]
        );
    }

    #[test]
    fn large_vaults_load_and_search_quickly() {
        let (mut store, mut v) = unlocked();
        let items: Vec<VaultItem> = (0..2_000)
            .map(|n| {
                VaultItem::new(&login(&format!("Site {n:04}"), &format!("pw-{n}")), now())
                    .expect("item")
            })
            .collect();
        v.put(&mut store, items).expect("bulk");
        v.lock();
        let started = std::time::Instant::now();
        assert!(unlock(&store, &mut v, "correct horse"));
        assert_eq!(v.items().len(), 2_000);
        assert_eq!(
            v.list(VaultFilter::All, None, "site 1999", VaultSort::Name, now())
                .len(),
            1
        );
        assert!(
            started.elapsed().as_secs() < 5,
            "unlock + decrypt 2,000 items"
        );
    }
}
