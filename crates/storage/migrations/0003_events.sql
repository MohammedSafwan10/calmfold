-- v3: calendar events.
-- `start_at`/`end_at` are local civil date-times as fixed-width ISO text
-- (`YYYY-MM-DDTHH:MM:SS`), so string order is time order. All-day events use
-- midnights with an exclusive end (see core::event).

CREATE TABLE events (
    id          TEXT PRIMARY KEY NOT NULL,
    title       TEXT NOT NULL,
    notes       TEXT NOT NULL DEFAULT '',
    location    TEXT NOT NULL DEFAULT '',
    all_day     INTEGER NOT NULL CHECK (all_day IN (0, 1)),
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL CHECK (end_at > start_at),
    recurrence  TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    deleted_at  INTEGER
);

CREATE INDEX events_range ON events (deleted_at, start_at, end_at);
