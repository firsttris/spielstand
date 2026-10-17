//! Retention: old versions go to the trash (hidden, still restorable by the admin), and after
//! `TRASH_DAYS` their packages are removed. Protected versions never go.

use std::time::Duration;

use chrono::Utc;
use spielstand_core::retention::{self, Item, Policy};

use crate::{app::AppState, blobs, db, mirror};

pub const TRASH_DAYS: i64 = 30;

async fn protected_ids(state: &AppState) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM versions WHERE pinned = 1
         UNION SELECT head FROM games WHERE head IS NOT NULL
         UNION SELECT version FROM device_bases
         UNION SELECT version FROM conflicts WHERE resolved_at IS NULL",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Moves expired versions to the trash; returns how many.
pub async fn trash_expired(state: &AppState, policy: Policy) -> anyhow::Result<usize> {
    let protected = protected_ids(state).await?;
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT id, game, device, created_at FROM versions WHERE deleted_at IS NULL")
            .fetch_all(&state.db)
            .await?;
    let items: Vec<Item> = rows
        .into_iter()
        .map(|(id, game, device, created_at)| Item {
            protected: protected.contains(&id),
            id,
            game,
            device,
            created_at: db::parse(&created_at),
        })
        .collect();
    let expired = retention::expired(&items, Utc::now(), policy);
    for id in &expired {
        sqlx::query("UPDATE versions SET deleted_at = ? WHERE id = ?")
            .bind(db::now())
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(expired.len())
}

/// Removes versions that sat in the trash long enough, with their packages and mirror copies.
pub async fn purge(state: &AppState) -> anyhow::Result<usize> {
    let cutoff = db::ts(Utc::now() - chrono::Duration::days(TRASH_DAYS));
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, game, blob_sha256 FROM versions WHERE deleted_at IS NOT NULL AND deleted_at < ?",
    )
    .bind(cutoff)
    .fetch_all(&state.db)
    .await?;
    let remote = state
        .settings
        .mirror
        .as_ref()
        .map(|m| mirror::remote(m, &state.settings.rclone));
    for (id, game, sha) in &rows {
        sqlx::query("DELETE FROM conflicts WHERE version = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
        sqlx::query("DELETE FROM versions WHERE id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
        let still_used: Option<(String,)> =
            sqlx::query_as("SELECT id FROM versions WHERE blob_sha256 = ? LIMIT 1")
                .bind(sha)
                .fetch_optional(&state.db)
                .await?;
        if still_used.is_none() {
            let _ = tokio::fs::remove_file(blobs::path(&state.settings.blobs_dir(), sha)).await;
        }
        if let Some(remote) = &remote {
            let (blob_rel, json_rel) = mirror::rel_paths(game, id);
            let _ = remote.delete(&blob_rel).await;
            let _ = remote.delete(&json_rel).await;
        }
    }
    Ok(rows.len())
}

pub async fn run(state: AppState) {
    loop {
        if let Err(e) = trash_expired(&state, Policy::default()).await {
            tracing::warn!("retention: {e:#}");
        }
        if let Err(e) = purge(&state).await {
            tracing::warn!("purge: {e:#}");
        }
        let _ = sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
            .bind(db::now())
            .execute(&state.db)
            .await;
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
