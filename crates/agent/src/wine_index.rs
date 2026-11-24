//! Which of many Wine prefixes can hold saves of a game. Ludusavi tries every path of every game
//! in every prefix; with 185 prefixes and a manifest of over 20,000 games with paths that is
//! millions of directory listings. Here each prefix is listed once, and a path is only tried
//! where the first folder it names exists.
//!
//! Never narrower than Ludusavi's `parse_paths` (`src/scan.rs`, `Store::OtherWine`): a path this
//! does not understand is tried in every prefix.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use ludusavi::resource::manifest::placeholder as p;

/// The folder below a prefix a manifest path starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Start {
    Prefix,
    Home,
    AppData,
    LocalAppData,
    LocalAppDataLow,
    Documents,
    MyGames,
    SavedGames,
    Public,
    ProgramData,
    Windows,
}

/// In which prefixes a path has to be tried.
#[derive(Debug, PartialEq, Eq)]
pub enum Where {
    /// In all of them.
    Everywhere,
    /// The path does not depend on the prefix: one of them is enough.
    Once,
    /// In these (their index).
    In(BTreeSet<usize>),
}

#[derive(Default)]
pub struct WineIndex {
    /// The prefixes with an entry of this name (ASCII lower case, as the glob compares) there.
    entries: HashMap<(Start, String), BTreeSet<usize>>,
    /// Prefixes with a folder that could not be listed: never left out.
    unclear: BTreeSet<usize>,
}

/// What a path can start with; the first that fits counts, so the longer ones come first. Where
/// many games share a folder (`AppData`, `My Games`), the folder below it decides.
const STARTS: &[(&str, Start)] = &[
    (p::ROOT, Start::Prefix),
    ("<home>/AppData/Roaming", Start::AppData),
    ("<home>/AppData/LocalLow", Start::LocalAppDataLow),
    ("<home>/AppData/Local", Start::LocalAppData),
    ("<home>/Documents/My Games", Start::MyGames),
    ("<winDocuments>/My Games", Start::MyGames),
    ("<home>/Documents", Start::Documents),
    ("<home>/Saved Games", Start::SavedGames),
    (p::HOME, Start::Home),
    (p::WIN_APP_DATA, Start::AppData),
    (p::WIN_LOCAL_APP_DATA, Start::LocalAppData),
    (p::WIN_LOCAL_APP_DATA_LOW, Start::LocalAppDataLow),
    (p::WIN_DOCUMENTS, Start::Documents),
    (p::WIN_PUBLIC, Start::Public),
    (p::WIN_PROGRAM_DATA, Start::ProgramData),
    (p::WIN_DIR, Start::Windows),
];

/// The folders of a Wine profile, as `parse_paths` spells them.
const IN_PROFILE: &[(Start, &[&str])] = &[
    (Start::AppData, &["appdata", "roaming"]),
    (Start::AppData, &["application data"]),
    (Start::LocalAppData, &["appdata", "local"]),
    (Start::LocalAppData, &["local settings", "application data"]),
    (Start::LocalAppDataLow, &["appdata", "locallow"]),
    (Start::Documents, &["documents"]),
    (Start::Documents, &["my documents"]),
    (Start::MyGames, &["documents", "my games"]),
    (Start::MyGames, &["my documents", "my games"]),
    (Start::SavedGames, &["saved games"]),
];

const IN_DRIVE: &[(Start, &[&str])] = &[
    (Start::Public, &["users", "public"]),
    (Start::ProgramData, &["programdata"]),
    (Start::Windows, &["windows"]),
];

fn in_profile(start: &Start) -> bool {
    *start == Start::Home || IN_PROFILE.iter().any(|(s, _)| s == start)
}

fn listed(dir: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.map(|e| (e.file_name().to_string_lossy().to_ascii_lowercase(), e.path())))
            .collect(),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(vec![])
        }
        Err(e) => Err(e),
    }
}

