-- The name of a game whose id is not one (Switch games are keyed by title id), as the device
-- that made the version knew it.
ALTER TABLE versions ADD COLUMN title TEXT;
