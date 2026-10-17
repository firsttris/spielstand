//! The web UI's API. Everything but session, setup and login needs the admin session.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
    routing::{delete, get, patch, post},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use chrono::{Duration, Utc};
use rand::Rng;
use spielstand_protocol::EventKind;
use spielstand_protocol::{
    Activity, Device, GameDetail, GameSummary, ImportReport, ImportRequest, Overview, PairCode,
    PasswordRequest, ResolveRequest, RestoreRequest, ServerMessage, SessionInfo, VersionPatch,
};

use crate::{
    app::AppState,
    auth::{self, Admin, SESSION_COOKIE, SESSION_DAYS},
    db,
    error::{ApiResult, AppError},
    importer, modes, queries, versions,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/overview", get(overview))
        .route("/api/games", get(games))
        .route("/api/games/{game}", get(game))
        .route("/api/games/{game}/restore", post(restore))
        .route("/api/games/{game}/resolve", post(resolve))
        .route("/api/games/{game}/backup", post(backup))
        .route("/api/versions/{id}", patch(patch_version))
        .route("/api/versions/{id}/download", get(download))
        .route("/api/devices", get(devices))
        .route("/api/devices/{id}", delete(remove_device))
        .route("/api/pair-codes", post(pair_code))
        .route("/api/activity", get(activity))
        .route("/api/import", post(import))
}

async fn has_admin(state: &AppState) -> anyhow::Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM admin")
        .fetch_optional(&state.db)
        .await?;
    Ok(row.is_some())
}

async fn session(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<SessionInfo>> {
    let logged_in = match jar.get(SESSION_COOKIE) {
        Some(c) => {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT token_hash FROM sessions WHERE token_hash = ? AND expires_at > ?")
                    .bind(auth::token_hash(c.value()))
                    .bind(db::now())
                    .fetch_optional(&state.db)
                    .await?;
            row.is_some()
        }
        None => false,
    };
    Ok(Json(SessionInfo {
        setup_required: !has_admin(&state).await?,
        logged_in,
    }))
}

async fn start_session(state: &AppState, jar: CookieJar) -> ApiResult<CookieJar> {
    let token = auth::random_token();
    sqlx::query("INSERT INTO sessions (token_hash, created_at, expires_at) VALUES (?, ?, ?)")
        .bind(auth::token_hash(&token))
        .bind(db::now())
        .bind(db::ts(Utc::now() + Duration::days(SESSION_DAYS)))
        .execute(&state.db)
        .await?;
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(state.settings.secure_cookies)
        .max_age(time_days(SESSION_DAYS))
        .build();
    Ok(jar.add(cookie))
}

fn time_days(days: i64) -> time::Duration {
    time::Duration::days(days)
}

fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(AppError::BadRequest(
            "Passwort braucht mindestens 8 Zeichen".into(),
        ));
    }
    Ok(())
}

