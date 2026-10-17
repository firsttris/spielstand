-- Who moves the saves (Spielstand or another tool), where each device keeps them, and the
-- alerts for saves that another tool replaced with older ones.

ALTER TABLE versions ADD COLUMN prefix TEXT;

CREATE TABLE server_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Games whose mode differs from the default ('sync' or 'backup').
CREATE TABLE game_modes (
    game TEXT PRIMARY KEY,
    mode TEXT NOT NULL
);

ALTER TABLE devices ADD COLUMN game_folders TEXT NOT NULL DEFAULT '[]';
-- The device's last report (JSON DeviceReport) and when it came.
ALTER TABLE devices ADD COLUMN report TEXT;
ALTER TABLE devices ADD COLUMN report_at TEXT;

CREATE TABLE prefix_choices (
    device TEXT NOT NULL REFERENCES devices(id),
    game TEXT NOT NULL,
    prefix TEXT NOT NULL,
    PRIMARY KEY (device, game)
);

CREATE TABLE alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    game TEXT NOT NULL,
    device TEXT NOT NULL REFERENCES devices(id),
    lost TEXT NOT NULL,
    found TEXT NOT NULL,
    created_at TEXT NOT NULL,
    dismissed_at TEXT
);
CREATE INDEX alerts_open ON alerts(game) WHERE dismissed_at IS NULL;
