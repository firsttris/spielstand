//! Stable snapshot attempts and bounded retries. The caller owns the per-game lock.

use std::{fmt, path::PathBuf, time::Duration};

use chrono::Utc;
use spielstand_core::{
    pack::{self, Fingerprint, PackError, Packed},
    portable,
};
use spielstand_protocol::{Trigger, VersionMeta};

use crate::{
    config::Dirs,
    disk::{self, Found},
    engine::Scanner,
    store::{Base, Store},
};

#[derive(Debug)]
pub struct UnstableSnapshot {
    pub(crate) game: String,
}

impl fmt::Display for UnstableSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: save files changed during backup; try again later",
            self.game
        )
    }
}

impl std::error::Error for UnstableSnapshot {}

#[derive(Clone, Copy)]
enum Phase {
    Read,
    Packed,
}

struct Request<'a> {
    dirs: &'a Dirs,
    store: &'a Store,
    scanner: &'a dyn Scanner,
    device: &'a str,
    game: &'a str,
    trigger: Trigger,
    seconds: Option<u64>,
}

struct Captured {
    packed: Packed,
    blob: tempfile::TempPath,
    prefix: Option<PathBuf>,
}

fn verify(scanner: &dyn Scanner, game: &str, before: &Found, stamps: &[Fingerprint]) -> anyhow::Result<()> {
    pack::check_sources(&before.sources, stamps)?;
    let after = disk::fresh(scanner, game);
    let mapping = |found: &Found| {
        found
            .sources
            .iter()
            .map(|s| (s.portable.clone(), s.path.clone()))
            .collect::<std::collections::BTreeSet<_>>()
    };
    // Scanner order is irrelevant; portable mappings and the selected prefix are not.
    if mapping(before) != mapping(&after) || before.prefix != after.prefix {
        return Err(PackError::SourceChanged("file set".into()).into());
    }
    pack::check_sources(&before.sources, stamps)?;
    Ok(())
}

async fn attempt(
    request: &Request<'_>,
    base: Option<&Base>,
    mut hook: impl FnMut(Phase),
) -> anyhow::Result<Option<Captured>> {
    let before = disk::fresh(request.scanner, request.game);
    let stamps = pack::fingerprints(&before.sources)?;
    let files = pack::describe(&before.sources);
    hook(Phase::Read);
    verify(request.scanner, request.game, &before, &stamps)?;
    let files = files?;
    if files.is_empty() || base.is_some_and(|b| b.content_hash == pack::content_hash(&files)) {
        return Ok(None);
    }
    tokio::fs::create_dir_all(request.dirs.outbox()).await?;
    let tmp = tempfile::NamedTempFile::new_in(request.dirs.outbox())?.into_temp_path();
    let sources = before.sources.clone();
    let (packed, blob) = tokio::task::spawn_blocking(move || {
        // Keep ownership of the temporary path in the blocking task so cancellation cannot
        // leave an archive behind after its future has been dropped.
        pack::pack(&sources, &tmp).map(|packed| (packed, tmp))
    })
    .await??;
    hook(Phase::Packed);
    verify(request.scanner, request.game, &before, &stamps)?;
    if base.is_some_and(|b| b.content_hash == packed.content_hash) {
        return Ok(None);
    }
    Ok(Some(Captured {
        packed,
        blob,
        prefix: before.prefix,
    }))
}

/// Packs the disk into the outbox and makes it the base. `None` means no saves or unchanged
/// content; exhausted instability is an error so a required safety snapshot cannot be skipped.
pub async fn snapshot(
    dirs: &Dirs,
    store: &Store,
    scanner: &dyn Scanner,
    device: &str,
    game: &str,
    trigger: Trigger,
    session_seconds: Option<u64>,
) -> anyhow::Result<Option<VersionMeta>> {
    take(
        Request {
            dirs,
            store,
            scanner,
            device,
            game,
            trigger,
            seconds: session_seconds,
        },
        |_, _| {},
    )
    .await
}

