//! The only place that talks to Ludusavi. Everything else sees the `Scanner` trait, so tests and
//! the simulation run against a fake disk.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use ludusavi::{
    path::StrictPath,
    resource::{
        ResourceFile,
        cache::Cache,
        config::{self, Config, Integration, Root},
        manifest::{Manifest, Store},
    },
    scan::{Launchers, SteamShortcuts, TitleFinder, scan_game_for_backup},
};
use rayon::prelude::*;
use spielstand_core::portable::Anchors;
use spielstand_protocol::CustomGame;

pub trait Scanner: Send + Sync {
    fn anchors(&self) -> Anchors;
    /// The save files of `game` that exist on disk right now.
    fn save_files(&self, game: &str) -> Vec<PathBuf>;
    /// Every game that has saves on this disk.
    fn games_with_saves(&self) -> Vec<String>;
    /// Steam Cloud syncs this game's saves on its own.
    fn steam_cloud(&self, game: &str) -> bool;
    fn game_for_steam_app(&self, app_id: u32) -> Option<String>;
    /// Installed games and their folders, to recognise running processes.
    fn install_dirs(&self) -> Vec<(String, PathBuf)>;
    /// Games installed here, whether or not they have saves yet.
    fn installed_games(&self) -> std::collections::BTreeSet<String> {
        self.install_dirs().into_iter().map(|(g, _)| g).collect()
    }
    /// Wine prefixes to look into besides the ones launchers know (Prefixr, game folders), the
    /// game folders, which anchor portable paths as `<root:games>`, and each game's own prefix
    /// by normalized title (see `settings::configure`).
    fn configure(
        &self,
        wine_prefixes: Vec<PathBuf>,
        game_folders: Vec<PathBuf>,
        own_prefixes: BTreeMap<String, PathBuf>,
    );
    /// The prefix `game` is assigned to on this device, if any; for a Switch game, its save
    /// folder.
    fn own_prefix(&self, game: &str) -> Option<PathBuf>;
    /// The name to show for a game whose key is not one (a Switch game's title id).
    fn title(&self, _game: &str) -> Option<String> {
        None
    }
    /// For a Switch game: where its save is on this device, or why that is unclear.
    fn emulator_place(
        &self,
        _game: &str,
    ) -> Option<Result<crate::emulators::Place, spielstand_protocol::GameProblem>> {
        None
    }
    /// The Switch games with a save on this device.
    fn emulator_games(&self) -> Vec<String> {
        vec![]
    }
    /// The manifest's name for a title as another program spells it ("ELDEN RING" → "Elden Ring").
    fn find_title(&self, name: &str) -> Option<String>;
    /// The custom games from the settings, added to the manifest.
    fn set_custom_games(&self, _games: &[CustomGame]) {}
    /// The manifest itself (without custom games) has a game of this name.
    fn in_manifest(&self, game: &str) -> bool;
}

/// Lower-case letters and digits only: how titles from different programs are compared when
/// the manifest has no better answer.
pub fn normalize_title(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn store_key(store: ludusavi::resource::manifest::Store) -> String {
    let debug = format!("{store:?}");
    let mut chars = debug.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => debug,
    }
}

/// The manifest with the custom games added, and what is derived from it.
struct Known {
    manifest: Manifest,
    title_finder: TitleFinder,
    custom: Vec<CustomGame>,
}

/// A custom game as Ludusavi's configuration holds it (`src/resource/config.rs`, `CustomGame`);
/// `add_custom_game` in `src/resource/manifest.rs` overrides or extends a known game of the
/// same name.
fn to_ludusavi(game: &CustomGame) -> config::CustomGame {
    config::CustomGame {
        name: game.name.clone(),
        integration: if game.extend {
            Integration::Extend
        } else {
            Integration::Override
        },
        files: game.files.clone(),
        install_dir: game.install_dir.clone(),
        ..Default::default()
    }
}

