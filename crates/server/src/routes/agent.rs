//! What agents call: pairing, heads, uploads, downloads, promote, bases and the live connection.

use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::header,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use futures::{SinkExt, StreamExt};
use spielstand_protocol::EventKind;
use spielstand_protocol::{
    AgentMessage, AgentSettings, BackupOnlyRequest, BaseReport, CustomGame, DeviceReport, HeadsResponse,
    KnownRequest, KnownResponse, OverwriteReport, OwnPrefixChoice, PairRequest, PairResponse, PresenceReport,
    PromoteOutcome, PromoteRequest, SaveMode, ServerMessage, UploadOutcome, VersionMeta, paths,
};
use tokio_util::io::ReaderStream;

use crate::{
    alerts,
    app::{AppState, MAX_PACKAGE},
    auth::{self, Agent},
    blobs,
    custom_games::{self, By},
    db,
    error::{ApiResult, AppError},
    framing, known, modes, presence, reports,
    versions::{self, NewVersion},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(paths::PAIR, post(pair))
        .route(paths::HEADS, get(heads))
        .route(paths::VERSIONS, post(upload))
        .route("/api/agent/versions/{id}/blob", get(blob))
        .route("/api/agent/versions/{id}/meta", get(meta))
        .route("/api/agent/versions/{id}/promote", post(promote))
        .route(paths::BASES, post(base))
        .route(paths::PRESENCE, post(report_presence))
        .route(paths::KNOWN, post(known_version))
        .route(paths::OVERWRITES, post(overwritten))
        .route(paths::SETTINGS, get(settings))
        .route(paths::REPORT, post(report))
        .route(paths::CUSTOM_GAMES, post(propose))
        .route("/api/agent/custom-games/{name}", delete(withdraw))
        .route(paths::BACKUP_ONLY, post(backup_only))
        .route(paths::PREFIX, put(own_prefix))
        .route(paths::WS, get(ws))
}

async fn known_version(
    State(state): State<AppState>,
    _agent: Agent,
    Json(req): Json<KnownRequest>,
) -> ApiResult<Json<KnownResponse>> {
    Ok(Json(known::known(&state, &req).await?))
}

async fn overwritten(
    State(state): State<AppState>,
    agent: Agent,
    Json(req): Json<OverwriteReport>,
) -> ApiResult<()> {
    alerts::record(&state, &agent.device_id, &req)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))
}

async fn settings(State(state): State<AppState>, agent: Agent) -> ApiResult<Json<AgentSettings>> {
    Ok(Json(modes::agent_settings(&state, &agent.device_id).await?))
}

async fn report(State(state): State<AppState>, agent: Agent, Json(req): Json<DeviceReport>) -> ApiResult<()> {
    Ok(reports::store(&state, &agent.device_id, &req).await?)
}

/// What a device may decide for itself: propose a custom game (in effect only there until
/// it is taken over), withdraw its proposal, choose its own prefix, and make a game only
/// backed up. Syncing a game again, and anything for other devices, is the web UI's.
async fn propose(
    State(state): State<AppState>,
    agent: Agent,
    Json(game): Json<CustomGame>,
) -> ApiResult<Json<AgentSettings>> {
    custom_games::put(&state, By::Device(&agent.device_id), &game).await?;
    Ok(Json(modes::agent_settings(&state, &agent.device_id).await?))
}

async fn withdraw(
    State(state): State<AppState>,
    agent: Agent,
    Path(name): Path<String>,
) -> ApiResult<Json<AgentSettings>> {
    custom_games::remove(&state, By::Device(&agent.device_id), &name).await?;
    Ok(Json(modes::agent_settings(&state, &agent.device_id).await?))
}

async fn backup_only(
    State(state): State<AppState>,
    agent: Agent,
    Json(req): Json<BackupOnlyRequest>,
) -> ApiResult<Json<AgentSettings>> {
    if req.game.trim().is_empty() {
        return Err(AppError::BadRequest("no game".into()));
    }
    modes::set_game_mode(&state, Some(&agent.device_id), &req.game, Some(SaveMode::Backup)).await?;
    Ok(Json(modes::agent_settings(&state, &agent.device_id).await?))
}

