//! The web UI's custom games: list, add or change (for every device), take a device's proposal
//! over, remove.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use spielstand_protocol::{CustomGame, CustomGameRow};

use crate::{
    app::AppState,
    auth::Admin,
    custom_games::{self, By},
    error::ApiResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/custom-games", get(list).put(put))
        .route("/api/custom-games/{name}/take-over", post(take_over))
        .route("/api/custom-games/{name}", delete(remove))
}

async fn list(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<CustomGameRow>>> {
    Ok(Json(custom_games::list(&state).await?))
}

async fn put(
    State(state): State<AppState>,
    _admin: Admin,
    Json(game): Json<CustomGame>,
) -> ApiResult<Json<CustomGame>> {
    Ok(Json(custom_games::put(&state, By::Admin, &game).await?))
}

async fn take_over(State(state): State<AppState>, _admin: Admin, Path(name): Path<String>) -> ApiResult<()> {
    custom_games::take_over(&state, &name).await
}

async fn remove(State(state): State<AppState>, _admin: Admin, Path(name): Path<String>) -> ApiResult<()> {
    custom_games::remove(&state, By::Admin, &name).await
}
