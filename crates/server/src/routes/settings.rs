//! The web UI's settings: the default mode, a game's mode, a device's game folders and prefix
//! choice, and the alerts about overwritten saves.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use spielstand_protocol::{DeviceSettings, GameModeRequest, PrefixChoice, ServerSettings};

use crate::{
    alerts,
    app::AppState,
    auth::Admin,
    error::{ApiResult, AppError},
    modes,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/games/{game}/mode", put(game_mode))
        .route("/api/games/{game}/prefix", put(prefix_choice))
        .route("/api/devices/{id}/settings", put(device_settings))
        .route("/api/alerts/{id}/dismiss", post(dismiss))
}

async fn get_settings(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<ServerSettings>> {
    Ok(Json(ServerSettings {
        default_mode: modes::default_mode(&state).await?,
    }))
}

async fn put_settings(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<ServerSettings>,
) -> ApiResult<Json<ServerSettings>> {
    modes::set_default_mode(&state, req.default_mode).await?;
    Ok(Json(req))
}

async fn game_mode(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
    Json(req): Json<GameModeRequest>,
) -> ApiResult<()> {
    Ok(modes::set_game_mode(&state, &game, req.mode).await?)
}

async fn known_device(state: &AppState, id: &str) -> ApiResult<()> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM devices WHERE id = ? AND removed_at IS NULL")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    row.map(|_| ()).ok_or(AppError::NotFound)
}

async fn prefix_choice(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
    Json(req): Json<PrefixChoice>,
) -> ApiResult<()> {
    known_device(&state, &req.device).await?;
    if req.prefix.trim().is_empty() {
        return Err(AppError::BadRequest("no prefix".into()));
    }
    Ok(modes::set_prefix_choice(&state, &req.device, &game, req.prefix.trim()).await?)
}

async fn device_settings(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    Json(req): Json<DeviceSettings>,
) -> ApiResult<Json<DeviceSettings>> {
    known_device(&state, &id).await?;
    if req
        .game_folders
        .iter()
        .any(|f| !f.trim().is_empty() && !f.trim().starts_with(['/', '~']))
    {
        return Err(AppError::BadRequest(
            "game folders are absolute paths or start with ~".into(),
        ));
    }
    modes::set_game_folders(&state, &id, &req.game_folders).await?;
    Ok(Json(DeviceSettings {
        game_folders: modes::game_folders(&state, &id).await?,
    }))
}

async fn dismiss(State(state): State<AppState>, _admin: Admin, Path(id): Path<i64>) -> ApiResult<()> {
    if alerts::dismiss(&state, id).await? {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}