async fn own_prefix(
    State(state): State<AppState>,
    agent: Agent,
    Json(req): Json<OwnPrefixChoice>,
) -> ApiResult<Json<AgentSettings>> {
    let prefix = req.prefix.trim();
    if req.game.trim().is_empty() || !prefix.starts_with(['/', '~']) {
        return Err(AppError::BadRequest(
            "a game and an absolute prefix path are needed".into(),
        ));
    }
    modes::set_prefix_choice(&state, &agent.device_id, &req.game, prefix).await?;
    Ok(Json(modes::agent_settings(&state, &agent.device_id).await?))
}

async fn pair(State(state): State<AppState>, Json(req): Json<PairRequest>) -> ApiResult<Json<PairResponse>> {
    let code = req.code.trim().to_uppercase();
    let attempt = state
        .pairings
        .attempt(std::time::Instant::now())
        .ok_or(AppError::TooManyAttempts)?;
    let used = sqlx::query(
        "UPDATE pair_codes SET used_at = ? WHERE code = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(db::now())
    .bind(&code)
    .bind(db::now())
    .execute(&state.db)
    .await?;
    if used.rows_affected() != 1 {
        return Err(AppError::Unauthorized);
    }
    attempt.succeeded();
    let token = auth::random_token();
    let name = if req.name.trim().is_empty() {
        req.hostname.clone()
    } else {
        req.name.trim().to_string()
    };
    let device_id = match rekey_previous(&state, &req, &name, &token).await? {
        Some(id) => id,
        None => add_device(&state, &req, &name, &token).await?,
    };
    db::event(&state.db, Some(&device_id), None, EventKind::Paired).await?;
    Ok(Json(PairResponse { device_id, token }))
}

/// A PC that pairs again while its pairing is still valid keeps its device (bases, prefix
/// choices, history) and gets a new token, instead of showing up twice.
async fn rekey_previous(
    state: &AppState,
    req: &PairRequest,
    name: &str,
    token: &str,
) -> anyhow::Result<Option<String>> {
    let Some(previous) = &req.previous else {
        return Ok(None);
    };
    let done = sqlx::query(
        "UPDATE devices SET name = ?, hostname = ?, os = ?, agent_version = ?, token_hash = ?, last_seen = ?
         WHERE id = ? AND token_hash = ? AND removed_at IS NULL",
    )
    .bind(name)
    .bind(&req.hostname)
    .bind(&req.os)
    .bind(&req.agent_version)
    .bind(auth::token_hash(token))
    .bind(db::now())
    .bind(&previous.device_id)
    .bind(auth::token_hash(&previous.token))
    .execute(&state.db)
    .await?;
    Ok((done.rows_affected() == 1).then(|| previous.device_id.clone()))
}

async fn add_device(state: &AppState, req: &PairRequest, name: &str, token: &str) -> anyhow::Result<String> {
    let device_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO devices (id, name, hostname, os, agent_version, token_hash, created_at, last_seen)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&device_id)
    .bind(name)
    .bind(&req.hostname)
    .bind(&req.os)
    .bind(&req.agent_version)
    .bind(auth::token_hash(token))
    .bind(db::now())
    .bind(db::now())
    .execute(&state.db)
    .await?;
    Ok(device_id)
}

async fn heads(State(state): State<AppState>, agent: Agent) -> ApiResult<Json<HeadsResponse>> {
    let mut response = versions::heads(&state, &agent.device_id).await?;
    response.playing_elsewhere = presence::playing_elsewhere(&state, &agent.device_id).await?;
    Ok(Json(response))
}

async fn upload(State(state): State<AppState>, agent: Agent, body: Body) -> ApiResult<Json<UploadOutcome>> {
    let (meta_json, received) = framing::receive(&state.settings.tmp_dir(), body, MAX_PACKAGE)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e}")))?;
    let meta: VersionMeta =
        serde_json::from_slice(&meta_json).map_err(|e| AppError::BadRequest(format!("manifest: {e}")))?;
    let outcome = versions::upload(
        &state,
        &agent.device_id,
        NewVersion {
            meta,
            pinned: false,
            label: None,
        },
        received,
    )
    .await?;
    Ok(Json(outcome))
}

