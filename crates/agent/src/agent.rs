//! The agent's state: configuration, store, server client, scanner, settings, and the
//! helpers the flows in [`crate::sync`] share (outbox, packages, snapshots, the game's prefix).

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use spielstand_core::{plan::SyncMode, portable};
use spielstand_protocol::{
    AgentSettings, ForeignSync, GameProblem, SaveMode, Trigger, UploadStatus, VersionMeta,
};

use crate::{
    client::Client,
    config::{AgentConfig, Dirs},
    disk,
    engine::{Scanner, normalize_title},
    foreign, prefixes,
    settings::{self, Sources},
    store::Store,
    sync::Ui,
};

#[derive(Clone)]
pub struct Agent {
    pub dirs: Dirs,
    pub config: AgentConfig,
    pub store: Store,
    pub client: Client,
    pub scanner: Arc<dyn Scanner>,
    pub ui: Arc<dyn Ui>,
    pub settings: Arc<RwLock<AgentSettings>>,
    pub sources: Sources,
    /// Why a game could not be loaded here, until it can.
    pub problems: Arc<Mutex<BTreeMap<String, GameProblem>>>,
    /// Poked when saves appeared in a new place (a game's first state, a load): the file
    /// watcher sets itself up again.
    pub places_changed: Arc<tokio::sync::Notify>,
}

pub(crate) fn sync_mode(mode: SaveMode) -> SyncMode {
    match mode {
        SaveMode::Sync => SyncMode::Sync,
        SaveMode::Backup => SyncMode::Backup,
    }
}

impl Agent {
    pub fn new(
        dirs: Dirs,
        config: AgentConfig,
        store: Store,
        client: Client,
        scanner: Arc<dyn Scanner>,
        ui: Arc<dyn Ui>,
        sources: Sources,
    ) -> Self {
        let agent = Self {
            settings: Arc::new(RwLock::new(settings::load(&dirs))),
            dirs,
            config,
            store,
            client,
            scanner,
            ui,
            sources,
            problems: Arc::default(),
            places_changed: Arc::default(),
        };
        agent.configure_scanner();
        agent
    }

    pub(crate) fn cached(&self, id: &str) -> PathBuf {
        self.dirs.cache().join(format!("{id}.tar.zst"))
    }

    pub fn settings(&self) -> AgentSettings {
        self.settings.read().unwrap().clone()
    }

    pub(crate) fn home(&self) -> PathBuf {
        self.scanner.anchors().home
    }

    pub fn configure_scanner(&self) {
        let settings = self.settings();
        settings::configure(self.scanner.as_ref(), &settings, &self.sources, &self.home());
    }

    /// New settings from the server: kept on disk, the scanner looks in the new places.
    pub fn apply_settings(&self, settings: AgentSettings) {
        if let Err(e) = settings::save(&self.dirs, &settings) {
            tracing::warn!("keeping settings: {e:#}");
        }
        *self.settings.write().unwrap() = settings;
        self.configure_scanner();
    }

    /// Fetches the settings, if the server is there.
    pub async fn refresh_settings(&self) {
        if let Ok(settings) = self.client.settings().await {
            self.apply_settings(settings);
        }
    }

    pub fn configured_mode(&self, game: &str) -> SaveMode {
        settings::mode_of(&self.settings.read().unwrap(), game)
    }

    /// The mode in effect: while another program still syncs a game's saves, Spielstand only
    /// backs them up, so two programs never write the same file.
    pub fn effective_mode(&self, game: &str) -> (SaveMode, Option<ForeignSync>) {
        let files = self.scanner.save_files(game);
        let foreign = foreign::check(&files, self.sources.stellwerk_installed());
        let mode = match (self.configured_mode(game), &foreign) {
            (SaveMode::Sync, Some(f)) if !f.excluded => SaveMode::Backup,
            (mode, _) => mode,
        };
        (mode, foreign)
    }

