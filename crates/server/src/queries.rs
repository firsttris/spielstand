//! Read models for the web UI.

use spielstand_protocol::EventKind;
use std::collections::{BTreeMap, BTreeSet};

use spielstand_protocol::{
    Activity, Device, DeviceReport, GameDetail, GameSummary, HeadSummary, Overview, VersionRow,
};
use sqlx::Row;

use crate::{alerts, app::AppState, db, modes, presence, reports, switch_titles, versions};

async fn device_names(state: &AppState) -> anyhow::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM devices")
        .fetch_all(&state.db)
        .await?;
    Ok(rows.into_iter().collect())
}

async fn playing_by_game(state: &AppState) -> anyhow::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, playing_game, last_seen FROM devices WHERE playing_game IS NOT NULL AND removed_at IS NULL",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _, seen)| presence::is_current(state, id, seen.as_deref()))
        .map(|(_, name, game, _)| (game, name))
        .collect())
}

pub async fn devices(state: &AppState) -> anyhow::Result<Vec<Device>> {
    let rows = sqlx::query(
        "SELECT d.*, (SELECT COUNT(*) FROM conflicts c WHERE c.device = d.id AND c.resolved_at IS NULL) AS open
         FROM devices d WHERE d.removed_at IS NULL AND d.token_hash IS NOT NULL ORDER BY d.created_at",
    )
    .fetch_all(&state.db)
    .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("id")?;
            let last_seen: Option<String> = r.try_get("last_seen")?;
            let current = presence::is_current(state, &id, last_seen.as_deref());
            let report: Option<DeviceReport> = r
                .try_get::<Option<String>, _>("report")?
                .and_then(|json| serde_json::from_str(&json).ok());
            let folders: String = r.try_get("game_folders")?;
            Ok(Device {
                game_folders: serde_json::from_str(&folders).unwrap_or_default(),
                stellwerk_agent: report.as_ref().is_some_and(|r| r.stellwerk_agent),
                prefixr: report.as_ref().is_some_and(|r| r.prefixr),
                online: state.hub.is_online(&id),
                name: r.try_get("name")?,
                hostname: r.try_get("hostname")?,
                os: r.try_get("os")?,
                agent_version: r.try_get("agent_version")?,
                last_seen: db::parse_opt(last_seen),
                playing: if current { r.try_get("playing_game")? } else { None },
                playing_since: if current {
                    db::parse_opt(r.try_get("playing_since")?)
                } else {
                    None
                },
                pending_conflicts: r.try_get::<i64, _>("open")? as u64,
                id,
            })
        })
        .collect()
}

/// The newest name a version gave each game, for games whose id is not a name.
async fn titles(state: &AppState) -> anyhow::Result<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT game, title FROM versions WHERE title IS NOT NULL AND deleted_at IS NULL ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().collect())
}

pub async fn games(state: &AppState) -> anyhow::Result<Vec<GameSummary>> {
    let titles = titles(state).await?;
    let names = device_names(state).await?;
    let playing = playing_by_game(state).await?;
    let (default_mode, game_modes) = (modes::default_mode(state).await?, modes::game_modes(state).await?);
    let overwritten: BTreeSet<String> = alerts::open(state, None)
        .await?
        .into_iter()
        .map(|a| a.game)
        .collect();
    let rows = sqlx::query(
        "SELECT g.id, g.head, v.content_hash, v.device, v.created_at,
            (SELECT COUNT(*) FROM versions x WHERE x.game = g.id AND x.deleted_at IS NULL) AS n,
            (SELECT COUNT(*) FROM conflicts c WHERE c.game = g.id AND c.resolved_at IS NULL) AS open
         FROM games g LEFT JOIN versions v ON v.id = g.head
         ORDER BY v.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    let bases: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT b.game, b.version, d.name FROM device_bases b JOIN devices d ON d.id = b.device
                        WHERE d.removed_at IS NULL AND d.token_hash IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("id")?;
            let head_id: Option<String> = r.try_get("head")?;
            let head = match &head_id {
                Some(h) => Some(HeadSummary {
                    id: h.clone(),
                    content_hash: r.try_get("content_hash")?,
                    device: r.try_get("device")?,
                    created_at: db::parse(&r.try_get::<String, _>("created_at")?),
                }),
                None => None,
            };
            let behind = bases
                .iter()
                .filter(|(g, v, _)| *g == id && Some(v) != head_id.as_ref())
                .map(|(_, _, name)| name.clone())
                .collect();
            Ok(GameSummary {
                head_device_name: head.as_ref().and_then(|h| names.get(&h.device).cloned()),
                head,
                versions: r.try_get::<i64, _>("n")? as u64,
                conflict: r.try_get::<i64, _>("open")? > 0,
                playing_on: playing.get(&id).cloned(),
                behind,
                overwritten: overwritten.contains(&id),
                mode: game_modes.get(&id).copied().unwrap_or(default_mode),
                title: titles
                    .get(&id)
                    .cloned()
                    .or_else(|| switch_titles::name_of(&state.switch_titles, &id)),
                id,
            })
        })
        .collect()
}

