//! The flows: what happens before a game starts, after it ends, when a save file changes and
//! when the server pushes a new head. Every write goes through `restore_version`, which
//! snapshots first; in backup mode nothing but an explicit restore ever writes.

use std::time::Duration;

use async_trait::async_trait;
use spielstand_core::{
    decide::{Decision, Local, Remote},
    plan::{self, Known, SyncMode},
};
use spielstand_protocol::{GameProblem, Trigger, UploadStatus};

use crate::{
    agent::{Agent, sync_mode},
    disk,
    store::Base,
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

impl Agent {
    /// Loads version `id` onto the disk, keeping what was there as a version first.
    pub async fn restore_version(&self, game: &str, id: &str) -> anyhow::Result<()> {
        let _working = self.lock_game(game).await;
        self.load(game, id).await
    }

    /// [`Agent::restore_version`] for a flow that holds the game's lock already.
    async fn load(&self, game: &str, id: &str) -> anyhow::Result<()> {
        let meta = self.client.meta(id).await?;
        anyhow::ensure!(meta.game == game, "version {id} belongs to {}", meta.game);
        let prefix = match self.prefix_for(game, &meta) {
            Ok(p) => p,
            Err(problem) => {
                self.set_problem(game, Some(problem.clone()));
                anyhow::bail!("{game}: {problem:?}");
            }
        };
        let blob = self.blob_for(&meta).await?;
        let previous = self.fallback(game).await?;
        let (dirs, scanner, m, p) = (
            self.dirs.clone(),
            self.scanner.clone(),
            meta.clone(),
            prefix.clone(),
        );
        let written = tokio::task::spawn_blocking(move || {
            let prev = previous.as_ref().map(|(m, b)| (m, b.as_path()));
            disk::write_or_roll_back(&dirs, scanner.as_ref(), &m, &blob, p.as_deref(), prev)
        })
        .await?;
        if let Err(e) = written {
            self.set_problem(
                game,
                Some(GameProblem::Failed {
                    message: format!("{e:#}"),
                }),
            );
            return Err(e);
        }
        self.set_problem(game, None);
        self.places_changed.notify_one();
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

    /// Brings one game in line with the server, in the mode in effect for it.
    pub async fn sync_game(&self, game: &str, mode: Mode) -> anyhow::Result<Outcome> {
        let (save_mode, _) = self.effective_mode(game);
        self.run(game, mode, sync_mode(save_mode)).await
    }

    /// A save file changed: record it, never write (the game may be running right now).
    pub async fn observe(&self, game: &str) -> anyhow::Result<Outcome> {
        self.run(game, Mode::Background, SyncMode::Backup).await
    }

    async fn run(&self, game: &str, mode: Mode, save_mode: SyncMode) -> anyhow::Result<Outcome> {
        let _working = self.lock_game(game).await;
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
        let changed = disk.content_hash.is_some()
            && disk.content_hash.as_deref() != base.as_ref().map(|b| b.content_hash.as_str());
        let known = match (&disk.content_hash, changed) {
            (Some(hash), true) => self
                .client
                .known(game, hash, base.as_ref().map(|b| b.version.as_str()))
                .await
                .ok()
                .filter(|k| k.version.is_some()),
            _ => None,
        };
        let local = Local {
            base: base.as_ref().map(|b| b.version.as_str()),
            base_hash: base.as_ref().map(|b| b.content_hash.as_str()),
            disk_hash: disk.content_hash.as_deref(),
            base_is_open_branch: base
                .as_ref()
                .is_some_and(|b| heads.open_branches.contains(&b.version)),
        };
        let remote = Remote {
            head: head.map(|h| h.id.as_str()),
            head_hash: head.map(|h| h.content_hash.as_str()),
        };
        let planned = plan::plan(
            local,
            known.as_ref().and_then(|k| {
                Some(Known {
                    version: k.version.as_deref()?,
                    descends_from_base: k.descends_from_base,
                    is_open_branch: k.is_open_branch,
                })
            }),
            remote,
            save_mode,
        );

        if let (Some(version), Some(hash)) = (&planned.adopt, &disk.content_hash) {
            self.adopt(game, version, hash).await?;
            if let Some(lost) = &planned.overwritten {
                self.client.overwritten(game, lost, version).await?;
                self.ui
                    .info(
                        &format!("{game}: älterer Spielstand eingespielt"),
                        "Ein anderes Programm hat einen neueren Stand ersetzt. Der neuere ist gesichert; \
                         in der Web-UI lässt er sich mit einem Klick zurückholen.",
                    )
                    .await;
            }
        }

        match planned.decision {
            Decision::Nothing => Ok(Outcome::UpToDate),
            Decision::Adopt { version } => {
                let hash = head.map(|h| h.content_hash.clone()).unwrap_or_default();
                self.adopt(game, &version, &hash).await?;
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
                self.load(game, &version).await?;
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

    /// The disk holds `version` already: remember it, tell the server.
    async fn adopt(&self, game: &str, version: &str, hash: &str) -> anyhow::Result<()> {
        if self.store.base(game).await?.is_none() {
            self.places_changed.notify_one();
        }
        self.store
            .set_base(
                game,
                &Base {
                    version: version.to_string(),
                    content_hash: hash.to_string(),
                },
            )
            .await?;
        self.client.report_base(game, version).await
    }

    /// Acts on the answer to a conflict question; no answer keeps the local state.
    async fn resolve(&self, game: &str, choice: Choice, head: &str) -> anyhow::Result<Outcome> {
        match choice {
            Choice::Theirs => {
                self.load(game, head).await?;
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
        let _working = self.lock_game(game).await;
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
