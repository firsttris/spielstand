//! Which version has a given content, and how it relates to what a device had: the agent's way
//! to tell "another tool brought a newer state" from "another tool put an older one back".

use spielstand_protocol::{KnownRequest, KnownResponse};

use crate::app::AppState;

/// Ancestors of `version`, nearest first, by following `parent`.
async fn ancestors(state: &AppState, version: &str) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "WITH RECURSIVE up(id, depth) AS (
             SELECT parent, 1 FROM versions WHERE id = ?
             UNION ALL
             SELECT v.parent, up.depth + 1 FROM versions v JOIN up ON v.id = up.id WHERE up.depth < 100000
         )
         SELECT id FROM up WHERE id IS NOT NULL ORDER BY depth",
    )
    .bind(version)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn known(state: &AppState, req: &KnownRequest) -> anyhow::Result<KnownResponse> {
    // Imported versions are never adopted by a disk on their own: they only show the history.
    let candidates: Vec<(String, bool)> = sqlx::query_as(
        "SELECT v.id, EXISTS (SELECT 1 FROM conflicts c WHERE c.version = v.id AND c.resolved_at IS NULL)
         FROM versions v
         WHERE v.game = ? AND v.content_hash = ? AND v.deleted_at IS NULL AND v.trigger != 'import'
         ORDER BY v.created_at DESC",
    )
    .bind(&req.game)
    .bind(&req.content_hash)
    .fetch_all(&state.db)
    .await?;
    let Some(base) = req.base.as_deref() else {
        return Ok(match candidates.first() {
            Some((id, open)) => KnownResponse {
                version: Some(id.clone()),
                descends_from_base: true,
                is_open_branch: *open,
            },
            None => KnownResponse::default(),
        });
    };
    // Prefer a candidate that descends from the base: the same content may exist twice.
    let mut fallback = None;
    for (id, open) in &candidates {
        if ancestors(state, id).await?.iter().any(|a| a == base) {
            return Ok(KnownResponse {
                version: Some(id.clone()),
                descends_from_base: true,
                is_open_branch: *open,
            });
        }
        fallback.get_or_insert((id.clone(), *open));
    }
    Ok(match fallback {
        Some((id, open)) => KnownResponse {
            version: Some(id),
            descends_from_base: false,
            is_open_branch: open,
        },
        None => KnownResponse::default(),
    })
}
