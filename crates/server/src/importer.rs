//! Takes over a Ludusavi backup folder: every backup becomes a version with its date, comment
//! and lock, differential backups resolved to full states. Imported versions never become a
//! head on their own; they wait in the timeline until someone loads them.

use spielstand_protocol::EventKind;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use ludusavi::{
    path::StrictPath,
    resource::config::ToggledPaths,
    scan::{ScanKind, layout::BackupLayout},
};
use spielstand_core::{
    pack::{self, Source},
    portable,
};
use spielstand_protocol::{ImportReport, Trigger, VersionMeta};

use crate::{
    app::AppState,
    blobs, db,
    error::ApiResult,
    versions::{self, NewVersion},
};

pub const IMPORT_DEVICE: &str = "ludusavi-import";

enum Origin {
    File(PathBuf),
    Zip { container: PathBuf, inner: String },
}

struct ImportedFile {
    original: String,
    origin: Origin,
}

struct ImportedBackup {
    name: String,
    when: DateTime<Utc>,
    comment: Option<String>,
    locked: bool,
    files: Vec<ImportedFile>,
}

struct ImportedGame {
    name: String,
    backups: Vec<ImportedBackup>,
}

/// `/home/<user>/…` and `/var/home/<user>/…` (Bazzite, Silverblue) become `<home>/…`.
pub fn portable_original(original: &str) -> String {
    let path = original.replace('\\', "/");
    portable_home(&path)
}

fn portable_home(path: &str) -> String {
    for prefix in ["/var/home/", "/home/"] {
        if let Some(rest) = path.strip_prefix(prefix)
            && let Some((_user, tail)) = rest.split_once('/')
        {
            return format!("<home>/{tail}");
        }
    }
    path.to_string()
}

/// A save inside a Wine prefix becomes `<prefix>/drive_c/…`, with the prefix's own folder as
/// the hint, the same form the agent uses; any other save is [`portable_original`].
pub fn portable_import(original: &str) -> (String, Option<String>) {
    let path = original.replace('\\', "/");
    match portable::split_at_drive_c(&path) {
        Some((prefix, rest)) => (
            format!("{}/{}", portable::PREFIX, portable::normalize_profile(rest)),
            Some(portable_home(prefix)),
        ),
        None => (portable_home(&path), None),
    }
}

fn read_layout(dir: &Path) -> Vec<ImportedGame> {
    let layout = BackupLayout::new(StrictPath::from(dir.to_path_buf()));
    layout
        .restorable_games()
        .into_iter()
        .map(|name| {
            let game = layout.game_layout(&name);
            let backups = game
                .restorable_backups_flattened()
                .into_iter()
                .map(|backup| {
                    // Ludusavi keys the map by StrictPath, which caches its canonical form inside;
                    // the map is only read here.
                    #[allow(clippy::mutable_key_type)]
                    let found = game.restorable_files(
                        &backup.id(),
                        ScanKind::Restore,
                        &[],
                        false,
                        &ToggledPaths::default(),
                    );
                    let files = found
                        .into_iter()
                        .filter_map(|(key, scanned)| {
                            let original = scanned.original_path?.render();
                            let origin = match scanned.container {
                                Some(container) => Origin::Zip {
                                    container: container.as_std_path_buf().ok()?,
                                    inner: key.raw().to_string(),
                                },
                                None => Origin::File(key.as_std_path_buf().ok()?),
                            };
                            Some(ImportedFile { original, origin })
                        })
                        .collect();
                    ImportedBackup {
                        name: backup.name().to_string(),
                        when: *backup.when(),
                        comment: backup.comment().cloned(),
                        locked: backup.locked(),
                        files,
                    }
                })
                .collect();
            ImportedGame { name, backups }
        })
        .collect()
}

fn materialize(file: &ImportedFile, target: &Path) -> anyhow::Result<()> {
    match &file.origin {
        Origin::File(path) => {
            std::fs::copy(path, target)?;
        }
        Origin::Zip { container, inner } => {
            let mut archive = zip::ZipArchive::new(std::fs::File::open(container)?)?;
            let mut entry = archive.by_name(inner)?;
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            std::fs::write(target, bytes)?;
        }
    }
    Ok(())
}

/// Stable ids, so importing the same folder twice adds nothing.
fn version_id(game: &str, backup: &ImportedBackup) -> String {
    let tag = pack::sha256_hex(format!("{game}\0{}", backup.name).as_bytes());
    format!("{}-imp{}", backup.when.format("%Y%m%dT%H%M%S%3fZ"), &tag[..9])
}

