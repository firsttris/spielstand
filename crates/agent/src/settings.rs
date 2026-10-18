//! What the server decides for this device (modes, game folders, prefix choices), kept on disk
//! so an agent that starts without the server still knows it, and where saves can be found.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use spielstand_protocol::{AgentSettings, SaveMode};

use crate::{
    config::Dirs,
    engine::{Scanner, normalize_title},
    prefixes::{self, PrefixrGame},
};

/// Where this agent looks for other programs' data; `None` turns a source off (tests).
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub prefixr_config: Option<PathBuf>,
    pub stellwerk_dir: Option<PathBuf>,
}

impl Sources {
    pub fn from_env() -> Self {
        Self {
            prefixr_config: prefixes::prefixr_config(),
            stellwerk_dir: crate::foreign::stellwerk_dir(),
        }
    }

    pub fn prefixr_games(&self) -> Option<Vec<PrefixrGame>> {
        prefixes::prefixr_games(self.prefixr_config.as_deref()?)
    }

    pub fn stellwerk_installed(&self) -> bool {
        self.stellwerk_dir
            .as_deref()
            .is_some_and(crate::foreign::stellwerk_installed)
    }
}

fn file(dirs: &Dirs) -> PathBuf {
    dirs.data.join("settings.json")
}

pub fn load(dirs: &Dirs) -> AgentSettings {
    std::fs::read_to_string(file(dirs))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(dirs: &Dirs, settings: &AgentSettings) -> anyhow::Result<()> {
    std::fs::create_dir_all(&dirs.data)?;
    let tmp = file(dirs).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings)?)?;
    std::fs::rename(tmp, file(dirs))?;
    Ok(())
}

pub fn mode_of(settings: &AgentSettings, game: &str) -> SaveMode {
    settings
        .game_modes
        .get(game)
        .copied()
        .unwrap_or(settings.default_mode)
}

/// A custom game this device proposed: only backed up here until it is taken over.
pub fn is_proposed(settings: &AgentSettings, game: &str) -> bool {
    settings.proposed_games.iter().any(|g| g == game)
}

/// A game the manifest lacks, defined as a custom game (not one that only extends a known one).
pub fn is_custom(settings: &AgentSettings, game: &str) -> bool {
    settings.custom_games.iter().any(|g| g.name == game && !g.extend)
}

pub fn game_folders(settings: &AgentSettings, home: &Path) -> Vec<PathBuf> {
    settings
        .game_folders
        .iter()
        .map(|f| prefixes::expand_home(f, home))
        .collect()
}

/// Every Wine prefix Prefixr knows, the ones chosen in the web UI and every prefix in the game
/// folders.
pub fn wine_prefixes(settings: &AgentSettings, sources: &Sources, home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = sources
        .prefixr_games()
        .unwrap_or_default()
        .into_iter()
        .map(|g| g.prefix)
        .chain(
            settings
                .prefix_choices
                .values()
                .map(|p| prefixes::expand_home(p, home)),
        )
        .filter(|p| prefixes::is_wine_prefix(p))
        .collect();
    for folder in game_folders(settings, home) {
        found.extend(prefixes::find_in(&folder, 3));
    }
    found.sort();
    found.dedup();
    found
}

/// The prefix chosen for `game` in the web UI, if it is a prefix on this device.
pub fn chosen_prefix(settings: &AgentSettings, game: &str, home: &Path) -> Option<PathBuf> {
    settings
        .prefix_choices
        .get(game)
        .map(|p| prefixes::expand_home(p, home))
        // For a Switch game the choice is the save folder of one of its emulators.
        .filter(|p| prefixes::is_wine_prefix(p) || crate::emulators::title_id(game).is_some())
}

/// The prefixes of the Prefixr games that are `game`: by the manifest's title, or the same
/// name written differently.
pub fn prefixr_prefixes(games: &[PrefixrGame], scanner: &dyn Scanner, game: &str) -> Vec<PathBuf> {
    let wanted = normalize_title(game);
    games
        .iter()
        .filter(|g| {
            scanner.find_title(&g.name).as_deref() == Some(game) || normalize_title(&g.name) == wanted
        })
        .map(|g| g.prefix.clone())
        .filter(|p| prefixes::is_wine_prefix(p))
        .collect()
}

/// Each game's assigned prefix (see [`prefixes::assigned`]), by normalized title.
fn own_prefixes(
    scanner: &dyn Scanner,
    settings: &AgentSettings,
    sources: &Sources,
    home: &Path,
) -> BTreeMap<String, PathBuf> {
    let library = sources.prefixr_games().unwrap_or_default();
    let games: std::collections::BTreeSet<String> = settings
        .prefix_choices
        .keys()
        .cloned()
        .chain(
            library
                .iter()
                .map(|g| scanner.find_title(&g.name).unwrap_or_else(|| g.name.clone())),
        )
        .collect();
    games
        .into_iter()
        .filter_map(|game| {
            let own = prefixes::assigned(
                chosen_prefix(settings, &game, home),
                &prefixr_prefixes(&library, scanner, &game),
            )?;
            Some((normalize_title(&game), own))
        })
        .collect()
}

/// Adds the custom games of `settings` to the scanner's manifest, points it at the prefixes and
/// game folders, and tells it which prefix is each game's own.
pub fn configure(scanner: &dyn Scanner, settings: &AgentSettings, sources: &Sources, home: &Path) {
    scanner.set_custom_games(&settings.custom_games);
    let own = own_prefixes(scanner, settings, sources, home);
    scanner.configure(
        wine_prefixes(settings, sources, home),
        game_folders(settings, home),
        own,
    );
}
