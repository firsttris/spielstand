//! Heads move only here, and only by compare-and-swap. A version whose parent is not the head
//! is kept as a branch with an open conflict; nothing is ever dropped.

use spielstand_core::pack::content_hash;
use spielstand_protocol::EventKind;
use spielstand_protocol::{
    HeadSummary, HeadsResponse, PromoteOutcome, ServerMessage, UploadOutcome, UploadStatus, VersionMeta,
};

use crate::{
    app::AppState,
    blobs, db,
    error::{ApiResult, AppError},
};

pub struct NewVersion {
    pub meta: VersionMeta,
    pub pinned: bool,
    pub label: Option<String>,
}

pub async fn head_of(db: &db::Db, game: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT head FROM games WHERE id = ?")
        .bind(game)
        .fetch_optional(db)
        .await?;
    Ok(row.and_then(|(h,)| h))
}

pub fn check_meta(meta: &VersionMeta, sha: &str, size: u64) -> Result<(), AppError> {
    if meta.blob_sha256 != sha || meta.blob_size != size {
        return Err(AppError::BadRequest("package does not match its manifest".into()));
    }
    if content_hash(&meta.files) != meta.content_hash {
        return Err(AppError::BadRequest(
            "content hash does not match the file list".into(),
        ));
    }
    Ok(())
}

async fn insert(tx: &mut sqlx::SqliteConnection, v: &NewVersion, branch: bool) -> anyhow::Result<()> {
    let m = &v.meta;
    sqlx::query("INSERT OR IGNORE INTO games (id, head, updated_at) VALUES (?, NULL, ?)")
        .bind(&m.game)
        .bind(db::now())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO versions (id, game, parent, device, created_at, trigger, session_seconds, content_hash,
            blob_sha256, blob_size, files_json, pinned, label, branch)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&m.id)
    .bind(&m.game)
    .bind(&m.parent)
    .bind(&m.device)
    .bind(db::ts(m.created_at))
    .bind(serde_json::to_string(&m.trigger)?.trim_matches('"'))
    .bind(m.session_seconds.map(|s| s as i64))
    .bind(&m.content_hash)
    .bind(&m.blob_sha256)
    .bind(m.blob_size as i64)
    .bind(serde_json::to_string(&m.files)?)
    .bind(v.pinned)
    .bind(&v.label)
    .bind(branch)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn try_move_head(
    tx: &mut sqlx::SqliteConnection,
    game: &str,
    to: &str,
    expected: Option<&str>,
) -> anyhow::Result<bool> {
    let done = sqlx::query("UPDATE games SET head = ?, updated_at = ? WHERE id = ? AND head IS ?")
        .bind(to)
        .bind(db::now())
        .bind(game)
        .bind(expected)
        .execute(&mut *tx)
        .await?;
    Ok(done.rows_affected() == 1)
}

