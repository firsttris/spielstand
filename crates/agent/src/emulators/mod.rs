//! Nintendo Switch emulators. Every title becomes a game of its own, keyed by its title id
//! (`Switch 0100F2C0115B6000`), whatever emulator holds it: the save files are the same for
//! the yuzu family and Ryujinx, so a state from Eden on one PC loads into Ryujinx on another.
//! Its files are stored relative to the title's save folder (`<switch>/…`); which folder that
//! is, with this device's emulator and profile, is decided here.

pub mod ryujinx;
pub mod scanner;
pub mod yuzu;

use std::path::{Path, PathBuf};

use spielstand_protocol::GameProblem;

const KEY_PREFIX: &str = "Switch ";

/// Emulator programs; while one runs, its games may be open and nothing is written to them.
pub const PROCESSES: &[&str] = &[
    "yuzu",
    "yuzu-cmd",
    "eden",
    "eden-cli",
    "citron",
    "citron-cmd",
    "citron.bin",
    "suyu",
    "suyu-cmd",
    "sudachi",
    "sudachi-cmd",
    "Ryujinx",
    "Ryujinx.Ava",
    "Ryujinx.Headless.SDL2",
];

pub fn key(title: &str) -> String {
    format!("{KEY_PREFIX}{title}")
}

/// The title id of a Switch game's key.
pub fn title_id(game: &str) -> Option<&str> {
    game.strip_prefix(KEY_PREFIX).filter(|t| is_title_id(t))
}

/// 16 upper-case hex digits, as the yuzu family names its save folders.
pub fn is_title_id(text: &str) -> bool {
    text.len() == 16
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() && c.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Yuzu,
    Eden,
    Citron,
    Suyu,
    Sudachi,
    Ryujinx,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Yuzu => "yuzu",
            Kind::Eden => "Eden",
            Kind::Citron => "Citron",
            Kind::Suyu => "Suyu",
            Kind::Sudachi => "Sudachi",
            Kind::Ryujinx => "Ryujinx",
        }
    }
}

/// One emulator installation's folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub kind: Kind,
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

/// The XDG base folders of this user.
#[derive(Clone, Debug)]
pub struct Xdg {
    pub home: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Xdg {
    pub fn of(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            data: home.join(".local/share"),
            config: home.join(".config"),
            cache: home.join(".cache"),
        }
    }

    pub fn from_env(home: &Path) -> Self {
        let var = |name: &str, default: PathBuf| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or(default)
        };
        let of = Self::of(home);
        Self {
            data: var("XDG_DATA_HOME", of.data),
            config: var("XDG_CONFIG_HOME", of.config),
            cache: var("XDG_CACHE_HOME", of.cache),
            home: of.home,
        }
    }
}

/// The yuzu family: folder name and Flatpak id (the ones Flathub has).
const YUZU_FAMILY: &[(Kind, &str, Option<&str>)] = &[
    (Kind::Yuzu, "yuzu", Some("org.yuzu_emu.yuzu")),
    (Kind::Eden, "eden", Some("dev.eden_emu.eden")),
    (Kind::Citron, "citron", None),
    (Kind::Suyu, "suyu", None),
    (Kind::Sudachi, "sudachi", None),
];
const RYUJINX_FLATPAK: &str = "org.ryujinx.Ryujinx";

/// Every emulator installation that has run here: native, or as Flatpak (whose XDG folders lie
/// in `~/.var/app/<id>`).
pub fn installs(xdg: &Xdg) -> Vec<Install> {
    let mut found = vec![];
    for (kind, dir, flatpak) in YUZU_FAMILY {
        let mut bases = vec![(xdg.data.clone(), xdg.config.clone(), xdg.cache.clone())];
        if let Some(id) = flatpak {
            let app = xdg.home.join(".var/app").join(id);
            bases.push((app.join("data"), app.join("config"), app.join("cache")));
        }
        for (data, config, cache) in bases {
            let install = Install {
                kind: *kind,
                data: data.join(dir),
                config: config.join(dir),
                cache: cache.join(dir),
            };
            if yuzu::is_install(&install.data) {
                found.push(install);
            }
        }
    }
    for config in [
        xdg.config.clone(),
        xdg.home.join(".var/app").join(RYUJINX_FLATPAK).join("config"),
    ] {
        let data = config.join("Ryujinx");
        if ryujinx::is_install(&data) {
            found.push(Install {
                kind: Kind::Ryujinx,
                config: data.clone(),
                cache: data.clone(),
                data,
            });
        }
    }
    found
}

