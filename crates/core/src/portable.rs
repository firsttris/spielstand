//! Paths that mean the same place on every device.
//!
//! A save under a launcher root becomes `<root:steam>/…`, one under the home folder `<home>/…`;
//! anything else stays absolute. Store roots win over the home folder, because Steam may live in
//! `~/.local/share/Steam` on one PC and in the Flatpak folder on the other.

use std::path::{Path, PathBuf};

/// The places a device anchors portable paths to.
#[derive(Clone, Debug, Default)]
pub struct Anchors {
    pub home: PathBuf,
    /// Store key (e.g. `steam`, `heroic`, `lutris`) and the root folder, in priority order.
    pub roots: Vec<(String, PathBuf)>,
}

const ANCHORED_STORES: &[&str] = &["steam", "heroic", "lutris", "legendary", "gog", "epic", "games"];

/// Saves inside a Wine prefix are stored relative to the prefix: `<prefix>/drive_c/…`. Which
/// folder that is on another device is decided there, for that game.
pub const PREFIX: &str = "<prefix>";

/// A Switch emulator's save of one title is stored relative to that title's save folder:
/// `<switch>/…`. The emulator, its data folder and the profile id differ between devices; which
/// folder it is there is decided on that device, for that title.
pub const SWITCH: &str = "<switch>";

pub fn is_anchored_store(store: &str) -> bool {
    ANCHORED_STORES.contains(&store)
}

fn rel(path: &Path, base: &Path) -> Option<String> {
    let rest = path.strip_prefix(base).ok()?;
    Some(rest.to_string_lossy().replace('\\', "/"))
}

fn join(anchor: &str, rest: &str) -> String {
    if rest.is_empty() {
        anchor.to_string()
    } else {
        format!("{anchor}/{rest}")
    }
}

pub fn to_portable(path: &Path, anchors: &Anchors) -> String {
    let by_root = anchors
        .roots
        .iter()
        .filter(|(store, _)| is_anchored_store(store))
        .filter_map(|(store, root)| rel(path, root).map(|r| (root.as_os_str().len(), store, r)))
        .max_by_key(|(len, _, _)| *len);
    if let Some((_, store, rest)) = by_root {
        return join(&format!("<root:{store}>"), &rest);
    }
    if let Some(rest) = rel(path, &anchors.home) {
        return join("<home>", &rest);
    }
    path.to_string_lossy().replace('\\', "/")
}

/// Where a portable path lands on this device; `None` when its store root is unknown here.
pub fn from_portable(portable: &str, anchors: &Anchors) -> Option<PathBuf> {
    if let Some(rest) = portable.strip_prefix("<home>") {
        return Some(anchors.home.join(rest.trim_start_matches('/')));
    }
    if let Some(after) = portable.strip_prefix("<root:") {
        let (store, rest) = after.split_once('>')?;
        let rest = rest.trim_start_matches('/');
        let candidates: Vec<&PathBuf> = anchors
            .roots
            .iter()
            .filter(|(s, _)| s == store)
            .map(|(_, root)| root)
            .collect();
        // Prefer the root where the file's folder already exists (several Steam libraries).
        let existing = candidates
            .iter()
            .find(|root| root.join(rest).parent().is_some_and(Path::exists));
        return existing.or(candidates.first()).map(|root| root.join(rest));
    }
    if portable.starts_with('<') {
        return None;
    }
    Some(PathBuf::from(portable))
}

/// Paths may only be written below the device's home, a known root or the game's prefix.
pub fn is_allowed_target(path: &Path, anchors: &Anchors, prefix: Option<&Path>) -> bool {
    let normal = !path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    normal
        && (path.starts_with(&anchors.home)
            || anchors.roots.iter().any(|(_, r)| path.starts_with(r))
            || prefix.is_some_and(|p| path.starts_with(p)))
}

/// Stands for the Wine profile in `drive_c/users/<user>/…`: `steamuser` under Proton and
/// Prefixr, the Linux user name under plain Wine. The same save has the same path, and so the
/// same content hash, on every device.
pub const WINE_USER: &str = "<user>";

/// `drive_c/users/<name>/…` → `drive_c/users/<user>/…`; `Public` is the same everywhere.
pub fn normalize_profile(rest: &str) -> String {
    let mut parts = rest.splitn(4, '/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("drive_c"), Some("users"), Some(user), tail) if user != "Public" => {
            join(&format!("drive_c/users/{WINE_USER}"), tail.unwrap_or(""))
        }
        _ => rest.to_string(),
    }
}

