# ottero-storage

SQLite persistence. One `Store` owns one connection.

## Rules

- Migrations in `migrations/` are append-only. Add `000N_name.sql`, append it to
  `MIGRATIONS` in `src/schema.rs`, and add a test that opens a v(N-1) database.
- Timestamps: INTEGER milliseconds UTC. Dates: TEXT `YYYY-MM-DD`. IDs: UUID v7 text.
- Every row has `created_at`, `updated_at`, and nullable `deleted_at` (soft delete),
  so sync can be added later without a rewrite.
- Multi-table writes run in one transaction (`save_note` updates links atomically).
- User search input goes through `codec::fts_query`, never straight into MATCH.
- Always use bound parameters. Never `format!` user data into SQL.
- Decoding failures return `StorageError::Corrupt(column)`, never panic, never
  include the stored value.
- List queries return lightweight summaries (`NoteSummary`); load full bodies by ID.

## Known limits

- `notes_fts` is an external-content index keyed by SQLite's implicit `rowid`.
  Never run `VACUUM` without `INSERT INTO notes_fts(notes_fts) VALUES('rebuild')` after it.
- `lower()` is ASCII-only, so title matching (`find_note_by_title`,
  `suggest_note_titles`) folds case in Rust. Fine for thousands of notes; add a
  normalized `title_key` column if it ever shows up in profiles.

## Test

`cargo test -p ottero-storage` (in-memory databases; one test uses a temp file).
