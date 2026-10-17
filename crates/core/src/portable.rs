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

const ANCHORED_STORES: &[&str] = &["steam", "heroic", "lutris", "legendary", "gog", "epic"];

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

/// Paths may only be written below the device's home or a known root.
pub fn is_allowed_target(path: &Path, anchors: &Anchors) -> bool {
    let normal = !path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    normal && (path.starts_with(&anchors.home) || anchors.roots.iter().any(|(_, r)| path.starts_with(r)))
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
        assert!(is_allowed_target(Path::new("/home/tris/x"), &a));
        assert!(is_allowed_target(Path::new("/mnt/games/Steam/x"), &a));
        assert!(!is_allowed_target(Path::new("/etc/passwd"), &a));
        assert!(!is_allowed_target(Path::new("/home/tris/../../etc/passwd"), &a));
    }
}
