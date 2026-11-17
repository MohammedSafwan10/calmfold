# ottero-core

Pure domain types and rules. No IO, no SQLite, no UI, no global clock (only
`clock.rs` reads the system time; everything else takes `now`).

## Rules

- Fields are private; change state through methods that enforce invariants.
- Methods that change state return `Ok(false)` for no-ops so callers can skip writes.
- `updated_at` never moves backwards (`touch`).
- Trashed items reject edits until restored (`DomainError::Deleted`).
- State machines are documented in the module docs **and** pinned by an
  exhaustive table test (see `task.rs::transition_table_is_exact`). Change both together.
- Storage codes (`TaskStatus::as_str`, `Priority::as_i64`) are stable forever.
- `from_parts` is for storage only; it may repair invariants, never panic.

## Test

`cargo test -p ottero-core`.
