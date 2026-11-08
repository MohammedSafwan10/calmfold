-- v2: note tags, projects, repeating tasks, subtasks.
-- Existing notes' tags are filled in by a Rust hook (see schema.rs).

CREATE TABLE note_tags (
    note_id  TEXT NOT NULL REFERENCES notes (id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (note_id, tag)
) WITHOUT ROWID;

CREATE INDEX note_tags_tag ON note_tags (tag);

CREATE TABLE projects (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Repeat rule code (see core::recurrence), parent for subtasks, owning project.
ALTER TABLE tasks ADD COLUMN recurrence TEXT;
ALTER TABLE tasks ADD COLUMN parent_id TEXT REFERENCES tasks (id) ON DELETE CASCADE;
ALTER TABLE tasks ADD COLUMN project_id TEXT REFERENCES projects (id) ON DELETE SET NULL;

CREATE INDEX tasks_parent ON tasks (parent_id);
CREATE INDEX tasks_project ON tasks (project_id, deleted_at, status);
