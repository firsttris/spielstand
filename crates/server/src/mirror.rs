//! Offsite copy of every version (Google Drive through rclone, or any folder). The server is the
//! only writer; the mirror is read back only to rebuild a lost server.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use spielstand_core::slug;
use spielstand_protocol::VersionMeta;
use tokio::process::Command;

use crate::{
    app::AppState,
    blobs, db,
    versions::{self, NewVersion},
};

#[async_trait]
pub trait Remote: Send + Sync {
    async fn put(&self, local: &Path, rel: &str) -> anyhow::Result<()>;
    async fn get(&self, rel: &str, local: &Path) -> anyhow::Result<()>;
    async fn list(&self) -> anyhow::Result<Vec<String>>;
    async fn delete(&self, rel: &str) -> anyhow::Result<()>;
}

pub struct LocalDir(pub PathBuf);

#[async_trait]
impl Remote for LocalDir {
    async fn put(&self, local: &Path, rel: &str) -> anyhow::Result<()> {
        let target = self.0.join(rel);
        if let Some(p) = target.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let tmp = target.with_extension("partial");
        tokio::fs::copy(local, &tmp).await?;
        tokio::fs::rename(&tmp, &target).await?;
        Ok(())
    }
    async fn get(&self, rel: &str, local: &Path) -> anyhow::Result<()> {
        tokio::fs::copy(self.0.join(rel), local).await?;
        Ok(())
    }
    async fn list(&self) -> anyhow::Result<Vec<String>> {
        let root = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut out = vec![];
            let mut stack = vec![root.clone()];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let Ok(rel) = p.strip_prefix(&root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
            out
        })
        .await
        .map_err(Into::into)
    }
    async fn delete(&self, rel: &str) -> anyhow::Result<()> {
        let _ = tokio::fs::remove_file(self.0.join(rel)).await;
        Ok(())
    }
}

pub struct Rclone {
    pub bin: String,
    pub remote: String,
}

impl Rclone {
    fn target(&self, rel: &str) -> String {
        format!("{}/{}", self.remote.trim_end_matches('/'), rel)
    }

