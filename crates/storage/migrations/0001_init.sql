-- Calmfold schema v1.
-- Timestamps are INTEGER milliseconds since the Unix epoch (UTC).
-- Dates are TEXT 'YYYY-MM-DD'. IDs are UUID v7 strings.
-- Never edit a shipped migration; add a new file instead.

CREATE TABLE notes (
    id          TEXT PRIMARY KEY NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',
    pinned      INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    deleted_at  INTEGER
);

CREATE INDEX notes_listing ON notes (deleted_at, pinned DESC, updated_at DESC);
CREATE INDEX notes_title_nocase ON notes (title COLLATE NOCASE);

-- Full-text index over notes, kept in sync by triggers.
CREATE VIRTUAL TABLE notes_fts USING fts5(
    title,
    body,
    content = 'notes',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;

CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body) VALUES ('delete', old.rowid, old.title, old.body);
END;

CREATE TRIGGER notes_fts_update AFTER UPDATE OF title, body ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body) VALUES ('delete', old.rowid, old.title, old.body);
    INSERT INTO notes_fts (rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;

-- Outgoing [[wiki links]] per note. target_key is the lower-cased, whitespace-normalised target.
CREATE TABLE note_links (
    source_id   TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    target_key  TEXT NOT NULL,
    PRIMARY KEY (source_id, target_key)
) WITHOUT ROWID;

CREATE INDEX note_links_target ON note_links (target_key);

CREATE TABLE tasks (
    id            TEXT PRIMARY KEY NOT NULL,
    title         TEXT NOT NULL,
    notes         TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL CHECK (status IN ('todo', 'in_progress', 'done', 'cancelled')),
    priority      INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
    due           TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    completed_at  INTEGER,
    deleted_at    INTEGER
);

CREATE INDEX tasks_open ON tasks (deleted_at, status, due);
CREATE INDEX tasks_completed ON tasks (deleted_at, completed_at DESC);