/// Every folder `path` names below `dir`, whatever its case.
fn descend(dir: &Path, path: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(vec![dir.to_path_buf()]);
    };
    let mut found = vec![];
    for (_, child) in listed(dir)?.into_iter().filter(|(name, _)| name == first) {
        found.extend(descend(&child, rest)?);
    }
    Ok(found)
}

fn entries_of(prefix: &Path, needed: &BTreeSet<Start>) -> std::io::Result<Vec<(Start, String)>> {
    let mut out = vec![];
    let mut add = |start: Start, dir: &Path| -> std::io::Result<()> {
        if !needed.contains(&start) {
            return Ok(());
        }
        out.extend(listed(dir)?.into_iter().map(|(name, _)| (start, name)));
        Ok(())
    };
    add(Start::Prefix, prefix)?;
    if needed.iter().all(|s| *s == Start::Prefix) {
        return Ok(out);
    }
    for (_, drive) in listed(prefix)?
        .into_iter()
        .filter(|(n, _)| n.starts_with("drive_"))
    {
        for (start, path) in IN_DRIVE.iter().filter(|(s, _)| needed.contains(s)) {
            for dir in descend(&drive, path)? {
                add(*start, &dir)?;
            }
        }
        if !needed.iter().any(in_profile) {
            continue;
        }
        for users in descend(&drive, &["users"])? {
            for (_, profile) in listed(&users)? {
                add(Start::Home, &profile)?;
                for (start, path) in IN_PROFILE.iter().filter(|(s, _)| needed.contains(s)) {
                    for dir in descend(&profile, path)? {
                        add(*start, &dir)?;
                    }
                }
            }
        }
    }
    Ok(out)
}

fn rest_after<'a>(path: &'a str, start: &str) -> Option<&'a str> {
    path.strip_prefix(start)?.strip_prefix(['/', '\\'])
}

/// Which folder of the prefix decides about `path`, if one does.
fn start_of(path: &str) -> Option<Start> {
    if path.starts_with(p::BASE) {
        return Some(Start::Prefix);
    }
    STARTS
        .iter()
        .find(|(start, _)| rest_after(path, start).is_some())
        .map(|(_, start)| *start)
}

/// The first folder a path names, if it is spelled out.
fn literal(rest: &str) -> Option<String> {
    let first = rest.split(['/', '\\']).next().unwrap_or_default();
    let spelled_out = !first.is_empty()
        && first != "."
        && first != ".."
        && !first.contains(['*', '?', '[', ']', '{', '}', '<', '>']);
    spelled_out.then(|| first.to_ascii_lowercase())
}

impl WineIndex {
    /// What the manifest paths `paths` need to know about `prefixes`.
    pub fn build<'a>(prefixes: &[PathBuf], paths: impl Iterator<Item = &'a str>) -> Self {
        let needed: BTreeSet<Start> = paths.filter_map(start_of).collect();
        let mut index = Self::default();
        for (i, prefix) in prefixes.iter().enumerate() {
            match entries_of(prefix, &needed) {
                Ok(entries) => {
                    for entry in entries {
                        index.entries.entry(entry).or_default().insert(i);
                    }
                }
                Err(_) => {
                    index.unclear.insert(i);
                }
            }
        }
        index
    }

    fn with(&self, start: Start, names: impl Iterator<Item = String>) -> Where {
        let mut found = self.unclear.clone();
        for name in names {
            found.extend(self.entries.get(&(start, name)).into_iter().flatten());
        }
        Where::In(found)
    }

    /// Where to try a manifest path of a game that may be installed in folders `install_dirs`.
    pub fn places(&self, path: &str, install_dirs: &[&str]) -> Where {
        // Ludusavi's `parse_paths` puts `<base>` into the prefix as `<prefix>/<install dir>`.
        let rest_of = |start: &str| rest_after(path, start);
        let in_install_dir =
            path.starts_with(p::BASE) || rest_of(p::ROOT).is_some_and(|rest| rest.starts_with(p::GAME));
        if in_install_dir {
            let names: Option<Vec<String>> = install_dirs.iter().map(|d| literal_dir(d)).collect();
            return names.map_or(Where::Everywhere, |n| self.with(Start::Prefix, n.into_iter()));
        }
        if let Some((start, rest)) = STARTS
            .iter()
            .find_map(|(placeholder, start)| Some((*start, rest_of(placeholder)?)))
        {
            return literal(rest).map_or(Where::Everywhere, |name| self.with(start, std::iter::once(name)));
        }
        let depends = STARTS.iter().any(|(placeholder, _)| path.contains(placeholder))
            || path.contains(p::BASE)
            || path.contains(p::OS_USER_NAME);
        if depends { Where::Everywhere } else { Where::Once }
    }
}