    pub fn problem(&self, game: &str) -> Option<GameProblem> {
        self.problems.lock().unwrap().get(game).cloned()
    }

    pub(crate) fn set_problem(&self, game: &str, problem: Option<GameProblem>) {
        let mut problems = self.problems.lock().unwrap();
        match problem {
            Some(p) => problems.insert(game.to_string(), p),
            None => problems.remove(game),
        };
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

    pub(crate) async fn blob_for(&self, meta: &VersionMeta) -> anyhow::Result<PathBuf> {
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

    pub(crate) async fn snapshot(
        &self,
        game: &str,
        trigger: Trigger,
        seconds: Option<u64>,
    ) -> anyhow::Result<Option<VersionMeta>> {
        if self.store.base(game).await?.is_none() {
            self.places_changed.notify_one();
        }
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
    pub(crate) async fn fallback(&self, game: &str) -> anyhow::Result<Option<(VersionMeta, PathBuf)>> {
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

    /// The prefix of `game` on this device, for a version with saves inside a prefix.
    pub fn prefix_for(&self, game: &str, meta: &VersionMeta) -> Result<Option<PathBuf>, GameProblem> {
        if !meta.files.iter().any(|f| portable::is_in_prefix(&f.path)) {
            return Ok(None);
        }
        let existing = |p: PathBuf| prefixes::is_wine_prefix(&p).then_some(p);
        let current = disk::read(self.scanner.as_ref(), game)
            .ok()
            .and_then(|d| d.prefix);
        let chosen = settings::chosen_prefix(&self.settings(), game, &self.home());
        let anchors = self.scanner.anchors();
        let hinted = meta
            .prefix
            .as_deref()
            .and_then(|hint| portable::from_portable(hint, &anchors))
            .and_then(existing);
        let prefixr = settings::prefixr_prefixes(
            &self.sources.prefixr_games().unwrap_or_default(),
            self.scanner.as_ref(),
            game,
        );
        prefixes::choose(prefixes::Candidates {
            current,
            chosen,
            hinted,
            prefixr,
        })
        .map(Some)
    }

    /// A game is here when a launcher has it installed, Prefixr has it in its library or a
    /// prefix was chosen for it: only then does a state from another PC belong on this disk.
    pub fn installed_here(&self, game: &str, installed: &std::collections::BTreeSet<String>) -> bool {
        if installed.contains(game) || self.settings().prefix_choices.contains_key(game) {
            return true;
        }
        let wanted = normalize_title(game);
        self.sources.prefixr_games().unwrap_or_default().iter().any(|g| {
            prefixes::is_wine_prefix(&g.prefix)
                && (normalize_title(&g.name) == wanted
                    || self.scanner.find_title(&g.name).as_deref() == Some(game))
        })
    }

    /// The games to keep in step: the ones this device knows, and the ones with a state on the
    /// server that are installed here but have no saves yet (a new PC, a fresh install).
    pub async fn games_to_follow(&self) -> Vec<String> {
        let mut games = self.store.tracked_games().await.unwrap_or_default();
        if let Ok(heads) = self.client.heads().await {
            let scanner = self.scanner.clone();
            let installed = tokio::task::spawn_blocking(move || scanner.installed_games())
                .await
                .unwrap_or_default();
            games.extend(
                heads
                    .heads
                    .keys()
                    .filter(|g| self.installed_here(g, &installed))
                    .cloned(),
            );
        }
        games.sort();
        games.dedup();
        games
    }

    /// Install folders to recognise running games: the launchers' and Prefixr's (the folder of
    /// each game's `.exe`, under the manifest's name for it).
    pub fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        let mut dirs = self.scanner.install_dirs();
        for game in self.sources.prefixr_games().unwrap_or_default() {
            let Some(folder) = game.exe.parent().filter(|f| f.components().count() > 1) else {
                continue;
            };
            let name = self.scanner.find_title(&game.name).unwrap_or(game.name.clone());
            dirs.push((name, folder.to_path_buf()));
        }
        dirs
    }
}
