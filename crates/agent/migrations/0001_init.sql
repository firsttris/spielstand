-- Local memory of the agent. The server is the truth; this only remembers what the disk holds
-- and what still has to be uploaded.

CREATE TABLE bases (
    game TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE outbox (
    id TEXT PRIMARY KEY,
    game TEXT NOT NULL,
    meta_json TEXT NOT NULL,
    blob TEXT NOT NULL,
    created_at TEXT NOT NULL
);