fn pack_backup(
    game: &str,
    backup: &ImportedBackup,
    parent: Option<String>,
    work: &Path,
) -> anyhow::Result<(VersionMeta, PathBuf)> {
    let dir = tempfile::tempdir_in(work)?;
    let mut sources = Vec::new();
    let mut prefix = None;
    for (n, file) in backup.files.iter().enumerate() {
        let target = dir.path().join(format!("f{n}"));
        materialize(file, &target)?;
        let (portable, hint) = portable_import(&file.original);
        prefix = prefix.or(hint);
        sources.push(Source {
            portable,
            path: target,
        });
    }
    let out = work.join(format!("{}.tar.zst", uuid::Uuid::new_v4()));
    let packed = pack::pack(&sources, &out)?;
    let meta = VersionMeta {
        title: None,
        id: version_id(game, backup),
        game: game.to_string(),
        parent,
        device: IMPORT_DEVICE.to_string(),
        created_at: backup.when,
        trigger: Trigger::Import,
        session_seconds: None,
        content_hash: packed.content_hash,
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix,
    };
    Ok((meta, out))
}

pub async fn import_dir(state: &AppState, dir: &Path) -> anyhow::Result<ImportReport> {
    let dir_owned = dir.to_path_buf();
    let games = tokio::task::spawn_blocking(move || read_layout(&dir_owned)).await?;
    sqlx::query("INSERT OR IGNORE INTO devices (id, name, created_at) VALUES (?, 'Ludusavi-Import', ?)")
        .bind(IMPORT_DEVICE)
        .bind(db::now())
        .execute(&state.db)
        .await?;
    let work = state.settings.tmp_dir();
    tokio::fs::create_dir_all(&work).await?;
    let mut report = ImportReport::default();
    for game in games {
        let mut backups = game.backups;
        backups.sort_by_key(|b| b.when);
        let mut parent = None;
        let mut added = 0;
        for backup in backups {
            if backup.files.is_empty() {
                report.skipped += 1;
                continue;
            }
            let (game_name, work, prev) = (game.name.clone(), work.clone(), parent.clone());
            let (packed, backup) = tokio::task::spawn_blocking(move || {
                let packed = pack_backup(&game_name, &backup, prev, &work);
                (packed, backup)
            })
            .await?;
            match packed {
                Ok((meta, blob)) => {
                    let sha = meta.blob_sha256.clone();
                    let id = meta.id.clone();
                    blobs::store_file(&state.settings.blobs_dir(), &blob, &sha).await?;
                    let _ = tokio::fs::remove_file(&blob).await;
                    let label = backup.comment.clone().filter(|c| !c.trim().is_empty());
                    let new = NewVersion {
                        meta,
                        pinned: backup.locked,
                        label,
                    };
                    if versions::insert_external(state, new, false).await? {
                        added += 1;
                    } else {
                        report.skipped += 1;
                    }
                    parent = Some(id);
                }
                Err(e) => report
                    .errors
                    .push(format!("{} / {}: {e:#}", game.name, backup.name)),
            }
        }
        if added > 0 {
            report.games += 1;
            report.versions += added;
            db::event(
                &state.db,
                Some(IMPORT_DEVICE),
                Some(&game.name),
                EventKind::Imported,
            )
            .await?;
        }
    }
    state.mirror_wake.notify_one();
    Ok(report)
}

/// A local folder, or `remote:path` that rclone copies to the server first (read only).
pub async fn import_source(state: &AppState, source: &str) -> ApiResult<ImportReport> {
    let source = source.trim();
    let is_remote = !source.starts_with('/') && source.contains(':');
    if !is_remote {
        return Ok(import_dir(state, Path::new(source)).await?);
    }
    let target = state
        .settings
        .data_dir
        .join("import")
        .join(Utc::now().format("%Y%m%d%H%M%S").to_string());
    tokio::fs::create_dir_all(&target).await?;
    let out = tokio::process::Command::new(&state.settings.rclone)
        .args(["copy", source, &target.to_string_lossy()])
        .output()
        .await?;
    if !out.status.success() {
        return Err(crate::error::AppError::BadRequest(format!(
            "rclone: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let report = import_dir(state, &target).await?;
    let _ = tokio::fs::remove_dir_all(&target).await;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_paths_become_portable() {
        assert_eq!(
            portable_original("/home/tris/.config/G/s.dat"),
            "<home>/.config/G/s.dat"
        );
        assert_eq!(
            portable_original("/var/home/tris/.local/share/x"),
            "<home>/.local/share/x"
        );
        assert_eq!(
            portable_original("C:/Users/t/Documents/x"),
            "C:/Users/t/Documents/x"
        );
    }

    #[test]
    fn saves_in_a_prefix_get_the_prefix_form_with_a_hint() {
        assert_eq!(
            portable_import("/home/tris/Games/Hades/prefix/drive_c/users/steamuser/AppData/x.sav"),
            (
                "<prefix>/drive_c/users/<user>/AppData/x.sav".to_string(),
                Some("<home>/Games/Hades/prefix".to_string())
            )
        );
        assert_eq!(
            portable_import("/home/tris/.config/G/s.dat"),
            ("<home>/.config/G/s.dat".to_string(), None)
        );
    }
}