pub struct Ludusavi {
    config: Config,
    /// The manifest as downloaded (with Ludusavi's own extensions), before custom games.
    base: Manifest,
    known: RwLock<Arc<Known>>,
    app_dir: StrictPath,
    steam_shortcuts: SteamShortcuts,
    launchers: Mutex<Option<Launchers>>,
    steam_ids: HashMap<u32, String>,
    wine_prefixes: RwLock<Vec<PathBuf>>,
    game_folders: RwLock<Vec<PathBuf>>,
    own_prefixes: RwLock<BTreeMap<String, PathBuf>>,
    /// Where the whole manifest is looked through for the service (see `in_background`); a
    /// command uses every core.
    pool: Option<rayon::ThreadPool>,
}

/// Threads the service looks through the whole manifest with.
const BACKGROUND_THREADS: usize = 2;

impl Ludusavi {
    /// Loads Ludusavi's data below `dir` (kept apart from a Ludusavi the user may also run),
    /// updates the manifest when it is older than a day, and finds the launcher roots.
    /// `SPIELSTAND_MANIFEST` names a manifest file to use instead (offline, tests).
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir)?;
        *ludusavi::prelude::CONFIG_DIR.lock().unwrap() = Some(dir.to_path_buf());
        let mut config = Config::load().unwrap_or_default();
        config.add_common_roots();
        let manifest = match std::env::var_os("SPIELSTAND_MANIFEST") {
            Some(file) => {
                let text = std::fs::read_to_string(&file)?;
                Manifest::load_from_string(&text).map_err(|e| anyhow::anyhow!("manifest: {e:?}"))?
            }
            None => {
                let mut cache = Cache::load().unwrap_or_default();
                if let Err(e) = Manifest::update_mut(&config, &mut cache, false) {
                    tracing::warn!("manifest update: {e:?}");
                }
                Manifest::load().map_err(|e| anyhow::anyhow!("manifest: {e:?}"))?
            }
        }
        .with_extensions(&config);
        Ok(Self::with(config, manifest, StrictPath::from(dir.to_path_buf())))
    }

    pub fn with(config: Config, manifest: Manifest, app_dir: StrictPath) -> Self {
        let title_finder = TitleFinder::new(&config, &manifest, Default::default());
        let steam_shortcuts = SteamShortcuts::scan(&title_finder);
        let steam_ids = manifest.map_steam_ids_to_names();
        let known = Known {
            manifest: manifest.clone(),
            title_finder,
            custom: vec![],
        };
        Self {
            config,
            base: manifest,
            known: RwLock::new(Arc::new(known)),
            app_dir,
            steam_shortcuts,
            launchers: Mutex::new(None),
            steam_ids,
            wine_prefixes: RwLock::default(),
            game_folders: RwLock::default(),
            own_prefixes: RwLock::default(),
            pool: None,
        }
    }

    /// For the service: looking through the whole manifest (at the start, after new settings
    /// and every 6 hours, while the PC is in use) takes a few threads, not every core. Ludusavi
    /// hashes every save file it finds.
    pub fn in_background(mut self) -> anyhow::Result<Self> {
        self.pool = Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(BACKGROUND_THREADS)
                .thread_name(|i| format!("scan-{i}"))
                .build()?,
        );
        Ok(self)
    }

    /// Ludusavi's roots plus one Wine root per extra prefix.
    fn roots(&self) -> Vec<Root> {
        let mut roots = self.config.expanded_roots();
        for prefix in self.wine_prefixes.read().unwrap().iter() {
            roots.push(Root::new(prefix.display().to_string(), Store::OtherWine));
        }
        roots
    }

    fn known(&self) -> Arc<Known> {
        self.known.read().unwrap().clone()
    }

    fn launchers(&self) -> Launchers {
        let mut cached = self.launchers.lock().unwrap();
        cached
            .get_or_insert_with(|| {
                let known = self.known();
                let games: Vec<String> = known.manifest.0.keys().cloned().collect();
                Launchers::scan(&self.roots(), &known.manifest, &games, &known.title_finder, None)
            })
            .clone()
    }

    /// Forget the launcher scan, e.g. after games were installed.
    pub fn refresh(&self) {
        *self.launchers.lock().unwrap() = None;
    }

    fn scan(&self, game: &str, launchers: &Launchers) -> Vec<PathBuf> {
        let known = self.known();
        let Some(entry) = known.manifest.0.get(game) else {
            return vec![];
        };
        let roots = self.roots();
        let info = scan_game_for_backup(
            entry,
            game,
            &roots,
            &self.app_dir,
            launchers,
            &self.config.backup.filter,
            None,
            &self.config.backup.toggled_paths,
            &self.config.backup.toggled_registry,
            None,
            &self.config.redirects,
            false,
            &self.steam_shortcuts,
            false,
        );
        let mut files: Vec<PathBuf> = info
            .found_files
            .iter()
            .filter(|(_, f)| !f.ignored && f.container.is_none())
            .filter_map(|(path, _)| path.as_std_path_buf().ok())
            .collect();
        files.sort();
        files
    }
}

