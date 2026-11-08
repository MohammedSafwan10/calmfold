# 0002 SQLite with FTS5 for storage and search

Date: 2026-11-08 · Status: accepted

**Decision.** Store notes and tasks in one SQLite database (rusqlite 0.40,
bundled, WAL) with `rusqlite_migration`. Use SQLite's built-in FTS5 for note
search instead of tantivy for now.

**Why.** One file, transactional link updates, zero extra index to keep in
sync, fast enough for tens of thousands of notes. rusqlite is pinned to 0.40
because `rusqlite_migration` 2.6 requires it (only one `libsqlite3-sys` may link).

**Revisit when** search needs fuzzy/typo tolerance or the corpus grows past
~100k notes (then add tantivy behind the same `search_notes` API).

Notes are stored as rows, not `.md` files; Markdown import/export keeps
Obsidian compatibility (plan 02).