/// `<prefix>/rel` for a file inside `prefix`, with the profile folder normalised.
pub fn to_portable_in_prefix(path: &Path, prefix: &Path) -> Option<String> {
    rel(path, prefix).map(|rest| join(PREFIX, &normalize_profile(&rest)))
}

/// Splits a path inside a Wine prefix at its `drive_c`: the prefix folder and the rest. Works on
/// paths of other machines too (a Ludusavi backup's original paths).
pub fn split_at_drive_c(path: &str) -> Option<(&str, &str)> {
    let at = path.find("/drive_c/")?;
    Some((&path[..at], &path[at + 1..]))
}

pub fn is_in_prefix(portable: &str) -> bool {
    portable == PREFIX || portable.starts_with("<prefix>/")
}

/// Where `<prefix>/…` lands in `prefix` on this device. `users` are the profile folders in the
/// prefix's `drive_c/users`; `<user>` becomes `steamuser` if it is there, else the only other
/// profile, else `fallback` (the Linux user name, which plain Wine would create). Several
/// profiles and no `steamuser`: `None`, a question rather than a guess.
pub fn resolve_in_prefix(portable: &str, prefix: &Path, users: &[String], fallback: &str) -> Option<PathBuf> {
    let rest = portable.strip_prefix(PREFIX)?.trim_start_matches('/');
    let marker = format!("drive_c/users/{WINE_USER}");
    let Some(tail) = rest.strip_prefix(&marker) else {
        return Some(prefix.join(rest));
    };
    let profiles: Vec<&String> = users.iter().filter(|u| *u != "Public").collect();
    let user = if profiles.iter().any(|u| *u == "steamuser") {
        "steamuser"
    } else {
        match profiles.as_slice() {
            [] => fallback,
            [only] => only.as_str(),
            _ => return None,
        }
    };
    Some(prefix.join(format!("drive_c/users/{user}{tail}")))
}

/// `<switch>/rel` for a file inside a title's save folder `dir`.
pub fn to_portable_in_save_dir(path: &Path, dir: &Path) -> Option<String> {
    rel(path, dir).map(|rest| join(SWITCH, &rest))
}

pub fn is_in_save_dir(portable: &str) -> bool {
    portable.starts_with("<switch>/")
}