async fn set_base(
    tx: &mut sqlx::SqliteConnection,
    device: &str,
    game: &str,
    version: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO device_bases (device, game, version, updated_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (device, game) DO UPDATE SET version = excluded.version, updated_at = excluded.updated_at",
    )
    .bind(device)
    .bind(game)
    .bind(version)
    .bind(db::now())
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn resolve_conflicts(
    tx: &mut sqlx::SqliteConnection,
    game: &str,
    device: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE conflicts SET resolved_at = ? WHERE game = ? AND resolved_at IS NULL AND (? IS NULL OR device = ?)",
    )
    .bind(db::now())
    .bind(game)
    .bind(device)
    .bind(device)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Stores an uploaded version from `device` and moves the head if its parent is the head.
pub async fn upload(
    state: &AppState,
    device: &str,
    mut version: NewVersion,
    received: blobs::Received,
) -> ApiResult<UploadOutcome> {
    version.meta.device = device.to_string();
    check_meta(&version.meta, &received.sha256, received.size)?;
    let game = version.meta.game.clone();
    let id = version.meta.id.clone();

    let known: Option<(String,)> = sqlx::query_as("SELECT id FROM versions WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;
    if known.is_some() {
        return Ok(UploadOutcome {
            status: UploadStatus::Known,
            head: head_of(&state.db, &game).await?,
        });
    }
    blobs::store(&state.settings.blobs_dir(), received).await?;

    let mut tx = state.db.begin().await?;
    insert(&mut tx, &version, false).await?;
    let moved = try_move_head(&mut tx, &game, &id, version.meta.parent.as_deref()).await?;
    if !moved {
        sqlx::query("UPDATE versions SET branch = 1 WHERE id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO conflicts (version, game, device, created_at) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(&game)
            .bind(device)
            .bind(db::now())
            .execute(&mut *tx)
            .await?;
    }
    set_base(&mut tx, device, &game, &id).await?;
    tx.commit().await?;

    let text = if moved {
        EventKind::Uploaded
    } else {
        EventKind::Branched
    };
    db::event(&state.db, Some(device), Some(&game), text).await?;
    if moved {
        state.hub.broadcast_except(
            Some(device),
            ServerMessage::NewHead {
                game: game.clone(),
                version: id.clone(),
                device: device.to_string(),
            },
        );
    }
    state.mirror_wake.notify_one();
    let status = if moved {
        UploadStatus::Head
    } else {
        UploadStatus::Branch
    };
    Ok(UploadOutcome {
        status,
        head: head_of(&state.db, &game).await?,
    })
}

/// Inserts a version that did not come from an agent (import, mirror restore). It never moves
/// a head: imported history is offered in the timeline, loading it is the user's choice.
pub async fn insert_external(state: &AppState, version: NewVersion, branch: bool) -> anyhow::Result<bool> {
    let mut tx = state.db.begin().await?;
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM versions WHERE id = ?")
        .bind(&version.meta.id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_some() {
        return Ok(false);
    }
    insert(&mut tx, &version, branch).await?;
    tx.commit().await?;
    Ok(true)
}

/// Sets a head without compare-and-swap; only for rebuilding a server from its mirror.
pub async fn set_head_unchecked(state: &AppState, game: &str, id: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE games SET head = ?, updated_at = ? WHERE id = ?")
        .bind(id)
        .bind(db::now())
        .bind(game)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// An agent keeps its own branch ("keep mine"): head moves only if it is still `expected`.
pub async fn promote(
    state: &AppState,
    device: &str,
    id: &str,
    expected: Option<&str>,
) -> ApiResult<PromoteOutcome> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT game FROM versions WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (game,) = row.ok_or(AppError::NotFound)?;
    let mut tx = state.db.begin().await?;
    let promoted = try_move_head(&mut tx, &game, id, expected).await?;
    if promoted {
        sqlx::query("UPDATE versions SET branch = 0 WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        resolve_conflicts(&mut tx, &game, None).await?;
        set_base(&mut tx, device, &game, id).await?;
    }
    tx.commit().await?;
    if promoted {
        db::event(&state.db, Some(device), Some(&game), EventKind::KeptOwn).await?;
        state.hub.broadcast_except(
            Some(device),
            ServerMessage::NewHead {
                game: game.clone(),
                version: id.to_string(),
                device: device.to_string(),
            },
        );
    }
    Ok(PromoteOutcome {
        promoted,
        head: head_of(&state.db, &game).await?,
    })
}

/// The admin chooses the version to continue with; every device follows on its next start.
pub async fn resolve(state: &AppState, game: &str, id: &str) -> ApiResult<()> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT game, device FROM versions WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (version_game, device) = row.ok_or(AppError::NotFound)?;
    if version_game != game {
        return Err(AppError::BadRequest("version belongs to another game".into()));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE games SET head = ?, updated_at = ? WHERE id = ?")
        .bind(id)
        .bind(db::now())
        .bind(game)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE versions SET branch = 0 WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    resolve_conflicts(&mut tx, game, None).await?;
    tx.commit().await?;
    db::event(&state.db, None, Some(game), EventKind::Resolved).await?;
    state.hub.broadcast_except(
        None,
        ServerMessage::NewHead {
            game: game.into(),
            version: id.into(),
            device,
        },
    );
    Ok(())
}

/// The disk of `device` now holds `version`. Choosing the head closes this device's branches.
pub async fn report_base(state: &AppState, device: &str, game: &str, version: &str) -> ApiResult<()> {
    let row: Option<(String,)> = sqlx::query_as("SELECT game FROM versions WHERE id = ?")
        .bind(version)
        .fetch_optional(&state.db)
        .await?;
    if row.map(|(g,)| g) != Some(game.to_string()) {
        return Err(AppError::BadRequest("unknown version for this game".into()));
    }
    let head = head_of(&state.db, game).await?;
    let mut tx = state.db.begin().await?;
    set_base(&mut tx, device, game, version).await?;
    if head.as_deref() == Some(version) {
        resolve_conflicts(&mut tx, game, Some(device)).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn heads(state: &AppState, device: &str) -> anyhow::Result<HeadsResponse> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT g.id, v.id, v.content_hash, v.device, v.created_at FROM games g JOIN versions v ON v.id = g.head",
    )
    .fetch_all(&state.db)
    .await?;
    let heads = rows
        .into_iter()
        .map(|(game, id, content_hash, device, created_at)| {
            (
                game,
                HeadSummary {
                    id,
                    content_hash,
                    device,
                    created_at: db::parse(&created_at),
                },
            )
        })
        .collect();
    let open: Vec<(String,)> =
        sqlx::query_as("SELECT version FROM conflicts WHERE device = ? AND resolved_at IS NULL")
            .bind(device)
            .fetch_all(&state.db)
            .await?;
    Ok(HeadsResponse {
        heads,
        open_branches: open.into_iter().map(|(v,)| v).collect(),
        playing_elsewhere: Default::default(),
    })
}

pub fn row_to_meta(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<VersionMeta> {
    use sqlx::Row;
    let trigger: String = row.try_get("trigger")?;
    Ok(VersionMeta {
        id: row.try_get("id")?,
        game: row.try_get("game")?,
        parent: row.try_get("parent")?,
        device: row.try_get("device")?,
        created_at: db::parse(&row.try_get::<String, _>("created_at")?),
        trigger: serde_json::from_str(&format!("\"{trigger}\""))?,
        session_seconds: row
            .try_get::<Option<i64>, _>("session_seconds")?
            .map(|s| s as u64),
        content_hash: row.try_get("content_hash")?,
        blob_sha256: row.try_get("blob_sha256")?,
        blob_size: row.try_get::<i64, _>("blob_size")? as u64,
        files: serde_json::from_str(&row.try_get::<String, _>("files_json")?)?,
    })
}

pub async fn meta(db: &db::Db, id: &str) -> anyhow::Result<Option<VersionMeta>> {
    let row = sqlx::query("SELECT * FROM versions WHERE id = ? AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(db)
        .await?;
    row.as_ref().map(row_to_meta).transpose()
}
