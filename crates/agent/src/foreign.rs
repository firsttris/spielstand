//! Other programs that synchronise the folders saves are in (Syncthing, and Stellwerk, which
//! drives Syncthing). Two programs writing the same save file is how saves get lost, so a game
//! whose saves another program still syncs is only backed up until they are excluded there.
//!
//! Exclusions reach Stellwerk as a list of absolute paths in its agent's `ignore.d` folder; the
//! Stellwerk agent turns each path into an anchored Syncthing pattern for the folder it is in.
//! Whether a file is excluded is read back from the folder's `.stignore`, which Syncthing writes.

use std::path::{Path, PathBuf};

use spielstand_protocol::ForeignSync;

/// Syncthing's folder marker.
const MARKER: &str = ".stfolder";
const DROP_IN: &str = "spielstand.list";

/// The root of the Syncthing folder `path` lies in.
pub fn syncthing_folder(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.join(MARKER).is_dir())
        .map(Path::to_path_buf)
}

/// A path relative to a Syncthing folder as a pattern that matches exactly that path: rooted
/// with `/`, and every character Syncthing's glob reads specially escaped with a backslash
/// (Syncthing's escape character on Linux; `lib/ignore/ignore.go`, gobwas/glob). `None` for a
/// name Syncthing cannot match exactly: it trims every pattern line, so a trailing or leading
/// space would be lost.
pub fn anchored_pattern(relative: &str) -> Option<String> {
    if relative.is_empty() || relative != relative.trim() || relative.contains('\n') {
        return None;
    }
    let mut out = String::with_capacity(relative.len() + 8);
    out.push('/');
    for c in relative.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | '{' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    Some(out)
}

fn relative(path: &Path, folder: &Path) -> Option<String> {
    Some(path.strip_prefix(folder).ok()?.to_str()?.to_string())
}

/// `file` is excluded when `.stignore` has the anchored pattern of the file or of one of its
/// folders inside the Syncthing folder.
fn excluded(file: &Path, folder: &Path, ignore_lines: &[String]) -> bool {
    file.ancestors()
        .take_while(|p| *p != folder)
        .filter_map(|p| relative(p, folder))
        .filter_map(|rel| anchored_pattern(&rel))
        .any(|pattern| ignore_lines.iter().any(|line| line.trim() == pattern))
}

/// Where the Stellwerk agent keeps its data on this device.
pub fn stellwerk_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SPIELSTAND_STELLWERK_DIR") {
        return Some(PathBuf::from(dir));
    }
    Some(dirs::data_local_dir()?.join("stellwerk-agent"))
}

/// The Stellwerk agent is set up here (it keeps its pairing in `agent.json`).
pub fn stellwerk_installed(dir: &Path) -> bool {
    dir.join("agent.json").is_file()
}

/// Another program syncing the folder `files` lie in, and whether all of them are excluded.
pub fn check(files: &[PathBuf], stellwerk: bool) -> Option<ForeignSync> {
    let folder = files.iter().find_map(|f| syncthing_folder(f))?;
    let lines: Vec<String> = std::fs::read_to_string(folder.join(".stignore"))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let all = files
        .iter()
        .filter(|f| f.starts_with(&folder))
        .all(|f| excluded(f, &folder, &lines));
    Some(ForeignSync {
        tool: if stellwerk { "stellwerk" } else { "syncthing" }.into(),
        folder: folder.display().to_string(),
        excluded: all,
    })
}

