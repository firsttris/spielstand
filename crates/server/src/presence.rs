//! Who is playing what. A report counts while the agent is connected, or for a few minutes
//! after the connection dropped (a sleeping laptop must not look idle at once).

use spielstand_protocol::EventKind;
use std::collections::BTreeMap;

use chrono::{Duration, Utc};

use crate::{app::AppState, db};

pub const STALE_AFTER_MINUTES: i64 = 5;

pub async fn set_playing(state: &AppState, device: &str, game: Option<&str>) -> anyhow::Result<()> {
    let since = game.map(|_| db::now());
    sqlx::query("UPDATE devices SET playing_game = ?, playing_since = ?, last_seen = ? WHERE id = ?")
        .bind(game)
        .bind(since)
        .bind(db::now())
        .bind(device)
        .execute(&state.db)
        .await?;
    let text = match game {
        Some(_) => EventKind::Started,
        None => EventKind::Stopped,
    };
    db::event(&state.db, Some(device), game, text).await?;
    Ok(())
}

pub fn is_current(state: &AppState, device: &str, last_seen: Option<&str>) -> bool {
    state.hub.is_online(device)
        || last_seen.is_some_and(|s| Utc::now() - db::parse(s) < Duration::minutes(STALE_AFTER_MINUTES))
}

/// Games played right now on devices other than `device`, with the device name.
pub async fn playing_elsewhere(state: &AppState, device: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, playing_game, last_seen FROM devices
         WHERE playing_game IS NOT NULL AND id != ? AND removed_at IS NULL",
    )
    .bind(device)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _, seen)| is_current(state, id, seen.as_deref()))
        .map(|(_, name, game, _)| (game, name))
        .collect())
}
