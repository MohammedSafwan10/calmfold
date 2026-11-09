//! Vault rows. Storage never sees plaintext: it stores the header needed to
//! derive the key and opaque sealed blobs, and doesn't depend on the crypto
//! crate.

use rusqlite::{OptionalExtension, params};

use crate::codec::uuid_from_sql;
use crate::{StorageError, StorageResult, Store};
use keepstone_core::VaultItemId;

/// The vault header (`vault_meta`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultMeta {
    pub version: u32,
    pub salt: Vec<u8>,
    pub kdf_memory_kib: u32,
    pub kdf_iterations: u32,
    pub kdf_parallelism: u32,
    /// The vault key, sealed with the key derived from the master password.
    pub wrapped_key: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One sealed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedItem {
    pub id: VaultItemId,
    pub blob: Vec<u8>,
    pub updated_at: i64,
}

impl Store {
    pub fn vault_meta(&self) -> StorageResult<Option<VaultMeta>> {
        self.conn
            .query_row(
                "SELECT version, salt, kdf_memory, kdf_time, kdf_lanes, wrapped_key, created_at, updated_at
                 FROM vault_meta WHERE id = 1",
                [],
                |r| {
                    Ok(VaultMeta {
                        version: r.get(0)?,
                        salt: r.get(1)?,
                        kdf_memory_kib: r.get(2)?,
                        kdf_iterations: r.get(3)?,
                        kdf_parallelism: r.get(4)?,
                        wrapped_key: r.get(5)?,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// Writes the header (setup, or a master-password change).
    pub fn save_vault_meta(&mut self, m: &VaultMeta) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO vault_meta (id, version, salt, kdf_memory, kdf_time, kdf_lanes, wrapped_key, created_at, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET
                version = excluded.version, salt = excluded.salt,
                kdf_memory = excluded.kdf_memory, kdf_time = excluded.kdf_time,
                kdf_lanes = excluded.kdf_lanes, wrapped_key = excluded.wrapped_key,
                updated_at = excluded.updated_at",
            params![
                m.version,
                m.salt,
                m.kdf_memory_kib,
                m.kdf_iterations,
                m.kdf_parallelism,
                m.wrapped_key,
                m.created_at,
                m.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn vault_items(&self) -> StorageResult<Vec<SealedItem>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, blob, updated_at FROM vault_items ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, blob, updated_at) = row?;
            Ok(SealedItem {
                id: VaultItemId::from_uuid(uuid_from_sql(&id, "vault_items.id")?),
                blob,
                updated_at,
            })
        })
        .collect()
    }

    /// Saves several sealed items in one transaction (an import, say).
    pub fn save_vault_items(&mut self, items: &[SealedItem]) -> StorageResult<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO vault_items (id, blob, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET blob = excluded.blob, updated_at = excluded.updated_at",
            )?;
            for item in items {
                stmt.execute(params![item.id.to_string(), item.blob, item.updated_at])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_vault_items(&mut self, ids: &[VaultItemId]) -> StorageResult<usize> {
        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM vault_items WHERE id = ?1")?;
            for id in ids {
                n += stmt.execute([id.to_string()])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> VaultMeta {
        VaultMeta {
            version: 1,
            salt: vec![1; 16],
            kdf_memory_kib: 65_536,
            kdf_iterations: 3,
            kdf_parallelism: 4,
            wrapped_key: vec![9; 72],
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn meta_and_items_round_trip() {
        let mut store = Store::open_in_memory().expect("store");
        assert_eq!(store.vault_meta().expect("meta"), None);
        store.save_vault_meta(&meta()).expect("save");
        let mut m = meta();
        m.wrapped_key = vec![7; 72];
        m.updated_at = 2;
        store.save_vault_meta(&m).expect("update");
        let loaded = store.vault_meta().expect("meta").expect("some");
        assert_eq!(loaded.wrapped_key, vec![7; 72]);
        assert_eq!(loaded.created_at, 1, "created_at kept");

        let a = SealedItem {
            id: VaultItemId::new(),
            blob: vec![1, 2, 3],
            updated_at: 5,
        };
        let b = SealedItem {
            id: VaultItemId::new(),
            blob: vec![4],
            updated_at: 6,
        };
        store
            .save_vault_items(&[a.clone(), b.clone()])
            .expect("save");
        let mut b2 = b.clone();
        b2.blob = vec![5, 5];
        store.save_vault_items(&[b2.clone()]).expect("update");
        let items = store.vault_items().expect("items");
        assert_eq!(items.len(), 2);
        assert!(items.contains(&b2));
        assert_eq!(store.delete_vault_items(&[a.id]).expect("delete"), 1);
        assert_eq!(store.vault_items().expect("items"), vec![b2]);
    }

    #[test]
    fn only_one_header_row_exists() {
        let store = Store::open_in_memory().expect("store");
        let r = store.conn.execute(
            "INSERT INTO vault_meta (id, version, salt, kdf_memory, kdf_time, kdf_lanes, wrapped_key, created_at, updated_at)
             VALUES (2, 1, x'00', 1, 1, 1, x'00', 0, 0)",
            [],
        );
        assert!(r.is_err());
    }
}
