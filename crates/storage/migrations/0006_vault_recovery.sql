-- v6: a recovery key can also unwrap the vault key (ADR 0006).
-- NULL until the user creates one. The key itself is never stored.
ALTER TABLE vault_meta ADD COLUMN recovery_salt BLOB;
ALTER TABLE vault_meta ADD COLUMN recovery_wrapped BLOB;
