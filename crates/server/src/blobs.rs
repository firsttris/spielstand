//! Packages on disk, addressed by their SHA-256. Written to a temporary file first, checked,
//! then moved into place, so a half-received upload never looks like a version.

use std::path::{Path, PathBuf};

pub fn path(blobs_dir: &Path, sha: &str) -> PathBuf {
    let prefix = sha.get(..2).unwrap_or("00");
    blobs_dir.join(prefix).join(format!("{sha}.tar.zst"))
}

pub struct Received {
    pub tmp: tempfile::TempPath,
    pub sha256: String,
    pub size: u64,
}

/// Moves a received package into the store; an existing identical package is kept.
pub async fn store(blobs_dir: &Path, received: Received) -> anyhow::Result<PathBuf> {
    let target = path(blobs_dir, &received.sha256);
    if tokio::fs::try_exists(&target).await? {
        return Ok(target);
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    received.tmp.persist(&target)?;
    Ok(target)
}

pub async fn store_file(blobs_dir: &Path, file: &Path, sha: &str) -> anyhow::Result<PathBuf> {
    let target = path(blobs_dir, sha);
    if !tokio::fs::try_exists(&target).await? {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(file, &target).await?;
    }
    Ok(target)
}