/// A value from an INI file as Qt writes it (`[Group]`, `key=value`).
pub fn ini_value(ini: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = name == group;
        } else if in_group
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Every regular file below `dir`, without the emulator's own files at its top (`skip`).
/// Links are not followed: a save is plain files.
pub fn list_files(dir: &Path, skip: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, top: bool, skip: &[&str], out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name();
            if top && skip.iter().any(|s| name == *s) {
                continue;
            }
            if kind.is_dir() {
                walk(&entry.path(), false, skip, out);
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
    let mut out = vec![];
    walk(dir, true, skip, &mut out);
    out.sort();
    out
}

fn save_dir(install: &Install, title: &str) -> Option<PathBuf> {
    match install.kind {
        Kind::Ryujinx => ryujinx::save_dir(install, title),
        _ => yuzu::save_dir(install, title),
    }
}

fn titles(install: &Install) -> Vec<String> {
    match install.kind {
        Kind::Ryujinx => ryujinx::titles(install),
        _ => yuzu::titles(install),
    }
}

/// The save files of `title` in `dir`, one of its save folders.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    list_files(dir, yuzu::OWN_FILES)
}

/// Where a title's save is on this device, and which emulator holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub dir: PathBuf,
    pub emulator: &'static str,
}

fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The save folder of `title` on this device: the one chosen in the web UI, else the one
/// emulator that has a save of it, else the one emulator that can take it. Several of either is
/// a question for the web UI. Ryujinx only reads saves it created, so it can take a title only
/// once the game has been started there.
pub fn place(installs: &[Install], title: &str, chosen: Option<&Path>) -> Result<Place, GameProblem> {
    let mut possible: Vec<Place> = vec![];
    for install in installs {
        let Some(dir) = save_dir(install, title) else {
            continue;
        };
        // One folder seen through a link (Eden's "link to Ryujinx") is one place.
        if possible.iter().any(|p| real(&p.dir) == real(&dir)) {
            continue;
        }
        possible.push(Place {
            dir,
            emulator: install.kind.name(),
        });
    }
    if let Some(chosen) = chosen
        && let Some(p) = possible.iter().find(|p| real(&p.dir) == real(chosen))
    {
        return Ok(p.clone());
    }
    let holding: Vec<&Place> = possible.iter().filter(|p| !files_in(&p.dir).is_empty()).collect();
    let several = |places: Vec<&Place>| GameProblem::SeveralEmulators {
        candidates: places.iter().map(|p| p.dir.display().to_string()).collect(),
    };
    match holding.as_slice() {
        [one] => return Ok((*one).clone()),
        [] => {}
        _ => return Err(several(holding)),
    }
    match possible.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(match installs.first() {
            Some(install) => GameProblem::StartInEmulator {
                emulator: install.kind.name().into(),
            },
            None => GameProblem::NoPrefix,
        }),
        _ => Err(several(possible.iter().collect())),
    }
}

/// Every title with a save in one of `installs`, as game keys.
pub fn games(installs: &[Install]) -> Vec<String> {
    let mut keys: Vec<String> = installs.iter().flat_map(titles).map(|t| key(&t)).collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The title's name, from the first emulator that knows it.
pub fn name(installs: &[Install], title: &str) -> Option<String> {
    installs.iter().find_map(|i| match i.kind {
        Kind::Ryujinx => ryujinx::name(i, title),
        _ => yuzu::name(i, title),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_the_title_id() {
        assert_eq!(key("0100F2C0115B6000"), "Switch 0100F2C0115B6000");
        assert_eq!(title_id("Switch 0100F2C0115B6000"), Some("0100F2C0115B6000"));
        assert_eq!(
            title_id("Switch 0100f2c0115b6000"),
            None,
            "folders are upper case"
        );
        assert_eq!(title_id("Hades"), None);
        assert_eq!(title_id("Switch Sports"), None);
    }

    #[test]
    fn qt_ini_values_are_read_by_group() {
        let ini = "[System]\ncurrent_user\\default=false\ncurrent_user=2\n\n[Data%20Storage]\nnand_directory=/mnt/nand/\n";
        assert_eq!(ini_value(ini, "System", "current_user").as_deref(), Some("2"));
        assert_eq!(
            ini_value(ini, "System", "current_user\\default").as_deref(),
            Some("false")
        );
        assert_eq!(
            ini_value(ini, "Data%20Storage", "nand_directory").as_deref(),
            Some("/mnt/nand/")
        );
        assert_eq!(ini_value(ini, "Other", "current_user"), None);
    }

    #[test]
    fn the_emulators_own_files_are_not_part_of_the_save() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("slot")).unwrap();
        for f in ["progress.sav", "slot/0.bin", ".yuzu_save_size", "ExtraData0"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        // Only at the top: a game's own file of that name deeper down stays.
        std::fs::write(dir.join("slot/ExtraData0"), "x").unwrap();
        let names: Vec<String> = files_in(dir)
            .iter()
            .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
            .collect();
        assert_eq!(names, ["progress.sav", "slot/0.bin", "slot/ExtraData0"]);
    }
}
