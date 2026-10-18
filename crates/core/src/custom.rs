//! Custom games: games the manifest lacks, added by hand or by an assistant on a device. A
//! custom game makes every device back up the files it names, so the paths it may name are
//! narrow: below a manifest placeholder, starting with a real folder, never into the folders
//! that hold keys and passwords.

use spielstand_protocol::CustomGame;

/// The placeholders a path may start with (Ludusavi's `src/resource/manifest.rs`,
/// `placeholder`). `<storeUserId>` and `<osUserName>` may appear further in.
const ANCHORS: &[&str] = &[
    "<home>",
    "<base>",
    "<root>",
    "<winAppData>",
    "<winLocalAppData>",
    "<winLocalAppDataLow>",
    "<winDocuments>",
    "<winPublic>",
    "<winProgramData>",
    "<winDir>",
    "<xdgData>",
    "<xdgConfig>",
];

/// Folders that hold keys, passwords or Spielstand's own token: never a save.
const PROTECTED: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".password-store",
    ".pki",
    ".aws",
    ".kube",
    "keyrings",
    "spielstand",
];

pub const MAX_PATHS: usize = 32;
/// What one custom game may hold on a device; more is a path that caught too much.
pub const MAX_FILES: usize = 10_000;
pub const MAX_BYTES: u64 = 1 << 30;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("the name is empty")]
    NoName,
    #[error("the name is taken by Switch games")]
    SwitchName,
    #[error("no paths")]
    NoPaths,
    #[error("more than {MAX_PATHS} paths")]
    TooManyPaths,
    #[error("{0}: a path starts with a placeholder such as <home> or <winAppData>")]
    NoAnchor(String),
    #[error("{0}: the first folder after the placeholder has to be named, not a pattern")]
    TooBroad(String),
    #[error("{0}: `..` is not allowed")]
    Parent(String),
    #[error("{0}: this folder holds keys or passwords, not saves")]
    Protected(String),
    #[error("{0}: an install folder is one folder name")]
    InstallDir(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TooMuch {
    #[error("{0} files, at most {MAX_FILES}")]
    Files(usize),
    #[error("{0} bytes, at most {MAX_BYTES}")]
    Bytes(u64),
}

fn is_pattern(part: &str) -> bool {
    part.contains(['*', '?', '[', ']', '{', '}']) || part.starts_with('<')
}

fn check_path(path: &str) -> Result<(), Invalid> {
    let invalid = |f: fn(String) -> Invalid| Err(f(path.to_string()));
    let Some(anchor) = ANCHORS.iter().find(|a| path.starts_with(*a)) else {
        return invalid(Invalid::NoAnchor);
    };
    let rest = &path[anchor.len()..];
    if !rest.starts_with('/') {
        return invalid(Invalid::TooBroad);
    }
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| *p == ".." || *p == ".") {
        return invalid(Invalid::Parent);
    }
    if parts.first().is_none_or(|p| is_pattern(p)) {
        return invalid(Invalid::TooBroad);
    }
    if parts
        .iter()
        .any(|p| PROTECTED.contains(&p.to_lowercase().as_str()))
    {
        return invalid(Invalid::Protected);
    }
    Ok(())
}

/// Switch games are keyed `Switch <title id>`; a custom game must not take such a key.
fn is_switch_key(name: &str) -> bool {
    name.strip_prefix("Switch ")
        .is_some_and(|id| id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()))
}