pub async fn game(state: &AppState, game: &str) -> anyhow::Result<Option<GameDetail>> {
    let head = versions::head_of(&state.db, game).await?;
    let rows =
        sqlx::query("SELECT * FROM versions WHERE game = ? AND deleted_at IS NULL ORDER BY created_at DESC")
            .bind(game)
            .fetch_all(&state.db)
            .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let names = device_names(state).await?;
    let bases: Vec<(String, String)> = sqlx::query_as(
        "SELECT b.version, d.name FROM device_bases b JOIN devices d ON d.id = b.device
         WHERE b.game = ? AND d.removed_at IS NULL AND d.token_hash IS NOT NULL",
    )
    .bind(game)
    .fetch_all(&state.db)
    .await?;
    let open: BTreeSet<String> = sqlx::query_as::<_, (String,)>(
        "SELECT version FROM conflicts WHERE game = ? AND resolved_at IS NULL",
    )
    .bind(game)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(v,)| v)
    .collect();
    let versions = rows
        .iter()
        .map(|r| {
            let meta = versions::row_to_meta(r)?;
            Ok(VersionRow {
                device_name: names
                    .get(&meta.device)
                    .cloned()
                    .unwrap_or_else(|| meta.device.clone()),
                is_head: head.as_ref() == Some(&meta.id),
                is_branch: r.try_get::<bool, _>("branch")?,
                pinned: r.try_get("pinned")?,
                label: r.try_get("label")?,
                on_devices: bases
                    .iter()
                    .filter(|(v, _)| *v == meta.id)
                    .map(|(_, n)| n.clone())
                    .collect(),
                meta,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let playing = playing_by_game(state).await?;
    let (mode, mode_is_default) = modes::mode_of(state, game).await?;
    Ok(Some(GameDetail {
        id: game.to_string(),
        title: versions
            .iter()
            .find_map(|v| v.meta.title.clone())
            .or_else(|| switch_titles::name_of(&state.switch_titles, game)),
        conflict: !open.is_empty(),
        locations: reports::locations(state, game, head.as_deref()).await?,
        alerts: alerts::open(state, Some(game)).await?,
        head,
        versions,
        playing_on: playing.get(game).cloned(),
        mode,
        mode_is_default,
    }))
}

/// id, time, device name, game, event kind.
type EventRow = (i64, String, Option<String>, Option<String>, String);

pub async fn activity(state: &AppState, limit: i64) -> anyhow::Result<Vec<Activity>> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT e.id, e.at, d.name, e.game, e.text FROM events e LEFT JOIN devices d ON d.id = e.device
         ORDER BY e.id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, at, device_name, game, text)| Activity {
            id,
            at: db::parse(&at),
            device_name,
            game,
            kind: EventKind::parse(&text),
        })
        .collect())
}

pub async fn overview(state: &AppState) -> anyhow::Result<Overview> {
    let (games, conflicts): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM games), (SELECT COUNT(*) FROM conflicts WHERE resolved_at IS NULL)",
    )
    .fetch_one(&state.db)
    .await?;
    let (versions, bytes): (i64, Option<i64>) =
        sqlx::query_as("SELECT COUNT(*), SUM(blob_size) FROM versions WHERE deleted_at IS NULL")
            .fetch_one(&state.db)
            .await?;
    let devices = devices(state).await?;
    let last = sqlx::query(
        "SELECT v.*, d.name AS device_name FROM versions v JOIN devices d ON d.id = v.device
         WHERE v.deleted_at IS NULL ORDER BY v.created_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    let (last_version, last_version_game, last_version_device) = match last {
        Some(r) => {
            let m = versions::row_to_meta(&r)?;
            let summary = HeadSummary {
                id: m.id.clone(),
                content_hash: m.content_hash.clone(),
                device: m.device.clone(),
                created_at: m.created_at,
            };
            (Some(summary), Some(m.game), Some(r.try_get("device_name")?))
        }
        None => (None, None, None),
    };
    let pending: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM versions WHERE mirrored_at IS NULL AND deleted_at IS NULL")
            .fetch_one(&state.db)
            .await?;
    let mut mirror = state.mirror_status.lock().unwrap().clone();
    mirror.pending = if mirror.enabled { pending.0 as u64 } else { 0 };
    Ok(Overview {
        games: games as u64,
        versions: versions as u64,
        stored_bytes: bytes.unwrap_or(0) as u64,
        conflicts: conflicts as u64,
        devices: devices.len() as u64,
        devices_online: devices.iter().filter(|d| d.online).count() as u64,
        last_version,
        last_version_game,
        last_version_device,
        mirror,
        version: env!("CARGO_PKG_VERSION").to_string(),
        alerts: alerts::open(state, None).await?.len() as u64,
        default_mode: modes::default_mode(state).await?,
    })
}
