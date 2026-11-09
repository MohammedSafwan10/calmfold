//! The unlocked vault in memory, and the crypto glue between the domain
//! (`keepstone_core::vault`), the crypto crate and storage.
//!
//! Key hierarchy (ADR 0005): master password → Argon2id → KEK → unwraps the
//! random vault key → seals each item (AAD = item id). The slow step,
//! [`derive`], is a free function so callers run it on a background thread;
//! everything else here is fast and runs on the main thread.
//!
//! State follows `keepstone_core::vault::lock::next`. Locking drops the vault
//! key and every decrypted item (both zero their memory on drop).

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use keepstone_core::vault::health::{self, Report, Strength};
use keepstone_core::vault::lock::{LockEvent, LockState, next};
use keepstone_core::vault::{ItemFields, ItemKind, VaultItem};
use keepstone_core::{DomainError, Timestamp, VaultItemId};
use keepstone_crypto::{
    CryptoError, KdfParams, Salt, SecretKey, SecretString, Zeroizing, derive_key, open, seal,
};
use keepstone_storage::{SealedItem, Store, VaultMeta};

/// Associated data binding the wrapped vault key to its purpose.
const KEY_AAD: &[u8] = b"keepstone-vault-key-v1";
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
    /// Items flagged by password health.
    Health,
    Trash,
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

    /// Finishes setup with the KEK derived from the new master password.
    pub fn finish_setup(
        &mut self,
        store: &mut Store,
        input: KdfInput,
        kek: &SecretKey,
        now: Timestamp,
    ) -> Result<()> {
        if store.vault_meta()?.is_some() {
            bail!("A vault already exists.");
        }
        let vault_key = SecretKey::generate().context("Couldn't get random bytes")?;
        let wrapped =
            seal(kek, vault_key.expose(), KEY_AAD).context("Couldn't encrypt the vault key")?;
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
        })?;
        self.transition(LockEvent::SetUp)?;
        self.key = Some(vault_key);
        self.items.clear();
        Ok(())
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
        change: impl FnOnce(&mut VaultItem) -> keepstone_core::DomainResult<bool>,
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

    /// Items for `filter` matching `query`, best matches first (title order
    /// within a score).
    #[must_use]
    pub fn list(&mut self, filter: VaultFilter, query: &str, now: Timestamp) -> Vec<VaultItemId> {
        let flagged: Vec<VaultItemId> = if filter == VaultFilter::Health {
            self.health(now).flagged()
        } else {
            Vec::new()
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
                VaultFilter::Health => flagged.contains(&i.id()),
            })
            .filter_map(|(ix, i)| i.matches(query).map(|s| (s, ix, i.id())))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, _, id)| id).collect()
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
    use keepstone_crypto::SecretString;

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
        v.finish_setup(&mut store, i, &kek, now()).expect("setup");
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

        assert_eq!(v.list(VaultFilter::All, "", now()), [gh, gl]);
        assert_eq!(v.list(VaultFilter::Favorites, "", now()), [gh]);
        assert_eq!(v.list(VaultFilter::Trash, "", now()), [weak]);
        assert_eq!(v.list(VaultFilter::All, "lab", now()), [gl]);
        assert_eq!(v.count(VaultFilter::All), 2);
        let report = v.health(now());
        assert_eq!(report.reused.len(), 1, "GitHub and GitLab share one");
        assert!(report.weak.is_empty(), "trashed items aren't checked");
        v.delete(&mut store, &[weak]).expect("delete");
        assert_eq!(store.vault_items().expect("rows").len(), 2);
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
        assert_eq!(v.list(VaultFilter::All, "site 1999", now()).len(), 1);
        assert!(
            started.elapsed().as_secs() < 5,
            "unlock + decrypt 2,000 items"
        );
    }
}
