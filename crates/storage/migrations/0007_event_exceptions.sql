-- Per-occurrence changes to repeating events ("this event only"): skipped
-- or moved occurrences, keyed by series date. Encoded by
-- ottero_core::event::exceptions_to_code; NULL when there are none.

ALTER TABLE events ADD COLUMN exceptions TEXT;
