//! Wine prefixes on this device: the ones Prefixr knows, the ones in the game folders set in the
//! web UI, and the rule that picks a game's prefix when a save has to be written into it.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use spielstand_core::portable;
use spielstand_protocol::GameProblem;

/// A folder is a Wine prefix when it has `drive_c` and Wine's registry next to it.
pub fn is_wine_prefix(dir: &Path) -> bool {
    dir.join("drive_c").is_dir() && (dir.join("system.reg").is_file() || dir.join("user.reg").is_file())
}

/// The prefix a file lies in, if any.
pub fn prefix_of(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?;
    let (prefix, _) = portable::split_at_drive_c(text)?;
    let prefix = PathBuf::from(prefix);
    is_wine_prefix(&prefix).then_some(prefix)
}

/// The profile folders in `drive_c/users` (`steamuser`, the Linux user name, `Public`).
pub fn wine_users(prefix: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(prefix.join("drive_c/users")) else {
        return vec![];
    };
    let mut users: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    users.sort();
    users
}

/// Every prefix below `folder`, a few levels deep (`Games/<game>/prefix`, `Games/<game>/pfx`,
/// `Games/<game>` itself). A prefix is not searched for further prefixes.
pub fn find_in(folder: &Path, depth: usize) -> Vec<PathBuf> {
    if is_wine_prefix(folder) {
        return vec![folder.to_path_buf()];
    }
    if depth == 0 {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .flat_map(|e| find_in(&e.path(), depth - 1))
        .collect();
    found.sort();
    found
}

/// `~/…` from the web UI, relative to this device's home.
pub fn expand_home(folder: &str, home: &Path) -> PathBuf {
    match folder.strip_prefix('~') {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(folder),
    }
}

/// A game in Prefixr's library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixrGame {
    pub name: String,
    pub prefix: PathBuf,
    /// The game's `.exe`; its folder tells a running game apart.
    pub exe: PathBuf,
}

#[derive(Deserialize)]
struct PrefixrConfig {
    #[serde(default)]
    games: Vec<PrefixrEntry>,
}

#[derive(Deserialize)]
struct PrefixrEntry {
    name: String,
    prefix_path: PathBuf,
    #[serde(default)]
    exe_path: PathBuf,
}

/// Where Prefixr keeps its library (`com.tristan.prefixr/config.json` in the config folder).
pub fn prefixr_config() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SPIELSTAND_PREFIXR_CONFIG") {
        return Some(PathBuf::from(path));
    }
    Some(dirs::config_dir()?.join("com.tristan.prefixr/config.json"))
}

/// The games of Prefixr's library; `None` when Prefixr is not set up here.
pub fn prefixr_games(config: &Path) -> Option<Vec<PrefixrGame>> {
    let text = std::fs::read_to_string(config).ok()?;
    let parsed: PrefixrConfig = serde_json::from_str(&text).ok()?;
    Some(
        parsed
            .games
            .into_iter()
            .map(|g| PrefixrGame {
                name: g.name,
                prefix: g.prefix_path,
                exe: g.exe_path,
            })
            .collect(),
    )
}

/// What is known on this device when a version of a game has to be written into a prefix.
#[derive(Clone, Debug, Default)]
pub struct Candidates {
    /// The prefix the game's saves are in right now.
    pub current: Option<PathBuf>,
    /// The choice made in the web UI.
    pub chosen: Option<PathBuf>,
    /// Where the version's own prefix would be on this device, if that folder is a prefix.
    pub hinted: Option<PathBuf>,
    /// Prefixes of Prefixr games with the same title.
    pub prefixr: Vec<PathBuf>,
}

/// The prefix a game is assigned to: the choice in the web UI, else the one prefix Prefixr
/// has for that title. An assignment beats where saves happen to lie: an old prefix the game
/// moved out of can still hold an old save.
pub fn assigned(chosen: Option<PathBuf>, prefixr: &[PathBuf]) -> Option<PathBuf> {
    chosen.or_else(|| {
        let mut distinct = prefixr.to_vec();
        distinct.sort();
        distinct.dedup();
        (distinct.len() == 1).then(|| distinct.remove(0))
    })
}