/// The list Stellwerk reads: every save file Spielstand moves itself and that lies in a
/// Syncthing folder, sorted, one absolute path per line.
pub fn drop_in(files: &[PathBuf]) -> String {
    let mut lines: Vec<String> = files
        .iter()
        .filter(|f| syncthing_folder(f).is_some())
        .filter_map(|f| f.to_str().map(str::to_string))
        .collect();
    lines.sort();
    lines.dedup();
    let mut out = String::from(
        "# Written by Spielstand: save files it synchronises itself. Stellwerk excludes them\n\
         # from Syncthing, so only one program writes them. Do not edit; it is rewritten.\n",
    );
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Writes the list into Stellwerk's `ignore.d` when it changed; only where Stellwerk runs.
pub fn write_drop_in(stellwerk: &Path, content: &str) -> anyhow::Result<bool> {
    if !stellwerk_installed(stellwerk) {
        return Ok(false);
    }
    let dir = stellwerk.join("ignore.d");
    let path = dir.join(DROP_IN);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{DROP_IN}.tmp"));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_rooted_and_escaped() {
        assert_eq!(
            anchored_pattern("Games/Hades/prefix/drive_c/x.sav").as_deref(),
            Some("/Games/Hades/prefix/drive_c/x.sav")
        );
        assert_eq!(
            anchored_pattern("Games/Save [1] {a,b} *?.dat").as_deref(),
            Some(r"/Games/Save \[1\] \{a,b\} \*\?.dat")
        );
        assert_eq!(anchored_pattern(r"a\b").as_deref(), Some(r"/a\\b"));
        assert_eq!(anchored_pattern("trailing "), None);
        assert_eq!(anchored_pattern(""), None);
    }

    fn syncthing_folder_with(root: &Path, ignore: &str) {
        std::fs::create_dir_all(root.join(MARKER)).unwrap();
        std::fs::write(root.join(".stignore"), ignore).unwrap();
    }

    #[test]
    fn a_save_in_a_syncthing_folder_is_found_with_its_exclusion() {
        let tmp = tempfile::tempdir().unwrap();
        let games = tmp.path().join("Games");
        let save = games.join("Hades/prefix/drive_c/users/steamuser/Saves/Profile1.sav");
        std::fs::create_dir_all(save.parent().unwrap()).unwrap();
        std::fs::write(&save, "x").unwrap();

        assert_eq!(
            check(std::slice::from_ref(&save), false),
            None,
            "no Syncthing folder"
        );

        syncthing_folder_with(&games, "*.tmp\n");
        let found = check(std::slice::from_ref(&save), true).unwrap();
        assert_eq!(found.tool, "stellwerk");
        assert_eq!(found.folder, games.display().to_string());
        assert!(!found.excluded);

        syncthing_folder_with(
            &games,
            "*.tmp\n/Hades/prefix/drive_c/users/steamuser/Saves/Profile1.sav\n",
        );
        assert!(check(std::slice::from_ref(&save), false).unwrap().excluded);

        syncthing_folder_with(&games, "/Hades/prefix/drive_c/users/steamuser/Saves\n");
        assert!(
            check(std::slice::from_ref(&save), false).unwrap().excluded,
            "a folder above the file counts"
        );

        let other = save.with_file_name("Profile2.sav");
        std::fs::write(&other, "y").unwrap();
        syncthing_folder_with(
            &games,
            "/Hades/prefix/drive_c/users/steamuser/Saves/Profile1.sav\n",
        );
        assert!(
            !check(&[save, other], false).unwrap().excluded,
            "every file must be excluded"
        );
    }

    #[test]
    fn the_drop_in_lists_only_files_in_syncthing_folders_and_is_written_where_stellwerk_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let synced = tmp.path().join("Games/G/x.sav");
        let local = tmp.path().join("local/y.sav");
        for f in [&synced, &local] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "x").unwrap();
        }
        std::fs::create_dir_all(tmp.path().join("Games").join(MARKER)).unwrap();
        let list = drop_in(&[synced.clone(), local, synced.clone()]);
        let paths: Vec<&str> = list.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(paths, vec![synced.to_str().unwrap()]);

        let stellwerk = tmp.path().join("stellwerk-agent");
        assert!(!write_drop_in(&stellwerk, &list).unwrap(), "not installed");
        std::fs::create_dir_all(&stellwerk).unwrap();
        std::fs::write(stellwerk.join("agent.json"), "{}").unwrap();
        assert!(write_drop_in(&stellwerk, &list).unwrap());
        assert!(!write_drop_in(&stellwerk, &list).unwrap(), "unchanged");
        assert_eq!(
            std::fs::read_to_string(stellwerk.join("ignore.d").join(DROP_IN)).unwrap(),
            list
        );
    }
}
