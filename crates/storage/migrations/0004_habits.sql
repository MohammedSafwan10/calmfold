-- v4: habits and their daily check-ins.

CREATE TABLE habits (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    schedule    TEXT NOT NULL,             -- core::habit::Schedule code
    target      INTEGER NOT NULL DEFAULT 1 CHECK (target BETWEEN 1 AND 99),
    position    INTEGER NOT NULL DEFAULT 0, -- manual order in the list
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    archived_at INTEGER
);

-- One row per habit and local date (`YYYY-MM-DD`); a zero count is deleted.
CREATE TABLE habit_checkins (
    habit_id  TEXT NOT NULL REFERENCES habits (id) ON DELETE CASCADE,
    day       TEXT NOT NULL,
    count     INTEGER NOT NULL CHECK (count > 0),
    PRIMARY KEY (habit_id, day)
) WITHOUT ROWID;

CREATE INDEX habit_checkins_day ON habit_checkins (day);
