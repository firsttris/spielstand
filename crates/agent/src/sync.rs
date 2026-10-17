//! The flows: what happens before a game starts, after it ends, and when the server pushes a
//! new head. Every write goes through `restore_version`, which snapshots first.

use std::{path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use spielstand_core::decide::{self, Decision};
use spielstand_protocol::{Trigger, UploadStatus, VersionMeta};

use crate::{
    client::Client,
    config::{AgentConfig, Dirs},
    disk,
    engine::Scanner,
    store::{Base, Store},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Mine,
    Theirs,
    NoAnswer,
}

/// How the agent talks to the person at the PC (desktop notifications in the real agent).
#[async_trait]
pub trait Ui: Send + Sync {
    async fn ask_conflict(&self, game: &str, other_device: &str, timeout: Duration) -> Choice;
    async fn warn_playing_elsewhere(&self, game: &str, device: &str);
    async fn info(&self, title: &str, body: &str);
}

/// A `Ui` that never answers; questions fall back to keeping the local state.
pub struct Silent;

#[async_trait]
impl Ui for Silent {
    async fn ask_conflict(&self, _: &str, _: &str, _: Duration) -> Choice {
        Choice::NoAnswer
    }
    async fn warn_playing_elsewhere(&self, _: &str, _: &str) {}
    async fn info(&self, _: &str, _: &str) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Right before the game starts: questions may be asked.
    Launch,
    /// In the background: nothing is asked, conflicts wait for the web UI.
    Background,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Uploaded,
    Loaded(String),
    Conflict { head: String },
    SkippedSteamCloud,
    Offline,
}

#[derive(Clone)]
pub struct Agent {
    pub dirs: Dirs,
    pub config: AgentConfig,
    pub store: Store,
    pub client: Client,
    pub scanner: Arc<dyn Scanner>,
    pub ui: Arc<dyn Ui>,
}

impl Agent {
    fn cached(&self, id: &str) -> PathBuf {
        self.dirs.cache().join(format!("{id}.tar.zst"))
    }

    /// Uploads the outbox in order; stops at the first failure (the server may be away).
    pub async fn flush(&self) -> anyhow::Result<Vec<(VersionMeta, UploadStatus)>> {
        let mut done = vec![];
        for pending in self.store.pending().await? {
            let outcome = self.client.upload(&pending.meta, &pending.blob).await?;
            tokio::fs::create_dir_all(self.dirs.cache()).await?;
            let _ = tokio::fs::rename(&pending.blob, self.cached(&pending.meta.id)).await;
            self.store.confirmed(&pending.meta.id).await?;
            done.push((pending.meta, outcome.status));
        }
        Ok(done)
    }

    async fn blob_for(&self, meta: &VersionMeta) -> anyhow::Result<PathBuf> {
        let path = self.cached(&meta.id);
        let fine = tokio::fs::try_exists(&path).await? && {
            let p = path.clone();
            tokio::task::spawn_blocking(move || spielstand_core::pack::sha256_file(&p)).await??
                == meta.blob_sha256
        };
        if !fine {
            self.client.download(&meta.id, &path).await?;
        }
        Ok(path)
    }

    async fn snapshot(
        &self,
        game: &str,
        trigger: Trigger,
        seconds: Option<u64>,
    ) -> anyhow::Result<Option<VersionMeta>> {
        let scanner = self.scanner.clone();
        disk::snapshot(
            &self.dirs,
            &self.store,
            scanner.as_ref(),
            &self.config.device_id,
            game,
            trigger,
            seconds,
        )
        .await
    }

    /// The state to put back if writing fails: a fresh snapshot, or the base it equals.
    async fn fallback(&self, game: &str) -> anyhow::Result<Option<(VersionMeta, PathBuf)>> {
        if let Some(meta) = self.snapshot(game, Trigger::BeforeRestore, None).await? {
            let blob = self.dirs.outbox().join(format!("{}.tar.zst", meta.id));
            return Ok(Some((meta, blob)));
        }
        let Some(base) = self.store.base(game).await? else {
            return Ok(None);
        };
        let pending = self.store.pending().await?;
        if let Some(p) = pending.into_iter().find(|p| p.meta.id == base.version) {
            return Ok(Some((p.meta, p.blob)));
        }
        let meta = self.client.meta(&base.version).await?;
        let blob = self.blob_for(&meta).await?;
        Ok(Some((meta, blob)))
    }

    /// Loads version `id` onto the disk, keeping what was there as a version first.
    pub async fn restore_version(&self, game: &str, id: &str) -> anyhow::Result<()> {
        let meta = self.client.meta(id).await?;
        anyhow::ensure!(meta.game == game, "version {id} belongs to {}", meta.game);
        let blob = self.blob_for(&meta).await?;
        let previous = self.fallback(game).await?;
        let (dirs, scanner, m) = (self.dirs.clone(), self.scanner.clone(), meta.clone());
        tokio::task::spawn_blocking(move || {
            let prev = previous.as_ref().map(|(m, b)| (m, b.as_path()));
            disk::write_or_roll_back(&dirs, scanner.as_ref(), &m, &blob, prev)
        })
        .await??;
        self.store
            .set_base(
                game,
                &Base {
                    version: meta.id.clone(),
                    content_hash: meta.content_hash.clone(),
                },
            )
            .await?;
        if let Err(e) = self.client.report_base(game, &meta.id).await {
            tracing::warn!("report base: {e:#}");
        }
        let _ = self.flush().await;
        Ok(())
    }

    /// Brings one game in line with the server, by the four cases of the safety concept.
    pub async fn sync_game(&self, game: &str, mode: Mode) -> anyhow::Result<Outcome> {
        let _ = self.flush().await;
        let Ok(heads) = self.client.heads().await else {
            // Without the server nothing is decided, but a changed disk is kept right away:
            // the game may overwrite it the moment it starts.
            self.snapshot(game, Trigger::CatchUp, None).await?;
            return Ok(Outcome::Offline);
        };
        let disk = {
            let scanner = self.scanner.clone();
            let g = game.to_string();
            tokio::task::spawn_blocking(move || disk::read(scanner.as_ref(), &g)).await??
        };
        let base = self.store.base(game).await?;
        let head = heads.heads.get(game);
        let local = decide::Local {
            base: base.as_ref().map(|b| b.version.as_str()),
            base_hash: base.as_ref().map(|b| b.content_hash.as_str()),
            disk_hash: disk.content_hash.as_deref(),
            base_is_open_branch: base
                .as_ref()
                .is_some_and(|b| heads.open_branches.contains(&b.version)),
        };
        let remote = decide::Remote {
            head: head.map(|h| h.id.as_str()),
            head_hash: head.map(|h| h.content_hash.as_str()),
        };

        match decide::decide(local, remote) {
            Decision::Nothing => Ok(Outcome::UpToDate),
            Decision::Adopt { version } => {
                let hash = head.map(|h| h.content_hash.clone()).unwrap_or_default();
                self.store
                    .set_base(
                        game,
                        &Base {
                            version: version.clone(),
                            content_hash: hash,
                        },
                    )
                    .await?;
                self.client.report_base(game, &version).await?;
                Ok(Outcome::UpToDate)
            }
            Decision::Upload => {
                self.snapshot(game, Trigger::CatchUp, None).await?;
                let results = self.flush().await?;
                Ok(match results.iter().find(|(m, _)| m.game == game) {
                    Some((_, UploadStatus::Branch)) => Outcome::Conflict {
                        head: head.map(|h| h.id.clone()).unwrap_or_default(),
                    },
                    _ => Outcome::Uploaded,
                })
            }
            Decision::FastForward { version } => {
                if self.scanner.steam_cloud(game) && !self.config.restore_steam_cloud_games {
                    return Ok(Outcome::SkippedSteamCloud);
                }
                self.restore_version(game, &version).await?;
                Ok(Outcome::Loaded(version))
            }
            Decision::Conflict { head: head_id } => {
                self.snapshot(game, Trigger::CatchUp, None).await?;
                let _ = self.flush().await;
                if mode == Mode::Background {
                    self.ui
                        .info(
                            &format!("{game}: auf zwei PCs gespielt"),
                            "Beide Stände sind gesichert. Wähle in der Web-UI, wie es weitergeht.",
                        )
                        .await;
                    return Ok(Outcome::Conflict { head: head_id });
                }
                let other = head.map(|h| h.device.clone()).unwrap_or_default();
                let choice = self
                    .ui
                    .ask_conflict(game, &other, Duration::from_secs(self.config.question_seconds))
                    .await;
                self.resolve(game, choice, &head_id).await
            }
        }
    }

    /// Acts on the answer to a conflict question; no answer keeps the local state.
    pub async fn resolve(&self, game: &str, choice: Choice, head: &str) -> anyhow::Result<Outcome> {
        match choice {
            Choice::Theirs => {
                self.restore_version(game, head).await?;
                Ok(Outcome::Loaded(head.to_string()))
            }
            Choice::Mine => {
                let base = self
                    .store
                    .base(game)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no local state"))?;
                let outcome = self.client.promote(&base.version, Some(head)).await?;
                if outcome.promoted {
                    Ok(Outcome::Uploaded)
                } else {
                    Ok(Outcome::Conflict {
                        head: outcome.head.unwrap_or_default(),
                    })
                }
            }
            Choice::NoAnswer => Ok(Outcome::Conflict {
                head: head.to_string(),
            }),
        }
    }

    /// Before a game starts. Never stops the start: at worst the local state is played.
    pub async fn prepare_launch(&self, game: &str) -> Outcome {
        if let Ok(heads) = self.client.heads().await
            && let Some(device) = heads.playing_elsewhere.get(game)
        {
            self.ui.warn_playing_elsewhere(game, device).await;
        }
        let _ = self.client.presence(Some(game)).await;
        match self.sync_game(game, Mode::Launch).await {
            Ok(Outcome::Offline) => {
                self.ui
                    .info(
                        &format!("{game}: Server nicht erreichbar"),
                        "Gespielt wird der Stand auf diesem PC; abgeglichen wird später.",
                    )
                    .await;
                Outcome::Offline
            }
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!("{game}: before start: {e:#}");
                self.ui
                    .info(&format!("{game}: Abgleich fehlgeschlagen"), &format!("{e:#}"))
                    .await;
                Outcome::Offline
            }
        }
    }

    /// After a game ended: snapshot, upload, tell the server the game stopped.
    pub async fn after_session(&self, game: &str, seconds: u64) -> anyhow::Result<Outcome> {
        let made = self.snapshot(game, Trigger::Session, Some(seconds)).await?;
        let _ = self.client.presence(None).await;
        if made.is_none() {
            return Ok(Outcome::UpToDate);
        }
        match self.flush().await {
            Ok(results) => Ok(match results.iter().find(|(m, _)| m.game == game) {
                Some((_, UploadStatus::Branch)) => {
                    self.ui
                        .info(
                            &format!("{game}: auf zwei PCs gespielt"),
                            "Beide Stände sind gesichert. Wähle in der Web-UI, wie es weitergeht.",
                        )
                        .await;
                    Outcome::Conflict { head: String::new() }
                }
                _ => Outcome::Uploaded,
            }),
            Err(_) => Ok(Outcome::Offline),
        }
    }
}
