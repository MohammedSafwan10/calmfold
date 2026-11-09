-- Birthdays and anniversaries: `birthday`, `birthday:1966`,
-- `anniversary:2019` (calmfold_core::Occasion::to_code). NULL for a plain event.

ALTER TABLE events ADD COLUMN occasion TEXT;
