-- v10: focus / pomodoro sessions, optionally linked to a task.

CREATE TABLE focus_sessions (
    id           TEXT PRIMARY KEY NOT NULL,
    task_id      TEXT REFERENCES tasks (id) ON DELETE SET NULL,
    label        TEXT NOT NULL DEFAULT '',
    kind         TEXT NOT NULL,
    planned_secs INTEGER NOT NULL CHECK (planned_secs > 0),
    actual_secs  INTEGER NOT NULL CHECK (actual_secs >= 0),
    started_at   INTEGER NOT NULL,
    ended_at     INTEGER NOT NULL,
    completed    INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX focus_sessions_ended_at ON focus_sessions (ended_at DESC);
CREATE INDEX focus_sessions_task_id ON focus_sessions (task_id) WHERE task_id IS NOT NULL;