impl Scanner for Ludusavi {
    fn anchors(&self) -> Anchors {
        let mut roots: Vec<(String, PathBuf)> = self
            .config
            .expanded_roots()
            .iter()
            .filter_map(|r| Some((store_key(r.store()), r.path().as_std_path_buf().ok()?)))
            .collect();
        roots.extend(
            self.game_folders
                .read()
                .unwrap()
                .iter()
                .map(|f| ("games".to_string(), f.clone())),
        );
        Anchors {
            home: dirs::home_dir().unwrap_or_default(),
            roots,
        }
    }

    fn save_files(&self, game: &str) -> Vec<PathBuf> {
        self.scan(game, &self.launchers())
    }

    fn games_with_saves(&self) -> Vec<String> {
        let all = || {
            let launchers = self.launchers();
            let known = self.known();
            let names: Vec<&String> = known.manifest.0.keys().collect();
            let mut found: Vec<String> = names
                .par_iter()
                .filter(|name| !self.scan(name, &launchers).is_empty())
                .map(|n| n.to_string())
                .collect();
            found.sort();
            found
        };
        match &self.pool {
            Some(pool) => pool.install(all),
            None => all(),
        }
    }

    fn steam_cloud(&self, game: &str) -> bool {
        self.known().manifest.0.get(game).is_some_and(|g| g.cloud.steam)
    }

    fn game_for_steam_app(&self, app_id: u32) -> Option<String> {
        self.steam_ids.get(&app_id).cloned()
    }

    fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        let launchers = self.launchers();
        let roots = self.roots();
        let mut out = BTreeMap::new();
        for name in self.known().manifest.0.keys() {
            for root in &roots {
                for found in launchers.get_game(root, name) {
                    if let Some(dir) = found.install_dir.as_ref().and_then(|d| d.as_std_path_buf().ok()) {
                        out.insert(dir, name.clone());
                    }
                }
            }
        }
        out.into_iter().map(|(dir, name)| (name, dir)).collect()
    }

    fn configure(
        &self,
        wine_prefixes: Vec<PathBuf>,
        game_folders: Vec<PathBuf>,
        own_prefixes: BTreeMap<String, PathBuf>,
    ) {
        let changed = *self.wine_prefixes.read().unwrap() != wine_prefixes;
        *self.wine_prefixes.write().unwrap() = wine_prefixes;
        *self.game_folders.write().unwrap() = game_folders;
        *self.own_prefixes.write().unwrap() = own_prefixes;
        if changed {
            self.refresh();
        }
    }

    fn own_prefix(&self, game: &str) -> Option<PathBuf> {
        self.own_prefixes
            .read()
            .unwrap()
            .get(&normalize_title(game))
            .cloned()
    }

    fn find_title(&self, name: &str) -> Option<String> {
        let known = self.known();
        known
            .title_finder
            .find_one_by_normalized_name(name)
            .or_else(|| known.title_finder.find_one_by_name(name))
    }

    fn set_custom_games(&self, games: &[CustomGame]) {
        if self.known().custom == games {
            return;
        }
        let mut manifest = self.base.clone();
        for game in games {
            manifest.add_custom_game(to_ludusavi(game));
        }
        let title_finder = TitleFinder::new(&self.config, &manifest, Default::default());
        *self.known.write().unwrap() = Arc::new(Known {
            manifest,
            title_finder,
            custom: games.to_vec(),
        });
        self.refresh();
    }

    fn in_manifest(&self, game: &str) -> bool {
        self.base.0.contains_key(game)
    }
}