pub async fn blob_response(state: &AppState, id: &str) -> ApiResult<Response> {
    let meta = versions::meta(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let path = blobs::path(&state.settings.blobs_dir(), &meta.blob_sha256);
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let headers = [
        (header::CONTENT_TYPE, "application/zstd".to_string()),
        (header::CONTENT_LENGTH, meta.blob_size.to_string()),
    ];
    Ok((headers, Body::from_stream(ReaderStream::new(file))).into_response())
}

async fn blob(State(state): State<AppState>, _agent: Agent, Path(id): Path<String>) -> ApiResult<Response> {
    blob_response(&state, &id).await
}

async fn meta(
    State(state): State<AppState>,
    _agent: Agent,
    Path(id): Path<String>,
) -> ApiResult<Json<VersionMeta>> {
    versions::meta(&state.db, &id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn promote(
    State(state): State<AppState>,
    agent: Agent,
    Path(id): Path<String>,
    Json(req): Json<PromoteRequest>,
) -> ApiResult<Json<PromoteOutcome>> {
    Ok(Json(
        versions::promote(&state, &agent.device_id, &id, req.expected_head.as_deref()).await?,
    ))
}

async fn base(State(state): State<AppState>, agent: Agent, Json(req): Json<BaseReport>) -> ApiResult<()> {
    versions::report_base(&state, &agent.device_id, &req.game, &req.version).await
}

async fn report_presence(
    State(state): State<AppState>,
    agent: Agent,
    Json(req): Json<PresenceReport>,
) -> ApiResult<()> {
    Ok(presence::set_playing(&state, &agent.device_id, req.game.as_deref()).await?)
}

async fn ws(State(state): State<AppState>, agent: Agent, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| live(state, agent, socket))
}

async fn live(state: AppState, agent: Agent, socket: WebSocket) {
    let (conn, mut outgoing) = state.hub.connect(&agent.device_id);
    let (mut tx, mut rx) = socket.split();
    if let Ok(settings) = modes::agent_settings(&state, &agent.device_id).await
        && let Ok(text) = serde_json::to_string(&ServerMessage::Settings { settings })
        && tx.send(Message::Text(text.into())).await.is_err()
    {
        state.hub.disconnect(&agent.device_id, conn);
        return;
    }
    loop {
        tokio::select! {
            Some(message) = outgoing.recv() => {
                let Ok(text) = serde_json::to_string(&message) else { continue };
                if tx.send(Message::Text(text.into())).await.is_err() { break; }
            }
            incoming = rx.next() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                let Ok(parsed) = serde_json::from_str::<AgentMessage>(&text) else { continue };
                if let Some(reply) = handle(&state, &agent, parsed).await {
                    let Ok(text) = serde_json::to_string(&reply) else { continue };
                    if tx.send(Message::Text(text.into())).await.is_err() { break; }
                }
            }
        }
    }
    state.hub.disconnect(&agent.device_id, conn);
    let _ = sqlx::query("UPDATE devices SET last_seen = ? WHERE id = ?")
        .bind(db::now())
        .bind(&agent.device_id)
        .execute(&state.db)
        .await;
}

async fn handle(state: &AppState, agent: &Agent, message: AgentMessage) -> Option<ServerMessage> {
    let result = match message {
        AgentMessage::Ping => return Some(ServerMessage::Pong),
        AgentMessage::Hello { agent_version } => {
            sqlx::query("UPDATE devices SET agent_version = ?, last_seen = ? WHERE id = ?")
                .bind(agent_version)
                .bind(db::now())
                .bind(&agent.device_id)
                .execute(&state.db)
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        }
        AgentMessage::Playing { game } => presence::set_playing(state, &agent.device_id, Some(&game)).await,
        AgentMessage::Stopped { .. } => presence::set_playing(state, &agent.device_id, None).await,
    };
    if let Err(e) = result {
        tracing::warn!("live message from {}: {e:#}", agent.name);
    }
    None
}