/// Picks the prefix for a game, in the order: its assigned prefix (see [`assigned`]), where its
/// saves are, the version's own place. More than one Prefixr candidate and nothing else to go
/// by is a question for the web UI, never a guess.
pub fn choose(c: Candidates) -> Result<PathBuf, GameProblem> {
    if let Some(p) = assigned(c.chosen, &c.prefixr).or(c.current).or(c.hinted) {
        return Ok(p);
    }
    let mut prefixr = c.prefixr;
    prefixr.sort();
    prefixr.dedup();
    match prefixr.len() {
        0 => Err(GameProblem::NoPrefix),
        _ => Err(GameProblem::SeveralPrefixes {
            candidates: prefixr.iter().map(|p| p.display().to_string()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn make_prefix(dir: &Path, users: &[&str]) {
        for u in users {
            std::fs::create_dir_all(dir.join("drive_c/users").join(u)).unwrap();
        }
        std::fs::create_dir_all(dir.join("drive_c")).unwrap();
        std::fs::write(dir.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
    }

    #[test]
    fn finds_prefixes_in_a_games_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let games = tmp.path().join("Games");
        make_prefix(&games.join("Hades/prefix"), &["steamuser"]);
        make_prefix(&games.join("Elden Ring/pfx"), &["steamuser"]);
        make_prefix(&games.join("Witcher"), &["tristan"]);
        std::fs::create_dir_all(games.join("NotAGame/data")).unwrap();
        std::fs::create_dir_all(games.join(".hidden/prefix/drive_c")).unwrap();
        assert_eq!(
            find_in(&games, 3),
            vec![
                games.join("Elden Ring/pfx"),
                games.join("Hades/prefix"),
                games.join("Witcher"),
            ]
        );
    }

    #[test]
    fn a_file_knows_its_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("Hades/prefix");
        make_prefix(&prefix, &["steamuser"]);
        let save = prefix.join("drive_c/users/steamuser/AppData/x.sav");
        assert_eq!(prefix_of(&save), Some(prefix.clone()));
        assert_eq!(
            prefix_of(&tmp.path().join("other/drive_c/x")),
            None,
            "not a prefix"
        );
        assert_eq!(wine_users(&prefix), vec!["steamuser".to_string()]);
    }

    #[test]
    fn reads_prefixr_library() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config.json");
        std::fs::write(
            &config,
            r#"{"runners_dir":"/x","prefixes":[{"path":"/g/Hades/prefix"}],
               "games":[{"id":"8f1c","name":"Hades","exe_path":"/g/Hades/Hades.exe",
                         "prefix_path":"/g/Hades/prefix","runner_id":"ge-proton","notes":""}]}"#,
        )
        .unwrap();
        assert_eq!(
            prefixr_games(&config),
            Some(vec![PrefixrGame {
                name: "Hades".into(),
                prefix: "/g/Hades/prefix".into(),
                exe: "/g/Hades/Hades.exe".into(),
            }])
        );
        assert_eq!(prefixr_games(&tmp.path().join("missing.json")), None);
    }

    #[test]
    fn the_prefix_is_chosen_in_a_fixed_order() {
        let p = |s: &str| Some(PathBuf::from(s));
        let all = Candidates {
            current: p("/current"),
            chosen: p("/chosen"),
            hinted: p("/hinted"),
            prefixr: vec!["/prefixr".into()],
        };
        assert_eq!(choose(all.clone()), Ok("/chosen".into()));
        assert_eq!(
            choose(Candidates {
                chosen: None,
                ..all.clone()
            }),
            Ok("/prefixr".into()),
            "the game's own Prefixr prefix, not an old one that still holds a save"
        );
        assert_eq!(
            choose(Candidates {
                chosen: None,
                prefixr: vec!["/a".into(), "/b".into()],
                ..all.clone()
            }),
            Ok("/current".into()),
            "Prefixr is unsure: where the saves are"
        );
        assert_eq!(
            choose(Candidates {
                current: None,
                chosen: None,
                prefixr: vec![],
                ..all.clone()
            }),
            Ok("/hinted".into())
        );
        assert_eq!(
            choose(Candidates {
                prefixr: vec!["/a".into(), "/a".into()],
                ..Default::default()
            }),
            Ok("/a".into())
        );
        assert_eq!(choose(Candidates::default()), Err(GameProblem::NoPrefix));
        assert_eq!(
            choose(Candidates {
                prefixr: vec!["/b".into(), "/a".into()],
                ..Default::default()
            }),
            Err(GameProblem::SeveralPrefixes {
                candidates: vec!["/a".into(), "/b".into()]
            })
        );
    }

    #[test]
    fn home_folders_from_the_web_ui_expand() {
        let home = Path::new("/home/deck");
        assert_eq!(expand_home("~/Games", home), PathBuf::from("/home/deck/Games"));
        assert_eq!(expand_home("~", home), PathBuf::from("/home/deck"));
        assert_eq!(
            expand_home("/run/media/SD/Games", home),
            PathBuf::from("/run/media/SD/Games")
        );
    }
}