/// A disk made of plain folders: `games[name]` lists the folders that hold its saves;
/// `profile_saves[name]` is a folder inside every Wine profile of every configured prefix
/// (`AppData/Roaming/Hades` → `<prefix>/drive_c/users/*/AppData/Roaming/Hades`), the way
/// Ludusavi looks into a Wine root.
#[derive(Default)]
pub struct FakeScanner {
    pub anchors: Anchors,
    pub games: BTreeMap<String, Vec<PathBuf>>,
    pub profile_saves: BTreeMap<String, String>,
    pub steam_cloud: Vec<String>,
    pub wine_prefixes: RwLock<Vec<PathBuf>>,
    pub game_folders: RwLock<Vec<PathBuf>>,
    pub own_prefixes: RwLock<BTreeMap<String, PathBuf>>,
    /// How long looking through the disk takes (a real one with many games takes minutes).
    pub scan_time: std::time::Duration,
}

impl FakeScanner {
    fn profile_dirs(&self, game: &str) -> Vec<PathBuf> {
        let Some(rel) = self.profile_saves.get(game) else {
            return vec![];
        };
        let mut out = vec![];
        for prefix in self.wine_prefixes.read().unwrap().iter() {
            let Ok(users) = std::fs::read_dir(prefix.join("drive_c/users")) else {
                continue;
            };
            for user in users.flatten() {
                out.push(user.path().join(rel));
            }
        }
        out
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

impl Scanner for FakeScanner {
    fn anchors(&self) -> Anchors {
        let mut anchors = self.anchors.clone();
        anchors.roots.extend(
            self.game_folders
                .read()
                .unwrap()
                .iter()
                .map(|f| ("games".to_string(), f.clone())),
        );
        anchors
    }
    fn save_files(&self, game: &str) -> Vec<PathBuf> {
        let mut out = vec![];
        for dir in self.games.get(game).into_iter().flatten() {
            walk(dir, &mut out);
        }
        for dir in self.profile_dirs(game) {
            walk(&dir, &mut out);
        }
        out.sort();
        out
    }
    fn games_with_saves(&self) -> Vec<String> {
        std::thread::sleep(self.scan_time);
        let names: std::collections::BTreeSet<&String> =
            self.games.keys().chain(self.profile_saves.keys()).collect();
        names
            .into_iter()
            .filter(|g| !self.save_files(g).is_empty())
            .cloned()
            .collect()
    }
    fn steam_cloud(&self, game: &str) -> bool {
        self.steam_cloud.iter().any(|g| g == game)
    }
    fn game_for_steam_app(&self, _app_id: u32) -> Option<String> {
        None
    }
    fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        vec![]
    }
    /// A fake disk lists the games installed on it.
    fn installed_games(&self) -> std::collections::BTreeSet<String> {
        self.games.keys().cloned().collect()
    }
    fn configure(
        &self,
        wine_prefixes: Vec<PathBuf>,
        game_folders: Vec<PathBuf>,
        own_prefixes: BTreeMap<String, PathBuf>,
    ) {
        *self.wine_prefixes.write().unwrap() = wine_prefixes;
        *self.game_folders.write().unwrap() = game_folders;
        *self.own_prefixes.write().unwrap() = own_prefixes;
    }
    fn own_prefix(&self, game: &str) -> Option<PathBuf> {
        self.own_prefixes
            .read()
            .unwrap()
            .get(&normalize_title(game))
            .cloned()
    }
    fn in_manifest(&self, game: &str) -> bool {
        self.games.contains_key(game) || self.profile_saves.contains_key(game)
    }
    fn find_title(&self, name: &str) -> Option<String> {
        let wanted = normalize_title(name);
        self.games
            .keys()
            .chain(self.profile_saves.keys())
            .find(|g| normalize_title(g) == wanted)
            .cloned()
    }
}
