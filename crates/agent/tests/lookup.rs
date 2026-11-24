//! The agent's lookup finds exactly what Ludusavi's own scan finds.

use std::{collections::BTreeSet, path::Path};

use ludusavi::{
    path::StrictPath,
    resource::{
        config::{BackupFilter, Root, ToggledPaths, ToggledRegistry},
        manifest::{Manifest, Store},
    },
    scan::{Launchers, SteamShortcuts, TitleFinder, scan_game_for_backup},
};
use spielstand_agent::lookup::{Lookup, Wine};

const MANIFEST: &str = r#"
Roaming: { files: { <winAppData>/Maker/Game: {} } }
Roaming Glob: { files: { <winAppData>/*/Game/slot*.sav: {} } }
Roaming Case: { files: { <winAppData>/MAKER/game: {} } }
Old Profile: { files: { <winDocuments>/Old Game: {} } }
Documents: { files: { <winDocuments>/My Games/Game/<storeUserId>/a.sav: {} } }
Local: { files: { <winLocalAppData>/Local Game: {} } }
Old Local: { files: { <winLocalAppData>/Old Local: {} } }
Low: { files: { <winLocalAppDataLow>/Studio/Low: {} } }
Saved Games: { files: { <home>/Saved Games/SG: {} } }
Home Roaming: { files: { <home>/AppData/Roaming/Maker/Game: {} } }
Home Low: { files: { <home>/AppData/LocalLow/Studio/Low: {} } }
Home Documents: { files: { <home>/Documents/My Games/Game: {} } }
Public: { files: { <winPublic>/Documents/Pub: {} } }
Program Data: { files: { <winProgramData>/PD/x.dat: {} } }
Windows: { files: { <winDir>/game.ini: {} } }
Drive D: { files: { <winAppData>/DriveD: {} } }
In Prefix: { files: { <base>/saves: {}, <root>/<game>/other: {} }, installDir: { InPrefixDir: {} } }
Root Path: { files: { <root>/drive_c/GOG Games/Root/save: {} } }
Profile Name: { files: { "<root>/drive_c/users/<osUserName>/Named": {} } }
Unix: { files: { <xdgData>/nothing-here-spielstand-test: {}, <home>/.config/Unix Game: {} } }
Nowhere: { files: { <winAppData>/Nowhere: {}, <winAppData>: { when: [] } } }
Missing: { files: { <winDocuments>/Missing: {}, <base>/missing: {} } }
Registry: { files: {}, registry: { "HKEY_CURRENT_USER/Software/X": {} } }
"#;

fn put(root: &Path, file: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, file).unwrap();
}

fn prefixes(dir: &Path) -> Vec<Root> {
    let a = dir.join("a");
    let user = "drive_c/users/steamuser";
    for file in [
        "AppData/Roaming/Maker/Game/slot1.sav",
        "AppData/Roaming/Other/Game/slot2.sav",
        "Documents/My Games/Game/1234/a.sav",
        "AppData/Local/Local Game/x/y.sav",
        "AppData/LocalLow/Studio/Low/z",
        "Saved Games/SG/s",
        "Named/n",
        ".config/Unix Game/c",
    ] {
        put(&a, &format!("{user}/{file}"));
    }
    put(&a, "drive_c/ProgramData/PD/x.dat");
    put(&a, "drive_c/windows/game.ini");
    put(&a, "drive_c/GOG Games/Root/save/1");
    std::os::unix::fs::symlink("steamuser", a.join("drive_c/users/tris")).unwrap();
    let b = dir.join("b [1]");
    put(&b, "drive_c/users/tris/My Documents/Old Game/1");
    put(
        &b,
        "drive_c/users/tris/Local Settings/Application Data/Old Local/1",
    );
    put(&b, "drive_c/users/Public/Documents/Pub/1");
    put(&b, "drive_d/users/tris/AppData/Roaming/DriveD/1");
    put(&b, "InPrefixDir/saves/1");
    put(&b, "In Prefix/other/1");
    // A link in the profile's place and one that leads nowhere.
    std::os::unix::fs::symlink(a.join("drive_c/users/steamuser"), b.join("drive_c/users/link")).unwrap();
    std::os::unix::fs::symlink("/nowhere", b.join("drive_c/users/broken")).unwrap();
    [a, b, dir.join("not-there")]
        .iter()
        .map(|p| Root::new(p.to_string_lossy().to_string(), Store::OtherWine))
        .collect()
}

#[test]
fn the_lookup_finds_what_ludusavi_finds() {
    let dir = tempfile::tempdir().unwrap();
    let mut roots = prefixes(dir.path());
    put(&dir.path().join("home"), ".config/Unix Game/settings");
    let home = dir.path().join("home").to_string_lossy().to_string();
    roots.insert(1, Root::new(home, Store::OtherHome));
    let manifest: Manifest = serde_yaml::from_str(MANIFEST).unwrap();
    let app_dir = StrictPath::from(dir.path().join("app"));
    let config = Default::default();
    let finder = TitleFinder::new(&config, &manifest, Default::default());
    let names: Vec<String> = manifest.0.keys().cloned().collect();
    let launchers = Launchers::scan(&roots, &manifest, &names, &finder, None);
    let (filter, ignored, shortcuts) = (
        BackupFilter::default(),
        ToggledPaths::default(),
        SteamShortcuts::default(),
    );
    let mut with_saves = 0;
    for (name, game) in &manifest.0 {
        let theirs: BTreeSet<_> = scan_game_for_backup(
            game,
            name,
            &roots,
            &app_dir,
            &launchers,
            &filter,
            None,
            &ignored,
            &ToggledRegistry::default(),
            None,
            &[],
            false,
            &shortcuts,
            false,
        )
        .found_files
        .keys()
        .map(|p| p.as_std_path_buf().unwrap())
        .collect();
        for wine in [
            Wine::of(&roots, std::iter::once(game)),
            Wine::of(&roots, manifest.0.values()),
        ] {
            let lookup = Lookup {
                roots: &roots,
                app_dir: &app_dir,
                launchers: &launchers,
                filter: &filter,
                ignored: &ignored,
                steam_shortcuts: &shortcuts,
                wine: &wine,
            };
            let ours: BTreeSet<_> = lookup.files(name, game).into_iter().collect();
            assert_eq!(ours, theirs, "{name}");
        }
        with_saves += usize::from(!theirs.is_empty());
    }
    assert!(with_saves >= 15, "the fixture has saves to find: {with_saves}");
}
