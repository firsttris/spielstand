//! A scanner that adds the Switch emulators' games to another one (Ludusavi in the agent, a
//! fake disk in tests): Switch keys are answered here, everything else by the inner scanner.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use spielstand_core::portable::Anchors;
use spielstand_protocol::GameProblem;

use super::{Install, Place, Xdg, files_in, games, installs, name, place, title_id};
use crate::engine::{Scanner, normalize_title};

pub struct WithEmulators {
    inner: Arc<dyn Scanner>,
    xdg: Xdg,
    installs: RwLock<Vec<Install>>,
    /// Each game's own place by normalized title, from the settings (choices in the web UI).
    chosen: RwLock<BTreeMap<String, PathBuf>>,
}

impl WithEmulators {
    pub fn new(inner: Arc<dyn Scanner>, xdg: Xdg) -> Self {
        let found = installs(&xdg);
        Self {
            inner,
            xdg,
            installs: RwLock::new(found),
            chosen: RwLock::default(),
        }
    }

    fn installs(&self) -> Vec<Install> {
        self.installs.read().unwrap().clone()
    }

    fn place_of(&self, title: &str, game: &str) -> Result<Place, GameProblem> {
        let chosen = self.chosen.read().unwrap().get(&normalize_title(game)).cloned();
        place(&self.installs(), title, chosen.as_deref())
    }
}

impl Scanner for WithEmulators {
    fn anchors(&self) -> Anchors {
        self.inner.anchors()
    }

    fn save_files(&self, game: &str) -> Vec<PathBuf> {
        match title_id(game) {
            Some(title) => self
                .place_of(title, game)
                .map(|p| files_in(&p.dir))
                .unwrap_or_default(),
            None => self.inner.save_files(game),
        }
    }

    fn games_with_saves(&self) -> Vec<String> {
        let mut all = self.inner.games_with_saves();
        all.extend(games(&self.installs()));
        all
    }

    fn steam_cloud(&self, game: &str) -> bool {
        title_id(game).is_none() && self.inner.steam_cloud(game)
    }

    fn game_for_steam_app(&self, app_id: u32) -> Option<String> {
        self.inner.game_for_steam_app(app_id)
    }

    fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        self.inner.install_dirs()
    }

    fn installed_games(&self) -> BTreeSet<String> {
        let mut all = self.inner.installed_games();
        all.extend(games(&self.installs()));
        all
    }

    fn configure(
        &self,
        wine_prefixes: Vec<PathBuf>,
        game_folders: Vec<PathBuf>,
        own_prefixes: BTreeMap<String, PathBuf>,
    ) {
        *self.installs.write().unwrap() = installs(&self.xdg);
        *self.chosen.write().unwrap() = own_prefixes.clone();
        self.inner.configure(wine_prefixes, game_folders, own_prefixes);
    }

    fn own_prefix(&self, game: &str) -> Option<PathBuf> {
        match title_id(game) {
            Some(title) => self.place_of(title, game).ok().map(|p| p.dir),
            None => self.inner.own_prefix(game),
        }
    }

    fn find_title(&self, name: &str) -> Option<String> {
        self.inner.find_title(name)
    }

    fn set_custom_games(&self, games: &[spielstand_protocol::CustomGame]) {
        self.inner.set_custom_games(games);
    }

    fn in_manifest(&self, game: &str) -> bool {
        title_id(game).is_some() || self.inner.in_manifest(game)
    }

    fn title(&self, game: &str) -> Option<String> {
        title_id(game).and_then(|t| name(&self.installs(), t))
    }

    fn emulator_place(&self, game: &str) -> Option<Result<Place, GameProblem>> {
        let title = title_id(game)?;
        Some(self.place_of(title, game))
    }

    fn emulator_games(&self) -> Vec<String> {
        games(&self.installs())
    }
}