async fn setup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<PasswordRequest>,
) -> ApiResult<(CookieJar, Json<SessionInfo>)> {
    check_password(&req.password)?;
    let inserted = sqlx::query("INSERT OR IGNORE INTO admin (id, password_hash) VALUES (1, ?)")
        .bind(auth::hash_password(&req.password)?)
        .execute(&state.db)
        .await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::Conflict("already set up".into()));
    }
    let jar = start_session(&state, jar).await?;
    Ok((
        jar,
        Json(SessionInfo {
            setup_required: false,
            logged_in: true,
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<PasswordRequest>,
) -> ApiResult<(CookieJar, Json<SessionInfo>)> {
    let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM admin WHERE id = 1")
        .fetch_optional(&state.db)
        .await?;
    let (hash,) = row.ok_or(AppError::Unauthorized)?;
    let attempt = state
        .logins
        .attempt(std::time::Instant::now())
        .ok_or(AppError::TooManyAttempts)?;
    if !auth::verify_password(&req.password, &hash) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(AppError::Unauthorized);
    }
    attempt.succeeded();
    let jar = start_session(&state, jar).await?;
    Ok((
        jar,
        Json(SessionInfo {
            setup_required: false,
            logged_in: true,
        }),
    ))
}

async fn logout(State(state): State<AppState>, _admin: Admin, jar: CookieJar) -> ApiResult<CookieJar> {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(auth::token_hash(c.value()))
            .execute(&state.db)
            .await?;
    }
    Ok(jar.remove(Cookie::build(SESSION_COOKIE).path("/").build()))
}

async fn overview(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Overview>> {
    Ok(Json(queries::overview(&state).await?))
}

async fn games(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<GameSummary>>> {
    Ok(Json(queries::games(&state).await?))
}

async fn game(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
) -> ApiResult<Json<GameDetail>> {
    queries::game(&state, &game)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn restore(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
    Json(req): Json<RestoreRequest>,
) -> ApiResult<()> {
    let meta = versions::meta(&state.db, &req.version)
        .await?
        .ok_or(AppError::NotFound)?;
    if meta.game != game {
        return Err(AppError::BadRequest("version belongs to another game".into()));
    }
    if !state.hub.is_online(&req.device) {
        return Err(AppError::Conflict("Das Gerät ist nicht verbunden".into()));
    }
    // Loading a version means continuing from it. When Spielstand moves this game's saves, it
    // becomes the current state and the other devices follow (their states are kept as
    // versions first); in backup mode only the chosen device is touched.
    if modes::mode_of(&state, &game).await?.0 == spielstand_protocol::SaveMode::Sync
        && versions::head_of(&state.db, &game).await?.as_deref() != Some(req.version.as_str())
    {
        versions::resolve(&state, &game, &req.version).await?;
    }
    let sent = state.hub.send(
        &req.device,
        ServerMessage::Restore {
            game: game.clone(),
            version: req.version,
        },
    );
    if !sent {
        return Err(AppError::Conflict("Das Gerät ist nicht verbunden".into()));
    }
    db::event(
        &state.db,
        Some(&req.device),
        Some(&game),
        EventKind::RestoreRequested,
    )
    .await?;
    Ok(())
}

async fn resolve(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
    Json(req): Json<ResolveRequest>,
) -> ApiResult<()> {
    versions::resolve(&state, &game, &req.version).await
}

#[derive(serde::Deserialize)]
struct BackupRequest {
    device: String,
}

async fn backup(
    State(state): State<AppState>,
    _admin: Admin,
    Path(game): Path<String>,
    Json(req): Json<BackupRequest>,
) -> ApiResult<()> {
    if !state.hub.send(&req.device, ServerMessage::BackUp { game }) {
        return Err(AppError::Conflict("Das Gerät ist nicht verbunden".into()));
    }
    Ok(())
}

async fn patch_version(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    Json(req): Json<VersionPatch>,
) -> ApiResult<()> {
    if let Some(pinned) = req.pinned {
        sqlx::query("UPDATE versions SET pinned = ? WHERE id = ?")
            .bind(pinned)
            .bind(&id)
            .execute(&state.db)
            .await?;
    }
    if let Some(label) = req.label {
        let label = Some(label.trim().to_string()).filter(|l| !l.is_empty());
        sqlx::query("UPDATE versions SET label = ? WHERE id = ?")
            .bind(label)
            .bind(&id)
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

async fn download(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    super::agent::blob_response(&state, &id).await
}

async fn devices(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<Device>>> {
    Ok(Json(queries::devices(&state).await?))
}

/// The device keeps its history; only its token stops working.
async fn remove_device(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
) -> ApiResult<()> {
    sqlx::query("UPDATE devices SET removed_at = ?, token_hash = NULL WHERE id = ?")
        .bind(db::now())
        .bind(&id)
        .execute(&state.db)
        .await?;
    db::event(&state.db, Some(&id), None, EventKind::Removed).await?;
    Ok(())
}

pub fn server_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.settings.public_url {
        return url.trim_end_matches('/').to_string();
    }
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8467");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    format!("{scheme}://{host}")
}

fn new_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let mut pick = || ALPHABET[rng.gen_range(0..ALPHABET.len())] as char;
    format!("{}{}{}-{}{}{}", pick(), pick(), pick(), pick(), pick(), pick())
}

async fn pair_code(
    State(state): State<AppState>,
    _admin: Admin,
    headers: HeaderMap,
) -> ApiResult<Json<PairCode>> {
    let code = new_code();
    let expires_at = Utc::now() + Duration::minutes(15);
    sqlx::query("INSERT INTO pair_codes (code, created_at, expires_at) VALUES (?, ?, ?)")
        .bind(&code)
        .bind(db::now())
        .bind(db::ts(expires_at))
        .execute(&state.db)
        .await?;
    let url = server_url(&state, &headers);
    let command = format!("curl -fsSL {url}/install.sh | sh -s -- {code}");
    Ok(Json(PairCode {
        code,
        expires_at,
        command,
    }))
}

#[derive(serde::Deserialize)]
struct ActivityQuery {
    limit: Option<i64>,
}

async fn activity(
    State(state): State<AppState>,
    _admin: Admin,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<Json<Vec<Activity>>> {
    Ok(Json(
        queries::activity(&state, q.limit.unwrap_or(50).clamp(1, 500)).await?,
    ))
}

async fn import(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<ImportRequest>,
) -> ApiResult<Json<ImportReport>> {
    Ok(Json(importer::import_source(&state, &req.source).await?))
}