async fn take(
    request: Request<'_>,
    mut hook: impl FnMut(usize, Phase),
) -> anyhow::Result<Option<VersionMeta>> {
    let base = request.store.base(request.game).await?;
    for n in 0..3 {
        if n > 0 {
            tokio::time::sleep(Duration::from_millis(250 * n as u64)).await;
        }
        match attempt(&request, base.as_ref(), |phase| hook(n, phase)).await {
            Ok(Some(capture)) => return record(&request, base, capture).await.map(Some),
            Ok(None) => return Ok(None),
            Err(e) if matches!(e.downcast_ref::<PackError>(), Some(PackError::SourceChanged(_))) => {
                tracing::debug!(game = request.game, attempt = n + 1, "snapshot source changed");
            }
            Err(e) => return Err(e),
        }
    }
    Err(UnstableSnapshot {
        game: request.game.into(),
    }
    .into())
}

async fn record(request: &Request<'_>, base: Option<Base>, capture: Captured) -> anyhow::Result<VersionMeta> {
    let Captured { packed, blob, prefix } = capture;
    let now = Utc::now();
    let id = spielstand_core::new_version_id(now);
    let prefix_hint = prefix
        .as_deref()
        .filter(|_| crate::emulators::title_id(request.game).is_none())
        .map(|p| portable::to_portable(p, &request.scanner.anchors()));
    let meta = VersionMeta {
        id: id.clone(),
        game: request.game.into(),
        parent: base.map(|b| b.version),
        device: request.device.into(),
        created_at: now,
        trigger: request.trigger,
        session_seconds: request.seconds,
        content_hash: packed.content_hash.clone(),
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix: prefix_hint,
        title: request.scanner.title(request.game),
    };
    let target = request.dirs.outbox().join(format!("{id}.tar.zst"));
    blob.persist(&target)?;
    if let Err(e) = request.store.enqueue(&meta, &target).await {
        let _ = tokio::fs::remove_file(&target).await;
        return Err(e);
    }
    request
        .store
        .set_base(
            request.game,
            &Base {
                version: id,
                content_hash: packed.content_hash,
            },
        )
        .await?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::FakeScanner;
    use spielstand_core::portable::Anchors;

    struct Fixture {
        _dir: tempfile::TempDir,
        dirs: Dirs,
        store: Store,
        scanner: FakeScanner,
        save: PathBuf,
    }

    impl Fixture {
        async fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let save = home.join("game/save");
            std::fs::create_dir_all(save.parent().unwrap()).unwrap();
            std::fs::write(&save, "original").unwrap();
            let dirs = Dirs::under(dir.path());
            let store = Store::open(&dirs.db()).await.unwrap();
            let scanner = FakeScanner {
                anchors: Anchors { home, roots: vec![] },
                games: [("G".into(), vec![save.parent().unwrap().into()])].into(),
                ..Default::default()
            };
            Self {
                _dir: dir,
                dirs,
                store,
                scanner,
                save,
            }
        }

        fn request(&self) -> Request<'_> {
            Request {
                dirs: &self.dirs,
                store: &self.store,
                scanner: &self.scanner,
                device: "test",
                game: "G",
                trigger: Trigger::Manual,
                seconds: None,
            }
        }

        fn archive_count(&self) -> usize {
            std::fs::read_dir(self.dirs.outbox())
                .map(|r| r.count())
                .unwrap_or(0)
        }
    }

    #[tokio::test]
    async fn an_unstable_attempt_retries_and_records_only_the_stable_capture() {
        let f = Fixture::new().await;
        let mut attempts = vec![];
        let meta = take(f.request(), |n, phase| {
            if matches!(phase, Phase::Read) {
                attempts.push(n);
                if n == 0 {
                    std::fs::write(&f.save, "changed").unwrap();
                }
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(attempts, vec![0, 1]);
        assert_eq!(f.store.pending().await.unwrap().len(), 1);
        assert_eq!(f.archive_count(), 1);
        let pending = f.store.pending().await.unwrap().remove(0);
        let bytes = pack::unpack(&pending.blob, &meta.files).unwrap();
        assert_eq!(bytes.values().next().unwrap(), b"changed");
        assert_eq!(f.store.base("G").await.unwrap().unwrap().version, meta.id);
        assert!(
            snapshot(&f.dirs, &f.store, &f.scanner, "test", "G", Trigger::Manual, None)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(f.archive_count(), 1);
    }

    #[tokio::test]
    async fn exhausted_captures_preserve_the_base_and_outbox() {
        for phase_to_change in [Phase::Read, Phase::Packed] {
            let f = Fixture::new().await;
            let base = Base {
                version: "existing".into(),
                content_hash: "other".into(),
            };
            f.store.set_base("G", &base).await.unwrap();
            let mut attempts = 0;
            let error = take(f.request(), |n, phase| {
                if std::mem::discriminant(&phase) == std::mem::discriminant(&phase_to_change) {
                    attempts += 1;
                    std::fs::write(&f.save, format!("changed {n}")).unwrap();
                }
            })
            .await
            .unwrap_err();
            assert!(error.downcast_ref::<UnstableSnapshot>().is_some());
            assert_eq!(attempts, 3);
            assert_eq!(f.store.base("G").await.unwrap(), Some(base));
            assert!(f.store.pending().await.unwrap().is_empty());
            assert_eq!(f.archive_count(), 0);
        }
    }

    #[tokio::test]
    async fn additions_and_removals_retry_the_complete_file_set() {
        for remove in [false, true] {
            let f = Fixture::new().await;
            let other = f.save.with_file_name("other");
            if remove {
                std::fs::write(&other, "other").unwrap();
            }
            let mut attempts = vec![];
            let meta = take(f.request(), |n, phase| {
                if matches!(phase, Phase::Packed) {
                    attempts.push(n);
                    if n == 0 {
                        if remove {
                            std::fs::remove_file(&other).unwrap();
                        } else {
                            std::fs::write(&other, "other").unwrap();
                        }
                    }
                }
            })
            .await
            .unwrap()
            .unwrap();
            assert_eq!(attempts, vec![0, 1]);
            assert_eq!(meta.files.len(), if remove { 1 } else { 2 });
            assert_eq!(f.archive_count(), 1);
        }
    }

    #[tokio::test]
    async fn discovery_order_is_ignored_but_mapping_changes_are_rejected() {
        let f = Fixture::new().await;
        std::fs::write(f.save.with_file_name("other"), "other").unwrap();
        let mut before = disk::fresh(&f.scanner, "G");
        before.sources.reverse();
        let stamps = pack::fingerprints(&before.sources).unwrap();
        verify(&f.scanner, "G", &before, &stamps).unwrap();
        before.sources[0].portable = "<home>/different".into();
        let error = verify(&f.scanner, "G", &before, &stamps).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<PackError>(),
            Some(PackError::SourceChanged(_))
        ));
    }

    #[tokio::test]
    async fn unrelated_io_errors_are_not_retried() {
        let f = Fixture::new().await;
        std::fs::create_dir_all(f.dirs.outbox().parent().unwrap()).unwrap();
        std::fs::write(f.dirs.outbox(), "not a directory").unwrap();
        let mut attempts = 0;
        let error = take(f.request(), |_, phase| {
            if matches!(phase, Phase::Read) {
                attempts += 1;
            }
        })
        .await
        .unwrap_err();
        assert!(error.downcast_ref::<UnstableSnapshot>().is_none());
        assert_eq!(attempts, 1);
        assert!(f.store.pending().await.unwrap().is_empty());
        assert!(f.store.base("G").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unchanged_shortcuts_are_rechecked_and_empty_games_return_none() {
        let f = Fixture::new().await;
        snapshot(&f.dirs, &f.store, &f.scanner, "test", "G", Trigger::Manual, None)
            .await
            .unwrap();
        let mut attempts = vec![];
        let meta = take(f.request(), |n, phase| {
            if matches!(phase, Phase::Read) {
                attempts.push(n);
                if n == 0 {
                    std::fs::write(&f.save, "new state").unwrap();
                }
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(attempts, vec![0, 1]);
        assert_eq!(meta.files[0].sha256, pack::sha256_hex(b"new state"));
        std::fs::remove_file(&f.save).unwrap();
        assert!(take(f.request(), |_, _| {}).await.unwrap().is_none());
    }
}
