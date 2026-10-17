//! Reading the disk into versions and writing versions back.
//!
//! The rule: before anything is written, the state on disk exists as a version. Writing is
//! staged next to each target and checked, then swapped in; files the version doesn't have go
//! to the agent's trash, never deleted. If any step fails, the snapshot taken before is put back.

use std::path::{Path, PathBuf};

use chrono::Utc;
use spielstand_core::{
    pack::{self, Source},
    portable::{self, Anchors},
};
use spielstand_protocol::{FileEntry, Trigger, VersionMeta};

use crate::{
    config::Dirs,
    engine::Scanner,
    store::{Base, Store},
};

pub struct DiskState {
    pub sources: Vec<Source>,
    pub files: Vec<FileEntry>,
    /// `None` when the game has no saves on this disk.
    pub content_hash: Option<String>,
}

pub fn read(scanner: &dyn Scanner, game: &str) -> anyhow::Result<DiskState> {
    let anchors = scanner.anchors();
    let sources: Vec<Source> = scanner
        .save_files(game)
        .into_iter()
        .map(|path| Source {
            portable: portable::to_portable(&path, &anchors),
            path,
        })
        .collect();
    let files = pack::describe(&sources)?;
    let content_hash = (!files.is_empty()).then(|| pack::content_hash(&files));
    Ok(DiskState {
        sources,
        files,
        content_hash,
    })
}

/// Packs the disk state of `game` into the outbox and makes it the base. Returns `None` when
/// the disk has no saves or already equals the base (that state is stored already).
pub async fn snapshot(
    dirs: &Dirs,
    store: &Store,
    scanner: &dyn Scanner,
    device: &str,
    game: &str,
    trigger: Trigger,
    session_seconds: Option<u64>,
) -> anyhow::Result<Option<VersionMeta>> {
    let disk = read(scanner, game)?;
    let Some(hash) = disk.content_hash.clone() else {
        return Ok(None);
    };
    let base = store.base(game).await?;
    if base.as_ref().is_some_and(|b| b.content_hash == hash) {
        return Ok(None);
    }
    let now = Utc::now();
    let id = spielstand_core::new_version_id(now);
    tokio::fs::create_dir_all(dirs.outbox()).await?;
    let blob = dirs.outbox().join(format!("{id}.tar.zst"));
    let packed = {
        let blob = blob.clone();
        tokio::task::spawn_blocking(move || pack::pack(&disk.sources, &blob)).await??
    };
    let meta = VersionMeta {
        id: id.clone(),
        game: game.to_string(),
        parent: base.map(|b| b.version),
        device: device.to_string(),
        created_at: now,
        trigger,
        session_seconds,
        content_hash: packed.content_hash.clone(),
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
    };
    store.enqueue(&meta, &blob).await?;
    store
        .set_base(
            game,
            &Base {
                version: id,
                content_hash: packed.content_hash,
            },
        )
        .await?;
    Ok(Some(meta))
}

struct Planned {
    target: PathBuf,
    staged: PathBuf,
}

fn staged_name(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.spielstand-new"))
}

fn targets(meta: &VersionMeta, anchors: &Anchors) -> anyhow::Result<Vec<(String, PathBuf)>> {
    meta.files
        .iter()
        .map(|f| {
            let target = portable::from_portable(&f.path, anchors)
                .ok_or_else(|| anyhow::anyhow!("no place for {} on this device", f.path))?;
            anyhow::ensure!(
                portable::is_allowed_target(&target, anchors),
                "refusing to write {}",
                target.display()
            );
            Ok((f.path.clone(), target))
        })
        .collect()
}

/// Writes `meta` (its package at `blob`) to the disk. The caller has taken a snapshot first.
pub fn write(dirs: &Dirs, scanner: &dyn Scanner, meta: &VersionMeta, blob: &Path) -> anyhow::Result<()> {
    let anchors = scanner.anchors();
    let contents = pack::unpack(blob, &meta.files)?;
    let places = targets(meta, &anchors)?;

    let mut planned = Vec::with_capacity(places.len());
    let staging = (|| -> anyhow::Result<()> {
        for (portable, target) in &places {
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let staged = staged_name(target);
            std::fs::write(&staged, &contents[portable])?;
            std::fs::File::open(&staged)?.sync_all()?;
            anyhow::ensure!(
                pack::sha256_file(&staged)?
                    == meta.files.iter().find(|f| &f.path == portable).unwrap().sha256,
                "staged {} does not match",
                target.display()
            );
            planned.push(Planned {
                target: target.clone(),
                staged,
            });
        }
        Ok(())
    })();
    if let Err(e) = staging {
        for p in &planned {
            let _ = std::fs::remove_file(&p.staged);
        }
        return Err(e);
    }

    let keep: std::collections::BTreeSet<PathBuf> = places.iter().map(|(_, t)| t.clone()).collect();
    let trash = dirs.trash().join(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S"),
        spielstand_core::slug(&meta.game)
    ));
    for extra in scanner
        .save_files(&meta.game)
        .into_iter()
        .filter(|p| !keep.contains(p))
    {
        std::fs::create_dir_all(&trash)?;
        let name = extra.to_string_lossy().replace(['/', '\\'], "_");
        if std::fs::rename(&extra, trash.join(&name)).is_err() {
            std::fs::copy(&extra, trash.join(&name))?;
            std::fs::remove_file(&extra)?;
        }
    }
    for p in &planned {
        std::fs::rename(&p.staged, &p.target)?;
    }
    let check = read(scanner, &meta.game)?;
    anyhow::ensure!(
        check.content_hash.as_deref() == Some(meta.content_hash.as_str()),
        "after writing, the disk does not hold {}",
        meta.id
    );
    Ok(())
}

/// Writes `meta`; if that fails, puts `previous` (the snapshot taken before) back.
pub fn write_or_roll_back(
    dirs: &Dirs,
    scanner: &dyn Scanner,
    meta: &VersionMeta,
    blob: &Path,
    previous: Option<(&VersionMeta, &Path)>,
) -> anyhow::Result<()> {
    match write(dirs, scanner, meta, blob) {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some((prev, prev_blob)) = previous
                && let Err(back) = write(dirs, scanner, prev, prev_blob)
            {
                anyhow::bail!("{e:#}; putting the previous state back also failed: {back:#}");
            }
            Err(e)
        }
    }
}
