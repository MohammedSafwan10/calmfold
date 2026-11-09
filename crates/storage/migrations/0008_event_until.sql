-- A repeating event can end: the last date an occurrence may start on
-- (inclusive, `YYYY-MM-DD`). NULL repeats forever.

ALTER TABLE events ADD COLUMN until TEXT;
