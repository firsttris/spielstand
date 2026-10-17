use spielstand_protocol::EventKind;
use std::{path::Path, str::FromStr, time::Duration};

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

pub type Db = SqlitePool;

pub async fn open(path: &Path) -> anyhow::Result<Db> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn now() -> String {
    ts(Utc::now())
}

pub fn parse(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_default()
}

pub fn parse_opt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref().map(parse)
}

pub async fn event(db: &Db, device: Option<&str>, game: Option<&str>, kind: EventKind) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO events (at, device, game, text) VALUES (?, ?, ?, ?)")
        .bind(now())
        .bind(device)
        .bind(game)
        .bind(kind.as_str())
        .execute(db)
        .await?;
    Ok(())
}
