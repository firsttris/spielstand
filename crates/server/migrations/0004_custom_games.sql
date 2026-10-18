-- Games the manifest lacks, as JSON CustomGame. `proposed_by` is the device that proposed one;
-- such a game is in effect only there until it is taken over for every device (NULL).
CREATE TABLE custom_games (
    name TEXT PRIMARY KEY,
    definition TEXT NOT NULL,
    proposed_by TEXT REFERENCES devices(id),
    changed_at TEXT NOT NULL
);
