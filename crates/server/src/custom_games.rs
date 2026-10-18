//! Custom games: added in the web UI for every device, or proposed by one device (its agent's
//! command line, e.g. driven by an assistant). A proposal is in effect only on that device
//! until it is taken over in the web UI: one device's token can never make the others back up
//! files it chose.

use spielstand_core::custom;
use spielstand_protocol::{CustomGame, CustomGameRow, EventKind};

use crate::{app::AppState, db, error::AppError, modes};

/// Who changes a custom game: the web UI (for every device) or one device (for itself).
#[derive(Clone, Copy)]
pub enum By<'a> {
    Admin,
    Device(&'a str),
}

impl By<'_> {
    fn device(self) -> Option<String> {
        match self {
            By::Admin => None,
            By::Device(d) => Some(d.to_string()),
        }
    }
}

type Row = (String, Option<String>, Option<String>, String);

fn row((definition, proposed_by, proposed_by_name, changed_at): Row) -> Option<CustomGameRow> {
    Some(CustomGameRow {
        game: serde_json::from_str(&definition).ok()?,
        proposed_by,
        proposed_by_name,
        changed_at: db::parse(&changed_at),
    })
}

pub async fn list(state: &AppState) -> anyhow::Result<Vec<CustomGameRow>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT c.definition, c.proposed_by, d.name, c.changed_at FROM custom_games c
         LEFT JOIN devices d ON d.id = c.proposed_by ORDER BY c.name",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().filter_map(row).collect())
}

/// The custom games in effect on `device`, and the names of the ones only it proposed.
pub async fn for_device(state: &AppState, device: &str) -> anyhow::Result<(Vec<CustomGame>, Vec<String>)> {
    let rows = list(state).await?;
    let proposed = rows
        .iter()
        .filter(|r| r.proposed_by.as_deref() == Some(device))
        .map(|r| r.game.name.clone())
        .collect();
    let games = rows
        .into_iter()
        .filter(|r| r.proposed_by.is_none() || r.proposed_by.as_deref() == Some(device))
        .map(|r| r.game)
        .collect();
    Ok((games, proposed))
}

async fn owner(state: &AppState, name: &str) -> anyhow::Result<Option<Option<String>>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT proposed_by FROM custom_games WHERE name = ?")
            .bind(name)
            .fetch_optional(&state.db)
            .await?;
    Ok(row.map(|(p,)| p))
}

/// A device may change only its own proposals; the web UI may change every custom game.
async fn may_change(state: &AppState, by: By<'_>, name: &str) -> Result<(), AppError> {
    let By::Device(device) = by else {
        return Ok(());
    };
    match owner(state, name).await? {
        None => Ok(()),
        Some(Some(owner)) if owner == device => Ok(()),
        Some(None) => Err(AppError::Conflict(
            "this custom game is in effect on every device; change it in the web UI".into(),
        )),
        Some(Some(_)) => Err(AppError::Conflict(
            "another device proposed a custom game of this name".into(),
        )),
    }
}

/// Settings go to every device: a change for every device concerns all of them, and a
/// proposal changes how the web UI shows the proposing device's games.
async fn changed(state: &AppState, by: By<'_>, name: &str) -> anyhow::Result<()> {
    let device = by.device();
    db::event(
        &state.db,
        device.as_deref(),
        Some(name),
        EventKind::CustomGameChanged,
    )
    .await?;
    match device {
        Some(d) => modes::push(state, &d).await,
        None => modes::push_all(state).await,
    }
}

/// Adds or replaces a custom game. From the web UI it is in effect on every device, from a
/// device only there.
pub async fn put(state: &AppState, by: By<'_>, game: &CustomGame) -> Result<CustomGame, AppError> {
    let game = custom::check(game).map_err(|e| AppError::BadRequest(e.to_string()))?;
    may_change(state, by, &game.name).await?;
    sqlx::query(
        "INSERT INTO custom_games (name, definition, proposed_by, changed_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (name) DO UPDATE SET definition = excluded.definition,
             proposed_by = excluded.proposed_by, changed_at = excluded.changed_at",
    )
    .bind(&game.name)
    .bind(serde_json::to_string(&game)?)
    .bind(by.device())
    .bind(db::now())
    .execute(&state.db)
    .await?;
    changed(state, by, &game.name).await?;
    Ok(game)
}

/// Takes a device's proposal over for every device.
pub async fn take_over(state: &AppState, name: &str) -> Result<(), AppError> {
    let done = sqlx::query("UPDATE custom_games SET proposed_by = NULL, changed_at = ? WHERE name = ?")
        .bind(db::now())
        .bind(name)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    changed(state, By::Admin, name).await?;
    Ok(())
}

/// Removes a custom game; its versions stay.
pub async fn remove(state: &AppState, by: By<'_>, name: &str) -> Result<(), AppError> {
    may_change(state, by, name).await?;
    let owner = owner(state, name).await?.ok_or(AppError::NotFound)?;
    sqlx::query("DELETE FROM custom_games WHERE name = ?")
        .bind(name)
        .execute(&state.db)
        .await?;
    // A proposal concerned only its device; a game for every device concerned all.
    let scope = match &owner {
        Some(device) => By::Device(device),
        None => By::Admin,
    };
    changed(state, scope, name).await?;
    Ok(())
}