/// The first folder of an install folder's name; `parse_paths` escapes it, so it is literal.
fn literal_dir(dir: &str) -> Option<String> {
    let first = dir.split(['/', '\\']).next().unwrap_or_default();
    (!first.is_empty() && first != "." && first != "..").then(|| first.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(dir: &Path, name: &str, folders: &[&str]) -> PathBuf {
        let prefix = dir.join(name);
        for folder in folders {
            std::fs::create_dir_all(prefix.join(folder)).unwrap();
        }
        prefix
    }

    const ALL: &[&str] = &[
        "<winAppData>/x",
        "<winLocalAppData>/x",
        "<winDocuments>/x",
        "<winDocuments>/My Games/x",
        "<home>/Saved Games/x",
        "<winProgramData>/x",
        "<base>/x",
    ];

    fn only(i: usize) -> Where {
        Where::In(BTreeSet::from([i]))
    }

    #[test]
    fn a_path_is_tried_where_its_first_folder_exists() {
        let dir = tempfile::tempdir().unwrap();
        let prefixes = [
            prefix(
                dir.path(),
                "a",
                &["drive_c/users/steamuser/AppData/Roaming/Supergiant Games"],
            ),
            prefix(
                dir.path(),
                "b",
                &[
                    "drive_c/users/tris/My Documents/My Games/Y",
                    "drive_c/ProgramData/Maker",
                    "My Game/saves",
                ],
            ),
        ];
        let index = WineIndex::build(&prefixes, ALL.iter().copied());
        assert_eq!(index.places("<winAppData>/Supergiant Games/Hades", &[]), only(0));
        assert_eq!(index.places("<winAppData>/supergiant games/*", &[]), only(0));
        assert_eq!(index.places("<winDocuments>/My Games/Y", &[]), only(1));
        assert_eq!(
            index.places("<winDocuments>/My Games/Nothing", &[]),
            Where::In(BTreeSet::new())
        );
        assert_eq!(
            index.places("<home>/AppData/Roaming/Supergiant Games/x", &[]),
            only(0)
        );
        assert_eq!(index.places("<winProgramData>/Maker/X", &[]), only(1));
        assert_eq!(index.places("<base>/saves", &["Other", "My Game"]), only(1));
        assert_eq!(index.places("<root>/<game>/saves", &["My Game"]), only(1));
        assert_eq!(
            index.places("<winLocalAppData>/Nothing", &[]),
            Where::In(BTreeSet::new())
        );
    }

    #[test]
    fn a_path_not_understood_is_tried_everywhere() {
        let index = WineIndex::build(&[], std::iter::empty());
        for path in [
            "<winAppData>",
            "<winAppData>/*/saves",
            "<winAppData>/<storeUserId>/saves",
            "<home>/../x",
            "/data/<osUserName>/x",
            "x/<home>/y",
        ] {
            assert_eq!(index.places(path, &[]), Where::Everywhere, "{path}");
        }
        assert_eq!(index.places("<base>/x", &["../y"]), Where::Everywhere);
    }

    #[test]
    fn a_path_outside_the_prefix_is_tried_once() {
        let index = WineIndex::build(&[], std::iter::empty());
        assert_eq!(index.places("<xdgData>/game", &[]), Where::Once);
        assert_eq!(index.places("/opt/game/saves", &[]), Where::Once);
    }
}