    async fn run(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = Command::new(&self.bin).args(args).output().await?;
        anyhow::ensure!(
            out.status.success(),
            "rclone {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

#[async_trait]
impl Remote for Rclone {
    async fn put(&self, local: &Path, rel: &str) -> anyhow::Result<()> {
        self.run(&["copyto", &local.to_string_lossy(), &self.target(rel)])
            .await
            .map(|_| ())
    }
    async fn get(&self, rel: &str, local: &Path) -> anyhow::Result<()> {
        self.run(&["copyto", &self.target(rel), &local.to_string_lossy()])
            .await
            .map(|_| ())
    }
    async fn list(&self) -> anyhow::Result<Vec<String>> {
        let out = self.run(&["lsf", "-R", "--files-only", &self.remote]).await?;
        Ok(out.lines().map(str::to_string).collect())
    }
    async fn delete(&self, rel: &str) -> anyhow::Result<()> {
        self.run(&["deletefile", &self.target(rel)]).await.map(|_| ())
    }
}

pub fn remote(spec: &str, rclone: &str) -> Box<dyn Remote> {
    if spec.starts_with('/') || spec.starts_with('.') {
        Box::new(LocalDir(PathBuf::from(spec)))
    } else {
        Box::new(Rclone {
            bin: rclone.to_string(),
            remote: spec.to_string(),
        })
    }
}

/// What the mirror keeps next to each package, enough to rebuild the database row.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub meta: VersionMeta,
    pub device_name: String,
    pub pinned: bool,
    pub label: Option<String>,
    pub branch: bool,
}

pub fn rel_paths(game: &str, id: &str) -> (String, String) {
    let dir = slug(game);
    (format!("{dir}/{id}.tar.zst"), format!("{dir}/{id}.json"))
}

const HEADS: &str = "_state/heads.json";

async fn copy_one(state: &AppState, remote: &dyn Remote, id: &str) -> anyhow::Result<()> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT v.*, d.name AS device_name FROM versions v JOIN devices d ON d.id = v.device WHERE v.id = ?",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let record = Record {
        meta: versions::row_to_meta(&row)?,
        device_name: row.try_get("device_name")?,
        pinned: row.try_get("pinned")?,
        label: row.try_get("label")?,
        branch: row.try_get("branch")?,
    };
    let (blob_rel, json_rel) = rel_paths(&record.meta.game, id);
    remote
        .put(
            &blobs::path(&state.settings.blobs_dir(), &record.meta.blob_sha256),
            &blob_rel,
        )
        .await?;
    let json = tempfile::NamedTempFile::new_in(state.settings.tmp_dir())?;
    std::fs::write(json.path(), serde_json::to_vec_pretty(&record)?)?;
    remote.put(json.path(), &json_rel).await?;
    sqlx::query("UPDATE versions SET mirrored_at = ? WHERE id = ?")
        .bind(db::now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn write_heads(state: &AppState, remote: &dyn Remote) -> anyhow::Result<()> {
    let heads: BTreeMap<String, String> =
        sqlx::query_as::<_, (String, String)>("SELECT id, head FROM games WHERE head IS NOT NULL")
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect();
    let file = tempfile::NamedTempFile::new_in(state.settings.tmp_dir())?;
    std::fs::write(file.path(), serde_json::to_vec_pretty(&heads)?)?;
    remote.put(file.path(), HEADS).await
}

/// Copies every version the mirror lacks; returns how many were copied.
pub async fn sync_once(state: &AppState, remote: &dyn Remote) -> anyhow::Result<usize> {
    tokio::fs::create_dir_all(state.settings.tmp_dir()).await?;
    let pending: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM versions WHERE mirrored_at IS NULL AND deleted_at IS NULL ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;
    for (id,) in &pending {
        copy_one(state, remote, id).await?;
    }
    write_heads(state, remote).await?;
    Ok(pending.len())
}

pub async fn run(state: AppState) {
    let Some(spec) = state.settings.mirror.clone() else {
        return;
    };
    let remote = remote(&spec, &state.settings.rclone);
    loop {
        let result = sync_once(&state, remote.as_ref()).await;
        {
            let mut status = state.mirror_status.lock().unwrap();
            match result {
                Ok(_) => {
                    status.last_ok = Some(Utc::now());
                    status.last_error = None;
                }
                Err(e) => {
                    tracing::warn!("mirror: {e:#}");
                    status.last_error = Some(format!("{e:#}"));
                }
            }
        }
        tokio::select! {
            _ = state.mirror_wake.notified() => tokio::time::sleep(Duration::from_secs(5)).await,
            _ = tokio::time::sleep(Duration::from_secs(600)) => {}
        }
    }
}

async fn ensure_device(state: &AppState, id: &str, name: &str) -> anyhow::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO devices (id, name, created_at) VALUES (?, ?, ?)")
        .bind(id)
        .bind(name)
        .bind(db::now())
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Rebuilds an empty server from its mirror. Devices come back without tokens: pair them again.
pub async fn restore(state: &AppState, remote: &dyn Remote) -> anyhow::Result<usize> {
    let tmp = tempfile::tempdir_in(state.settings.data_dir.as_path())?;
    let files = remote.list().await?;
    let mut restored = 0;
    for json_rel in files
        .iter()
        .filter(|f| f.ends_with(".json") && !f.starts_with("_state/"))
    {
        let local = tmp.path().join("record.json");
        remote.get(json_rel, &local).await?;
        let record: Record = serde_json::from_slice(&std::fs::read(&local)?)?;
        let (blob_rel, _) = rel_paths(&record.meta.game, &record.meta.id);
        let blob_local = tmp.path().join("blob");
        remote.get(&blob_rel, &blob_local).await?;
        let sha = spielstand_core::pack::sha256_file(&blob_local)?;
        anyhow::ensure!(sha == record.meta.blob_sha256, "{blob_rel} is damaged");
        blobs::store_file(&state.settings.blobs_dir(), &blob_local, &sha).await?;
        ensure_device(state, &record.meta.device, &record.device_name).await?;
        let id = record.meta.id.clone();
        let new = NewVersion {
            meta: record.meta,
            pinned: record.pinned,
            label: record.label,
        };
        if versions::insert_external(state, new, record.branch).await? {
            sqlx::query("UPDATE versions SET mirrored_at = ? WHERE id = ?")
                .bind(db::now())
                .bind(&id)
                .execute(&state.db)
                .await?;
            restored += 1;
        }
    }
    let heads_local = tmp.path().join("heads.json");
    if remote.get(HEADS, &heads_local).await.is_ok() {
        let heads: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(&heads_local)?)?;
        for (game, id) in heads {
            versions::set_head_unchecked(state, &game, &id).await?;
        }
    }
    Ok(restored)
}
