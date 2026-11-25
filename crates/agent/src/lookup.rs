//! Where a game's save files are on this disk: what Ludusavi's `scan_game_for_backup`
//! (`src/scan.rs`) finds, without its SHA-1 of every file, and without trying every path in
//! every Wine prefix (see `wine_index`). The paths themselves come from its `parse_paths`.

// A `StrictPath` hashes and compares its text only, not the canonical path it caches.
#![allow(clippy::mutable_key_type)]

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    ops::ControlFlow,
    path::PathBuf,
};

use ludusavi::{
    path::StrictPath,
    prelude::SKIP,
    resource::{
        config::{BackupFilter, Root, ToggledPaths},
        manifest::{Game, Os, Store},
    },
    scan::{Launchers, SteamShortcuts, parse_paths},
};

use crate::wine_index::{Where, WineIndex};

/// What a lookup needs besides the game.
pub struct Lookup<'a> {
    pub roots: &'a [Root],
    pub app_dir: &'a StrictPath,
    pub launchers: &'a Launchers,
    pub filter: &'a BackupFilter,
    pub ignored: &'a ToggledPaths,
    pub steam_shortcuts: &'a SteamShortcuts,
    pub wine: &'a Wine,
}

/// Folders every Wine prefix may have, whatever is installed in it (Wine, Proton, umu).
const WINE_FOLDERS: &[&str] = &["dosdevices", "shadercache", "gstreamer-1.0", "pfx"];

/// A Wine prefix without a folder of its own next to Wine's: no launcher has games there, so
/// Ludusavi's launcher scan, which compares every folder with every game's name, finds nothing.
pub fn is_bare_prefix(root: &Root) -> bool {
    if root.store() != Store::OtherWine || root.is_game_specific() {
        return false;
    }
    let Some(entries) = root.path().as_std_path_buf().ok().and_then(|p| p.read_dir().ok()) else {
        return false;
    };
    entries
        .map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            let wines = name.starts_with("drive_") || WINE_FOLDERS.contains(&name.as_str());
            (wines || !e.file_type().ok()?.is_dir()).then_some(())
        })
        .all(|wines| wines.is_some())
}

/// The bare Wine prefixes among the roots (by position), and what is in them.
#[derive(Default)]
pub struct Wine {
    prefixes: Vec<usize>,
    index: WineIndex,
}

impl Wine {
    /// For looking up `games`; the less they need, the less is listed.
    pub fn of<'a>(roots: &[Root], games: impl Iterator<Item = &'a Game>) -> Self {
        let (prefixes, paths): (Vec<usize>, Vec<PathBuf>) = roots
            .iter()
            .enumerate()
            .filter(|(_, root)| is_bare_prefix(root))
            .filter_map(|(i, root)| Some((i, root.path().as_std_path_buf().ok()?)))
            .unzip();
        Self {
            prefixes,
            index: WineIndex::build(&paths, games.flat_map(|g| g.files.keys().map(String::as_str))),
        }
    }
}

type Candidates = HashSet<(StrictPath, Option<bool>)>;

