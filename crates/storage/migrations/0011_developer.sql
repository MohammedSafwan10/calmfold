-- Developer metadata extends the task project instead of duplicating it.
-- Contains ordinary project context only, never credentials or .env values.
CREATE TABLE developer_projects (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    links TEXT NOT NULL DEFAULT '[]',
    folder TEXT NOT NULL DEFAULT '',
    setup_note_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);
