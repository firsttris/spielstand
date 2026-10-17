//! Who moves each game's saves, and the settings every agent gets from the server.

use std::collections::BTreeMap;

use spielstand_protocol::{AgentSettings, SaveMode, ServerMessage};

use crate::{app::AppState, db};

const DEFAULT_MODE: &str = "default_mode";

fn parse(text: &str) -> SaveMode {
    serde_json::from_value(serde_json::Value::String(text.into())).unwrap_or_default()
}

fn name(mode: SaveMode) -> &'static str {
    match mode {
        SaveMode::Sync => "sync",
        SaveMode::Backup => "backup",
    }
}

pub async fn default_mode(state: &AppState) -> anyhow::Result<SaveMode> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM server_settings WHERE key = ?")
        .bind(DEFAULT_MODE)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|(v,)| parse(&v)).unwrap_or_default())
}

pub async fn set_default_mode(state: &AppState, mode: SaveMode) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO server_settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(DEFAULT_MODE)
    .bind(name(mode))
    .execute(&state.db)
    .await?;
    push_all(state).await
}

pub async fn game_modes(state: &AppState) -> anyhow::Result<BTreeMap<String, SaveMode>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT game, mode FROM game_modes")
        .fetch_all(&state.db)
        .await?;
    Ok(rows.into_iter().map(|(g, m)| (g, parse(&m))).collect())
}

/// The mode of one game, and whether it only follows the default.
pub async fn mode_of(state: &AppState, game: &str) -> anyhow::Result<(SaveMode, bool)> {
    match game_modes(state).await?.get(game) {
        Some(mode) => Ok((*mode, false)),
        None => Ok((default_mode(state).await?, true)),
    }
}

/// `None` lets the game follow the default again.
pub async fn set_game_mode(state: &AppState, game: &str, mode: Option<SaveMode>) -> anyhow::Result<()> {
    match mode {
        Some(m) => {
            sqlx::query(
                "INSERT INTO game_modes (game, mode) VALUES (?, ?)
                 ON CONFLICT (game) DO UPDATE SET mode = excluded.mode",
            )
            .bind(game)
            .bind(name(m))
            .execute(&state.db)
            .await?
        }
        None => {
            sqlx::query("DELETE FROM game_modes WHERE game = ?")
                .bind(game)
                .execute(&state.db)
                .await?
        }
    };
    db::event(
        &state.db,
        None,
        Some(game),
        spielstand_protocol::EventKind::ModeChanged,
    )
    .await?;
    push_all(state).await
}

pub async fn game_folders(state: &AppState, device: &str) -> anyhow::Result<Vec<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT game_folders FROM devices WHERE id = ?")
        .bind(device)
        .fetch_optional(&state.db)
        .await?;
    Ok(row
        .and_then(|(json,)| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

pub async fn set_game_folders(state: &AppState, device: &str, folders: &[String]) -> anyhow::Result<()> {
    let clean: Vec<String> = folders
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    sqlx::query("UPDATE devices SET game_folders = ? WHERE id = ?")
        .bind(serde_json::to_string(&clean)?)
        .bind(device)
        .execute(&state.db)
        .await?;
    push(state, device).await
}

pub async fn set_prefix_choice(
    state: &AppState,
    device: &str,
    game: &str,
    prefix: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO prefix_choices (device, game, prefix) VALUES (?, ?, ?)
         ON CONFLICT (device, game) DO UPDATE SET prefix = excluded.prefix",
    )
    .bind(device)
    .bind(game)
    .bind(prefix)
    .execute(&state.db)
    .await?;
    push(state, device).await
}

pub async fn agent_settings(state: &AppState, device: &str) -> anyhow::Result<AgentSettings> {
    let choices: Vec<(String, String)> =
        sqlx::query_as("SELECT game, prefix FROM prefix_choices WHERE device = ?")
            .bind(device)
            .fetch_all(&state.db)
            .await?;
    Ok(AgentSettings {
        default_mode: default_mode(state).await?,
        game_modes: game_modes(state).await?,
        game_folders: game_folders(state, device).await?,
        prefix_choices: choices.into_iter().collect(),
    })
}

async fn push(state: &AppState, device: &str) -> anyhow::Result<()> {
    let settings = agent_settings(state, device).await?;
    state.hub.send(device, ServerMessage::Settings { settings });
    Ok(())
}

async fn push_all(state: &AppState) -> anyhow::Result<()> {
    let devices: Vec<(String,)> = sqlx::query_as("SELECT id FROM devices WHERE removed_at IS NULL")
        .fetch_all(&state.db)
        .await?;
    for (device,) in devices {
        push(state, &device).await?;
    }
    Ok(())
}
