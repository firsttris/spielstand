//! The agent's memory: per game the version the disk holds (`base`), and the outbox of
//! versions that are packed but not yet confirmed by the server.

use std::{path::PathBuf, str::FromStr, time::Duration};

use chrono::{SecondsFormat, Utc};
use spielstand_protocol::VersionMeta;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base {
    pub version: String,
    pub content_hash: String,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub meta: VersionMeta,
    pub blob: PathBuf,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Store {
    pub async fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn base(&self, game: &str) -> anyhow::Result<Option<Base>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT version, content_hash FROM bases WHERE game = ?")
                .bind(game)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(version, content_hash)| Base {
            version,
            content_hash,
        }))
    }

    pub async fn set_base(&self, game: &str, base: &Base) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO bases (game, version, content_hash, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (game) DO UPDATE SET version = excluded.version, content_hash = excluded.content_hash,
             updated_at = excluded.updated_at",
        )
        .bind(game)
        .bind(&base.version)
        .bind(&base.content_hash)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn tracked_games(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT game FROM bases ORDER BY game")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(g,)| g).collect())
    }

    pub async fn enqueue(&self, meta: &VersionMeta, blob: &std::path::Path) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO outbox (id, game, meta_json, blob, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&meta.id)
        .bind(&meta.game)
        .bind(serde_json::to_string(meta)?)
        .bind(blob.to_string_lossy().to_string())
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn pending(&self) -> anyhow::Result<Vec<Pending>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT meta_json, blob FROM outbox ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|(meta, blob)| {
                Ok(Pending {
                    meta: serde_json::from_str(&meta)?,
                    blob: PathBuf::from(blob),
                })
            })
            .collect()
    }

    pub async fn pending_count(&self) -> anyhow::Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    pub async fn confirmed(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM outbox WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