/// Where `<switch>/…` lands in the title's save folder `dir` on this device.
pub fn resolve_in_save_dir(portable: &str, dir: &Path) -> Option<PathBuf> {
    let rest = portable.strip_prefix("<switch>/")?;
    (!rest.is_empty()).then(|| dir.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(home: &str, steam: &str) -> Anchors {
        Anchors {
            home: home.into(),
            roots: vec![("steam".into(), steam.into())],
        }
    }

    #[test]
    fn steam_root_wins_over_home() {
        let a = anchors("/home/tris", "/home/tris/.local/share/Steam");
        let p = Path::new("/home/tris/.local/share/Steam/steamapps/compatdata/1/pfx/save.dat");
        assert_eq!(
            to_portable(p, &a),
            "<root:steam>/steamapps/compatdata/1/pfx/save.dat"
        );
    }

    #[test]
    fn moves_between_devices() {
        let desktop = anchors("/home/tris", "/home/tris/.local/share/Steam");
        let laptop = anchors(
            "/home/tristan",
            "/home/tristan/.var/app/com.valvesoftware.Steam/data/Steam",
        );
        let p = to_portable(
            Path::new("/home/tris/.local/share/Steam/steamapps/x.sav"),
            &desktop,
        );
        assert_eq!(
            from_portable(&p, &laptop).unwrap(),
            Path::new("/home/tristan/.var/app/com.valvesoftware.Steam/data/Steam/steamapps/x.sav")
        );
        let h = to_portable(Path::new("/home/tris/.config/Game/s.dat"), &desktop);
        assert_eq!(h, "<home>/.config/Game/s.dat");
        assert_eq!(
            from_portable(&h, &laptop).unwrap(),
            Path::new("/home/tristan/.config/Game/s.dat")
        );
    }

    #[test]
    fn unknown_store_has_no_target() {
        let a = anchors("/home/tris", "/s");
        assert_eq!(from_portable("<root:heroic>/a", &a), None);
    }

    #[test]
    fn targets_stay_inside_known_places() {
        let a = anchors("/home/tris", "/mnt/games/Steam");
        assert!(is_allowed_target(Path::new("/home/tris/x"), &a, None));
        assert!(is_allowed_target(Path::new("/mnt/games/Steam/x"), &a, None));
        assert!(!is_allowed_target(Path::new("/etc/passwd"), &a, None));
        assert!(!is_allowed_target(
            Path::new("/home/tris/../../etc/passwd"),
            &a,
            None
        ));
        let sd = Path::new("/run/media/deck/SD/Games/Hades/prefix");
        assert!(is_allowed_target(&sd.join("drive_c/x"), &a, Some(sd)));
        assert!(!is_allowed_target(
            Path::new("/run/media/deck/SD/other"),
            &a,
            Some(sd)
        ));
    }

    fn users(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_path_splits_at_drive_c() {
        assert_eq!(
            split_at_drive_c("/home/t/Games/Hades/prefix/drive_c/users/steamuser/x"),
            Some(("/home/t/Games/Hades/prefix", "drive_c/users/steamuser/x"))
        );
        assert_eq!(split_at_drive_c("/home/t/.config/x"), None);
    }

    #[test]
    fn saves_in_a_prefix_follow_the_game_to_another_folder() {
        let desktop = Path::new("/home/tris/Games/Hades/prefix");
        let deck = Path::new("/run/media/deck/SD/Games/Hades/prefix");
        let file = desktop.join("drive_c/users/steamuser/AppData/Roaming/Hades/save.sav");
        let p = to_portable_in_prefix(&file, desktop).unwrap();
        assert_eq!(p, "<prefix>/drive_c/users/<user>/AppData/Roaming/Hades/save.sav");
        assert!(is_in_prefix(&p));
        assert_eq!(
            resolve_in_prefix(&p, deck, &users(&["steamuser", "deck"]), "deck").unwrap(),
            deck.join("drive_c/users/steamuser/AppData/Roaming/Hades/save.sav")
        );
    }

    #[test]
    fn the_wine_profile_is_normalised_and_mapped_to_the_one_on_this_device() {
        assert_eq!(
            normalize_profile("drive_c/users/tristan/Documents/G/s.dat"),
            "drive_c/users/<user>/Documents/G/s.dat"
        );
        assert_eq!(
            normalize_profile("drive_c/users/Public/Documents/x"),
            "drive_c/users/Public/Documents/x"
        );
        assert_eq!(normalize_profile("drive_c/Games/x"), "drive_c/Games/x");

        let deck = Path::new("/home/deck/Games/G/pfx");
        let p = "<prefix>/drive_c/users/<user>/Documents/G/s.dat";
        let at = |names: &[&str]| resolve_in_prefix(p, deck, &users(names), "deck");
        assert_eq!(
            at(&["Public", "deck"]),
            Some(deck.join("drive_c/users/deck/Documents/G/s.dat"))
        );
        assert_eq!(
            at(&["Public", "deck", "steamuser"]),
            Some(deck.join("drive_c/users/steamuser/Documents/G/s.dat")),
            "steamuser wins: Proton and Prefixr profiles"
        );
        assert_eq!(
            at(&[]),
            Some(deck.join("drive_c/users/deck/Documents/G/s.dat")),
            "a new prefix"
        );
        assert_eq!(at(&["a", "b"]), None, "two candidates: no guessing");
        assert_eq!(
            resolve_in_prefix(
                "<prefix>/drive_c/users/Public/Documents/x",
                deck,
                &users(&["deck"]),
                "deck"
            ),
            Some(deck.join("drive_c/users/Public/Documents/x"))
        );
    }

    #[test]
    fn switch_saves_are_relative_to_the_titles_save_folder() {
        let here = Path::new("/home/t/.local/share/eden/nand/user/save/0000000000000000/AB/0100F2C0115B6000");
        let there = Path::new("/home/deck/.config/Ryujinx/bis/user/save/0000000000000004/0");
        let portable = to_portable_in_save_dir(&here.join("slot_0/progress.sav"), here).unwrap();
        assert_eq!(portable, "<switch>/slot_0/progress.sav");
        assert!(is_in_save_dir(&portable));
        assert_eq!(
            resolve_in_save_dir(&portable, there),
            Some(there.join("slot_0/progress.sav"))
        );
        assert_eq!(resolve_in_save_dir("<switch>/", there), None);
        assert_eq!(to_portable_in_save_dir(Path::new("/elsewhere/x"), here), None);
    }
}
