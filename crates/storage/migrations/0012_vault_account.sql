-- v12: the vault key can be wrapped by the Ottero Cloud account key (ADR 0014).
--
-- vault_meta is rebuilt (SQLite can't drop NOT NULL in place) so the
-- master-password columns can be NULL once the account key has taken over.
-- Every row and value is copied unchanged; nothing is re-encrypted.
--
-- Invariants, enforced here so no code path can strand a vault:
-- * the master-password columns are all set or all NULL;
-- * account_email and account_wrapped are both set or both NULL;
-- * a vault always keeps a way in: the master password, or the account key
--   together with a recovery key (ADR 0006).
CREATE TABLE vault_meta_v12 (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    version           INTEGER NOT NULL,
    salt              BLOB,
    kdf_memory        INTEGER,
    kdf_time          INTEGER,
    kdf_lanes         INTEGER,
    wrapped_key       BLOB,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    recovery_salt     BLOB,
    recovery_wrapped  BLOB,
    account_email     TEXT,
    account_wrapped   BLOB,
    CHECK ((salt IS NULL) = (wrapped_key IS NULL)
       AND (kdf_memory IS NULL) = (wrapped_key IS NULL)
       AND (kdf_time IS NULL) = (wrapped_key IS NULL)
       AND (kdf_lanes IS NULL) = (wrapped_key IS NULL)),
    CHECK ((account_email IS NULL) = (account_wrapped IS NULL)),
    CHECK (wrapped_key IS NOT NULL
       OR (account_wrapped IS NOT NULL
           AND recovery_salt IS NOT NULL
           AND recovery_wrapped IS NOT NULL))
);

INSERT INTO vault_meta_v12 (id, version, salt, kdf_memory, kdf_time, kdf_lanes, wrapped_key,
                            created_at, updated_at, recovery_salt, recovery_wrapped)
SELECT id, version, salt, kdf_memory, kdf_time, kdf_lanes, wrapped_key,
       created_at, updated_at, recovery_salt, recovery_wrapped
FROM vault_meta;

DROP TABLE vault_meta;
ALTER TABLE vault_meta_v12 RENAME TO vault_meta;