fn clean(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for item in items {
        let item = item.trim().replace('\\', "/");
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// The custom game with its name and paths trimmed, or why it may not be added.
pub fn check(game: &CustomGame) -> Result<CustomGame, Invalid> {
    let name = game.name.trim().to_string();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(Invalid::NoName);
    }
    if is_switch_key(&name) {
        return Err(Invalid::SwitchName);
    }
    let files = clean(&game.files);
    if files.is_empty() {
        return Err(Invalid::NoPaths);
    }
    if files.len() > MAX_PATHS {
        return Err(Invalid::TooManyPaths);
    }
    files.iter().try_for_each(|f| check_path(f))?;
    let install_dir = clean(&game.install_dir);
    if let Some(bad) = install_dir
        .iter()
        .find(|d| d.contains('/') || *d == "." || *d == "..")
    {
        return Err(Invalid::InstallDir(bad.clone()));
    }
    Ok(CustomGame {
        name,
        files,
        install_dir,
        extend: game.extend,
    })
}

/// Whether the files a custom game found on a device are few enough to be saves.
pub fn check_found(sizes: &[u64]) -> Result<(), TooMuch> {
    if sizes.len() > MAX_FILES {
        return Err(TooMuch::Files(sizes.len()));
    }
    let total: u64 = sizes.iter().sum();
    if total > MAX_BYTES {
        return Err(TooMuch::Bytes(total));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(files: &[&str]) -> CustomGame {
        CustomGame {
            name: " Indie ".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_narrow_path_is_taken_trimmed() {
        let ok = check(&game(&[
            " <home>/.local/share/Indie/*.sav ",
            "<winAppData>\\Indie",
            "<home>/.local/share/Indie/*.sav",
        ]))
        .unwrap();
        assert_eq!(ok.name, "Indie");
        assert_eq!(
            ok.files,
            ["<home>/.local/share/Indie/*.sav", "<winAppData>/Indie"]
        );
    }

    #[test]
    fn paths_that_catch_too_much_are_refused() {
        let refused = |p: &str| check(&game(&[p])).unwrap_err();
        assert!(matches!(
            refused("/home/t/.local/share/Indie"),
            Invalid::NoAnchor(_)
        ));
        assert!(matches!(refused("~/Indie"), Invalid::NoAnchor(_)));
        assert!(matches!(refused("<home>"), Invalid::TooBroad(_)));
        assert!(matches!(refused("<home>/"), Invalid::TooBroad(_)));
        assert!(matches!(refused("<home>/*"), Invalid::TooBroad(_)));
        assert!(matches!(refused("<home>/**/save"), Invalid::TooBroad(_)));
        assert!(matches!(refused("<home>/<osUserName>"), Invalid::TooBroad(_)));
        assert!(matches!(refused("<homeX>/Indie"), Invalid::NoAnchor(_)));
        assert!(matches!(refused("<home>/Games/../.ssh"), Invalid::Parent(_)));
        assert!(matches!(refused("<home>/.ssh/id_ed25519"), Invalid::Protected(_)));
        assert!(matches!(refused("<xdgData>/keyrings"), Invalid::Protected(_)));
        assert!(matches!(
            refused("<xdgConfig>/Spielstand/agent.toml"),
            Invalid::Protected(_)
        ));
    }

    #[test]
    fn names_and_install_folders_are_checked() {
        let mut g = game(&["<home>/Indie"]);
        g.name = "  ".into();
        assert_eq!(check(&g), Err(Invalid::NoName));
        g.name = "Switch 0100F2C0115B6000".into();
        assert_eq!(check(&g), Err(Invalid::SwitchName));
        g.name = "Switch Sports Fan Game".into();
        assert!(check(&g).is_ok());
        g.install_dir = vec!["Games/Indie".into()];
        assert!(matches!(check(&g), Err(Invalid::InstallDir(_))));
        assert_eq!(check(&game(&[" "])), Err(Invalid::NoPaths));
    }

    #[test]
    fn too_many_or_too_large_files_are_refused() {
        assert!(check_found(&[10, 20]).is_ok());
        assert_eq!(
            check_found(&vec![1; MAX_FILES + 1]),
            Err(TooMuch::Files(MAX_FILES + 1))
        );
        assert_eq!(check_found(&[MAX_BYTES, 1]), Err(TooMuch::Bytes(MAX_BYTES + 1)));
    }
}
