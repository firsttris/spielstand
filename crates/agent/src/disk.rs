//! Reading the disk into versions and writing versions back.
//!
//! The rule: before anything is written, the state on disk exists as a version. Writing is
//! staged next to each target and checked, then swapped in; files the version doesn't have go
//! to the agent's trash, never deleted. If any step fails, the snapshot taken before is put back.
//!
//! Saves inside a Wine prefix are stored relative to the prefix (`<prefix>/drive_c/…`) and by
//! their real path: Proton and Prefixr link `drive_c/users/<name>` to `steamuser`, so the same
//! file can be found twice; it counts once, under `steamuser`.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use chrono::Utc;
use spielstand_core::{
    pack::{self, Source},
    portable::{self, Anchors},
};
use spielstand_protocol::{FileEntry, Trigger, VersionMeta};

use crate::{
    config::Dirs,
    engine::Scanner,
    prefixes,
    store::{Base, Store},
};

pub struct DiskState {
    pub sources: Vec<Source>,
    pub files: Vec<FileEntry>,
    /// `None` when the game has no saves on this disk.
    pub content_hash: Option<String>,
    /// The Wine prefix the saves are in, if they are in one.
    pub prefix: Option<PathBuf>,
}

/// The agent's own files next to a save while it writes: never part of a version.
const STAGING: &str = ".spielstand-new";

pub fn is_staging(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(STAGING))
}

/// The save files of `game` now, without the agent's own staging files. Once the game's own
/// prefix holds saves, saves in other prefixes are not the game's: an old prefix it moved out
/// of, or another install. They are neither backed up as the game's state nor touched.
fn save_files(scanner: &dyn Scanner, game: &str) -> Vec<PathBuf> {
    let mut files = scanner.save_files(game);
    files.retain(|f| !is_staging(f));
    if let Some(own) = scanner.own_prefix(game)
        && files.iter().any(|f| in_prefix(f, &own))
    {
        files.retain(|f| prefixes::prefix_of(f).is_none() || in_prefix(f, &own));
    }
    files
}

/// `file` lies in `prefix` (compared by real path: a prefix may be reached through a link).
fn in_prefix(file: &Path, prefix: &Path) -> bool {
    prefixes::prefix_of(file).is_some_and(|p| real_path(&p) == real_path(prefix))
}

/// The real path of `path`; for a file that does not exist yet, its folder's real path.
pub fn real_path(path: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => real_path(dir).join(name),
        _ => path.to_path_buf(),
    }
}

/// The prefix most save files lie in.
fn main_prefix(files: &[PathBuf]) -> Option<PathBuf> {
    let mut counts: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for prefix in files.iter().filter_map(|f| prefixes::prefix_of(f)) {
        *counts.entry(prefix).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(p, _)| p)
}

fn portable_of(path: &Path, prefix: Option<&Path>, anchors: &Anchors) -> String {
    if let Some(prefix) = prefix
        && let Some(p) = portable::to_portable_in_prefix(&real_path(path), &real_path(prefix))
    {
        return p;
    }
    portable::to_portable(path, anchors)
}

pub fn read(scanner: &dyn Scanner, game: &str) -> anyhow::Result<DiskState> {
    let anchors = scanner.anchors();
    let found = save_files(scanner, game);
    let prefix = main_prefix(&found);
    let mut seen = BTreeSet::new();
    let mut portables = BTreeSet::new();
    let sources: Vec<Source> = found
        .into_iter()
        .filter(|path| seen.insert(real_path(path)))
        .map(|path| Source {
            portable: portable_of(&path, prefix.as_deref(), &anchors),
            path,
        })
        // Two real profiles with the same save path: one place to put it back, so one file.
        .filter(|s| portables.insert(s.portable.clone()))
        .collect();
    let files = pack::describe(&sources)?;
    let content_hash = (!files.is_empty()).then(|| pack::content_hash(&files));
    Ok(DiskState {
        sources,
        files,
        content_hash,
        prefix,
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
    let prefix_hint = disk
        .prefix
        .as_deref()
        .map(|p| portable::to_portable(p, &scanner.anchors()));
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
        prefix: prefix_hint,
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
    target.with_file_name(format!(".{name}{STAGING}"))
}

/// The user name plain Wine would give a new profile: the home folder's name.
fn linux_user(home: &Path) -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .or_else(|| home.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "steamuser".into())
}

/// Where each file of `meta` goes on this device; `prefix` is the game's prefix here.
pub fn targets(
    meta: &VersionMeta,
    anchors: &Anchors,
    prefix: Option<&Path>,
) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let users = prefix.map(prefixes::wine_users).unwrap_or_default();
    let fallback = linux_user(&anchors.home);
    meta.files
        .iter()
        .map(|f| {
            let target = if portable::is_in_prefix(&f.path) {
                let prefix =
                    prefix.ok_or_else(|| anyhow::anyhow!("no Wine prefix for {} here", meta.game))?;
                portable::resolve_in_prefix(&f.path, prefix, &users, &fallback)
            } else {
                portable::from_portable(&f.path, anchors)
            }
            .ok_or_else(|| anyhow::anyhow!("no place for {} on this device", f.path))?;
            anyhow::ensure!(
                portable::is_allowed_target(&target, anchors, prefix),
                "refusing to write {}",
                target.display()
            );
            Ok((f.path.clone(), target))
        })
        .collect()
}

/// Writes `meta` (its package at `blob`) to the disk. The caller has taken a snapshot first.
pub fn write(
    dirs: &Dirs,
    scanner: &dyn Scanner,
    meta: &VersionMeta,
    blob: &Path,
    prefix: Option<&Path>,
) -> anyhow::Result<()> {
    let anchors = scanner.anchors();
    let contents = pack::unpack(blob, &meta.files)?;
    let places = targets(meta, &anchors, prefix)?;

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

    // Compared by real path: a file seen through a linked profile folder is the same file.
    let keep: BTreeSet<PathBuf> = places.iter().map(|(_, t)| real_path(t)).collect();
    let trash = dirs.trash().join(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S"),
        spielstand_core::slug(&meta.game)
    ));
    // Writing into the game's own prefix leaves saves in other prefixes alone: once the own
    // prefix holds the new state, they no longer count as the game's (see `save_files`).
    let own_target = prefix.filter(|p| {
        scanner
            .own_prefix(&meta.game)
            .is_some_and(|own| real_path(&own) == real_path(p))
    });
    let elsewhere =
        |f: &Path| own_target.is_some_and(|own| prefixes::prefix_of(f).is_some() && !in_prefix(f, own));
    let mut moved = BTreeSet::new();
    for extra in save_files(scanner, &meta.game) {
        let real = real_path(&extra);
        if keep.contains(&real) || elsewhere(&extra) || !moved.insert(real.clone()) {
            continue;
        }
        std::fs::create_dir_all(&trash)?;
        let name = real.to_string_lossy().replace(['/', '\\'], "_");
        if std::fs::rename(&real, trash.join(&name)).is_err() {
            std::fs::copy(&real, trash.join(&name))?;
            std::fs::remove_file(&real)?;
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
    prefix: Option<&Path>,
    previous: Option<(&VersionMeta, &Path)>,
) -> anyhow::Result<()> {
    match write(dirs, scanner, meta, blob, prefix) {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some((prev, prev_blob)) = previous
                && let Err(back) = write(dirs, scanner, prev, prev_blob, prefix)
            {
                anyhow::bail!("{e:#}; putting the previous state back also failed: {back:#}");
            }
            Err(e)
        }
    }
}
