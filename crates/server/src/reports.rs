//! What each device last told the server about its games, for the game page.

use spielstand_protocol::{DeviceReport, GameLocation};

use crate::{app::AppState, db};

pub async fn store(state: &AppState, device: &str, report: &DeviceReport) -> anyhow::Result<()> {
    sqlx::query("UPDATE devices SET report = ?, report_at = ? WHERE id = ?")
        .bind(serde_json::to_string(report)?)
        .bind(db::now())
        .bind(device)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn of_device(state: &AppState, device: &str) -> anyhow::Result<Option<DeviceReport>> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT report FROM devices WHERE id = ?")
        .bind(device)
        .fetch_optional(&state.db)
        .await?;
    Ok(row
        .and_then(|(r,)| r)
        .and_then(|json| serde_json::from_str(&json).ok()))
}

/// One row per active device: its version of the game and what it reported about it.
pub async fn locations(
    state: &AppState,
    game: &str,
    head: Option<&str>,
) -> anyhow::Result<Vec<GameLocation>> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT d.id, d.name, d.report, b.version FROM devices d
         LEFT JOIN device_bases b ON b.device = d.id AND b.game = ?
         WHERE d.removed_at IS NULL AND d.token_hash IS NOT NULL ORDER BY d.created_at",
    )
    .bind(game)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, report, version)| {
            let report = report
                .and_then(|json| serde_json::from_str::<DeviceReport>(&json).ok())
                .and_then(|r| r.games.into_iter().find(|g| g.game == game));
            GameLocation {
                online: state.hub.is_online(&id),
                is_current: version.is_some() && version.as_deref() == head,
                device_id: id,
                device_name: name,
                version,
                report,
            }
        })
        .collect())
}
