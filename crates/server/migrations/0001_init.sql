-- The server is the only writer. Heads move only through compare-and-swap updates.

CREATE TABLE admin (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    hostname TEXT NOT NULL DEFAULT '',
    os TEXT NOT NULL DEFAULT '',
    agent_version TEXT NOT NULL DEFAULT '',
    token_hash TEXT UNIQUE,
    created_at TEXT NOT NULL,
    last_seen TEXT,
    playing_game TEXT,
    playing_since TEXT,
    removed_at TEXT
);

CREATE TABLE pair_codes (
    code TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used_at TEXT
);

CREATE TABLE games (
    id TEXT PRIMARY KEY,
    head TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE versions (
    id TEXT PRIMARY KEY,
    game TEXT NOT NULL REFERENCES games(id),
    parent TEXT,
    device TEXT NOT NULL REFERENCES devices(id),
    created_at TEXT NOT NULL,
    trigger TEXT NOT NULL,
    session_seconds INTEGER,
    content_hash TEXT NOT NULL,
    blob_sha256 TEXT NOT NULL,
    blob_size INTEGER NOT NULL,
    files_json TEXT NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    branch INTEGER NOT NULL DEFAULT 0,
    deleted_at TEXT,
    mirrored_at TEXT
);
CREATE INDEX versions_game ON versions(game, created_at);
CREATE INDEX versions_blob ON versions(blob_sha256);

-- Which version the disk of a device holds, per game.
CREATE TABLE device_bases (
    device TEXT NOT NULL REFERENCES devices(id),
    game TEXT NOT NULL REFERENCES games(id),
    version TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (device, game)
);

-- A branch: uploaded while the head had moved on. Open until someone chooses.
CREATE TABLE conflicts (
    version TEXT PRIMARY KEY REFERENCES versions(id),
    game TEXT NOT NULL,
    device TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    device TEXT,
    game TEXT,
    text TEXT NOT NULL
);
CREATE INDEX events_at ON events(at);
