//! Saves that another tool replaced with older ones. Both states are versions; the alert says
//! where the newer one went missing, so it can be put back with one click.

use spielstand_protocol::{Alert, EventKind, OverwriteReport};

use crate::{app::AppState, db, versions};

pub async fn record(state: &AppState, device: &str, report: &OverwriteReport) -> anyhow::Result<()> {
    let lost = versions::meta(&state.db, &report.lost).await?;
    let found = versions::meta(&state.db, &report.found).await?;
    let (Some(lost), Some(found)) = (lost, found) else {
        anyhow::bail!("unknown version");
    };
    anyhow::ensure!(
        lost.game == report.game && found.game == report.game,
        "versions of another game"
    );
    let open: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM alerts WHERE game = ? AND device = ? AND lost = ? AND dismissed_at IS NULL",
    )
    .bind(&report.game)
    .bind(device)
    .bind(&report.lost)
    .fetch_optional(&state.db)
    .await?;
    if open.is_some() {
        return Ok(());
    }
    sqlx::query("INSERT INTO alerts (game, device, lost, found, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&report.game)
        .bind(device)
        .bind(&report.lost)
        .bind(&report.found)
        .bind(db::now())
        .execute(&state.db)
        .await?;
    db::event(
        &state.db,
        Some(device),
        Some(&report.game),
        EventKind::Overwritten,
    )
    .await
}

type AlertRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

pub async fn open(state: &AppState, game: Option<&str>) -> anyhow::Result<Vec<Alert>> {
    let rows: Vec<AlertRow> = sqlx::query_as(
        "SELECT a.id, a.game, a.device, d.name, a.lost, a.found, a.created_at, v.created_at
         FROM alerts a JOIN devices d ON d.id = a.device LEFT JOIN versions v ON v.id = a.lost
         WHERE a.dismissed_at IS NULL AND (? IS NULL OR a.game = ?) ORDER BY a.created_at DESC",
    )
    .bind(game)
    .bind(game)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, game, device_id, device_name, lost, found, created_at, lost_at)| Alert {
                id,
                game,
                device_id,
                device_name,
                lost_version: lost,
                lost_at: db::parse_opt(lost_at),
                found_version: found,
                created_at: db::parse(&created_at),
            },
        )
        .collect())
}

pub async fn dismiss(state: &AppState, id: i64) -> anyhow::Result<bool> {
    let done = sqlx::query("UPDATE alerts SET dismissed_at = ? WHERE id = ? AND dismissed_at IS NULL")
        .bind(db::now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// The device's disk holds `version` now: alerts about that game on that device are settled.
pub async fn settle(state: &AppState, device: &str, game: &str, version: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE alerts SET dismissed_at = ? WHERE device = ? AND game = ? AND lost = ? AND dismissed_at IS NULL",
    )
    .bind(db::now())
    .bind(device)
    .bind(game)
    .bind(version)
    .execute(&state.db)
    .await?;
    Ok(())
}