impl Lookup<'_> {
    /// Calls `found` with every save file, until it says to stop.
    pub fn visit(&self, name: &str, game: &Game, mut found: impl FnMut(PathBuf) -> ControlFlow<()>) {
        for (path, case_sensitive) in self.candidates(name, game) {
            if self.filter.is_path_ignored(&path) {
                continue;
            }
            let matches = match case_sensitive {
                None => path.glob(),
                Some(cs) => path.glob_case_sensitive(cs),
            };
            for matched in matches {
                if self.visit_match(name, &matched, &mut found).is_break() {
                    return;
                }
            }
        }
    }

    pub fn files(&self, name: &str, game: &Game) -> Vec<PathBuf> {
        let mut files = BTreeSet::new();
        self.visit(name, game, |file| {
            files.insert(file);
            ControlFlow::Continue(())
        });
        files.into_iter().collect()
    }

    fn visit_match(
        &self,
        name: &str,
        matched: &StrictPath,
        found: &mut impl FnMut(PathBuf) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        if matched.is_file() {
            return self.kept(name, matched).map_or(ControlFlow::Continue(()), found);
        }
        let Ok(dir) = matched.as_std_path_buf() else {
            return ControlFlow::Continue(());
        };
        if !dir.is_dir() {
            return ControlFlow::Continue(());
        }
        for child in walkdir::WalkDir::new(dir)
            .max_depth(100)
            .follow_links(true)
            .into_iter()
            .flatten()
        {
            // As Ludusavi: names with a backslash are not supported.
            if !child.file_type().is_file() || child.path().to_string_lossy().contains('\\') {
                continue;
            }
            if let Some(file) = self.kept(name, &StrictPath::from(child.path())) {
                found(file)?;
            }
        }
        ControlFlow::Continue(())
    }

    /// The file's path, unless it is excluded.
    fn kept(&self, name: &str, file: &StrictPath) -> Option<PathBuf> {
        let key = file.interpreted().ok()?.rendered();
        let wanted = !self.filter.is_path_ignored(&key) && !self.ignored.is_ignored(name, &key);
        wanted.then(|| key.as_std_path_buf().ok()).flatten()
    }

    fn candidates(&self, name: &str, game: &Game) -> Candidates {
        let mut out = Candidates::new();
        let dummy = Root::new(SKIP, Store::Other);
        self.add_root(&mut out, name, game, &dummy, |_| true);
        let install_dirs: Vec<&str> = std::iter::once(name)
            .chain(game.install_dir.keys().map(String::as_str))
            .collect();
        for root in self.others() {
            self.add_root(&mut out, name, game, root, |_| true);
        }
        let mut by_prefix: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
        for path in game.files.keys() {
            let prefixes = match self.wine.index.places(path, &install_dirs) {
                Where::Everywhere => (0..self.wine.prefixes.len()).collect(),
                Where::Once => (0..self.wine.prefixes.len().min(1)).collect(),
                Where::In(prefixes) => prefixes,
            };
            for prefix in prefixes {
                by_prefix.entry(prefix).or_default().insert(path);
            }
        }
        for (prefix, paths) in by_prefix {
            let root = &self.roots[self.wine.prefixes[prefix]];
            self.add_root(&mut out, name, game, root, |path| paths.contains(path));
        }
        for prefix in self.own_prefixes(name, game) {
            if !game.registry.is_empty() {
                out.insert((prefix.joined("*.reg"), None));
            }
            let root = Root::new(prefix, Store::OtherWine);
            self.add_root(&mut out, name, game, &root, |_| true);
        }
        out
    }

    /// The roots that are not bare Wine prefixes: the only ones with launcher games.
    fn others(&self) -> impl Iterator<Item = &Root> {
        self.roots
            .iter()
            .enumerate()
            .filter(|(i, _)| self.wine.prefixes.binary_search(i).is_err())
            .map(|(_, root)| root)
    }

    /// The prefixes known to be this game's: from the manifest and the launchers.
    fn own_prefixes(&self, name: &str, game: &Game) -> Vec<StrictPath> {
        let mut prefixes: Vec<StrictPath> = game
            .wine_prefix
            .iter()
            .filter(|p| !p.trim().is_empty())
            .map(StrictPath::new)
            .collect();
        for root in self.others() {
            for prefix in self
                .launchers
                .get_game(root, name)
                .filter_map(|g| g.prefix.as_ref())
            {
                let pfx = prefix.joined("pfx");
                prefixes.push(prefix.clone());
                if pfx.exists() {
                    prefixes.push(pfx);
                }
            }
        }
        prefixes
    }

    fn add_root(
        &self,
        out: &mut Candidates,
        name: &str,
        game: &Game,
        root: &Root,
        wanted: impl Fn(&str) -> bool,
    ) {
        if root.path().raw().trim().is_empty() {
            return;
        }
        let ids = game.all_ids();
        let shortcut = self.steam_shortcuts.get(name);
        for (raw, data) in game.files.iter().filter(|(raw, _)| !raw.trim().is_empty()) {
            if !wanted(raw) {
                continue;
            }
            let mut add = |install_dir: Option<&str>, full: Option<&StrictPath>, os: Os| {
                let parsed = parse_paths(
                    raw,
                    data,
                    root,
                    install_dir,
                    full,
                    &ids,
                    self.app_dir,
                    shortcut,
                    os,
                );
                out.extend(parsed.into_iter().map(|(p, cs)| (p, Some(cs))));
            };
            let mut installs = self.launchers.get_game(root, name).peekable();
            if installs.peek().is_none() {
                for dir in std::iter::once(name).chain(game.install_dir.keys().map(String::as_str)) {
                    add(Some(dir), None, Os::HOST);
                }
            }
            for install in installs {
                let full = install.install_dir.as_ref();
                let dir = full.and_then(|dir| root.path().suffix_for(dir));
                add(dir.as_deref(), full, install.platform.unwrap_or(Os::HOST));
            }
        }
        if root.store() == Store::Steam {
            self.add_steam(out, game, root, &ids, shortcut.map(|s| s.id));
        }
    }

    fn add_steam(
        &self,
        out: &mut Candidates,
        game: &Game,
        root: &Root,
        ids: &ludusavi::resource::manifest::IdSet<'_>,
        shortcut: Option<u32>,
    ) {
        let steam = root.path().globbable();
        let basis = self.app_dir.globbable();
        let mut add = |path: String| {
            out.insert((StrictPath::relative(path, Some(basis.clone())), None));
        };
        for id in ids.steam(shortcut) {
            add(format!("{steam}/userdata/*/{id}/remote/"));
            if !self.filter.exclude_store_screenshots {
                add(format!("{steam}/userdata/*/760/remote/{id}/screenshots/*.*"));
            }
            if !game.registry.is_empty() {
                add(format!("{steam}/steamapps/compatdata/{id}/pfx/*.reg"));
            }
        }
    }
}
