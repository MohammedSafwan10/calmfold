-- v5: password vault. Ciphertext only (docs/decisions/0005-vault-dependencies.md).

-- One row: how to turn the master password into the key that unwraps the vault key.
CREATE TABLE vault_meta (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    version      INTEGER NOT NULL,
    salt         BLOB NOT NULL,
    kdf_memory   INTEGER NOT NULL,
    kdf_time     INTEGER NOT NULL,
    kdf_lanes    INTEGER NOT NULL,
    wrapped_key  BLOB NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

-- Each item is one sealed JSON blob (AAD = id). Timestamps are for sync ordering.
CREATE TABLE vault_items (
    id          TEXT PRIMARY KEY NOT NULL,
    blob        BLOB NOT NULL,
    updated_at  INTEGER NOT NULL
);
