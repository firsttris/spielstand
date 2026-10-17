//! The real Ludusavi engine against fixture folders: a native Linux save and a Wine prefix.

use std::collections::BTreeSet;

use ludusavi::{
    path::StrictPath,
    resource::{
        config::{Config, Root},
        manifest::{Manifest, Store},
    },
};
use spielstand_agent::engine::{Ludusavi, Scanner};

#[test]
fn finds_native_and_wine_saves() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let native = home.join(".local/share/DemoGame");
    std::fs::create_dir_all(&native).unwrap();
    std::fs::write(native.join("save1.dat"), "native").unwrap();
    let prefix = dir.path().join("Games/wine-game");
    let roaming = prefix.join("drive_c/users/steamuser/AppData/Roaming/WineGame");
    std::fs::create_dir_all(&roaming).unwrap();
    std::fs::write(roaming.join("slot.sav"), "wine").unwrap();

    let manifest: Manifest = serde_yaml::from_str(
        r#"
Demo Game:
  files:
    <home>/.local/share/DemoGame:
      tags: [save]
  cloud:
    steam: true
Wine Game:
  files:
    <winAppData>/WineGame:
      tags: [save]
Not Installed:
  files:
    <home>/.local/share/Nothing:
      tags: [save]
"#,
    )
    .unwrap();
    let config = Config {
        roots: vec![
            Root::new(home.to_string_lossy().to_string(), Store::OtherHome),
            Root::new(prefix.to_string_lossy().to_string(), Store::OtherWine),
        ],
        ..Config::default()
    };
    let engine = Ludusavi::with(config, manifest, StrictPath::from(dir.path().join("app")));

    let games: BTreeSet<String> = engine.games_with_saves().into_iter().collect();
    assert_eq!(
        games,
        ["Demo Game", "Wine Game"].into_iter().map(String::from).collect()
    );
    assert_eq!(engine.save_files("Demo Game"), vec![native.join("save1.dat")]);
    assert_eq!(engine.save_files("Wine Game"), vec![roaming.join("slot.sav")]);
    assert!(engine.steam_cloud("Demo Game"));
    assert!(!engine.steam_cloud("Wine Game"));
    let anchors = engine.anchors();
    assert!(anchors.roots.iter().any(|(store, _)| store == "otherWine"));
}

/// Prefixr and game-folder prefixes are not in Ludusavi's config: they are added at run time.
/// A profile linked to `steamuser` (as Proton and Prefixr make it) is found twice by Ludusavi's
/// `users/*`; reading the disk counts the file once, under `<user>`.
#[test]
fn finds_saves_in_prefixes_added_later_and_counts_linked_profiles_once() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("Games/Hades/prefix");
    let users = prefix.join("drive_c/users");
    let roaming = users.join("steamuser/AppData/Roaming/Supergiant Games/Hades");
    std::fs::create_dir_all(&roaming).unwrap();
    std::os::unix::fs::symlink("steamuser", users.join("tristan")).unwrap();
    std::fs::write(prefix.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
    std::fs::write(roaming.join("Profile1.sav"), "hades").unwrap();

    let manifest: Manifest = serde_yaml::from_str(
        r#"
Hades:
  files:
    <winAppData>/Supergiant Games/Hades:
      tags: [save]
"#,
    )
    .unwrap();
    let engine = Ludusavi::with(
        Config::default(),
        manifest,
        StrictPath::from(dir.path().join("app")),
    );
    assert!(engine.save_files("Hades").is_empty(), "not a root yet");

    engine.configure(vec![prefix.clone()], vec![dir.path().join("Games")]);
    let found = engine.save_files("Hades");
    assert!(!found.is_empty(), "the configured prefix is scanned");
    assert_eq!(engine.games_with_saves(), vec!["Hades".to_string()]);
    assert_eq!(engine.find_title("HADES").as_deref(), Some("Hades"));
    assert!(engine.anchors().roots.iter().any(|(store, _)| store == "games"));

    let disk = spielstand_agent::disk::read(&engine, "Hades").unwrap();
    assert_eq!(disk.files.len(), 1, "{:?}", disk.files);
    assert_eq!(
        disk.files[0].path,
        "<prefix>/drive_c/users/<user>/AppData/Roaming/Supergiant Games/Hades/Profile1.sav"
    );
    assert_eq!(disk.prefix.as_deref(), Some(prefix.as_path()));
}

/// Writing a version into a save folder Ludusavi scans as a whole: the agent's own staging file
/// appears in that scan for a moment and must not be taken for a stray save.
#[test]
fn a_version_is_written_into_a_folder_ludusavi_scans_as_a_whole() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("Games/Hades/prefix");
    let saves = prefix.join("drive_c/users/steamuser/AppData/Roaming/Supergiant Games/Hades");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(prefix.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
    std::fs::write(saves.join("Profile1.sav"), "old").unwrap();
    let manifest: Manifest = serde_yaml::from_str(
        "Hades:\n  files:\n    <winAppData>/Supergiant Games/Hades:\n      tags: [save]\n",
    )
    .unwrap();
    let engine = Ludusavi::with(
        Config::default(),
        manifest,
        StrictPath::from(dir.path().join("app")),
    );
    engine.configure(vec![prefix.clone()], vec![]);

    // A version holding "new", packed from a copy of the folder.
    std::fs::write(saves.join("Profile1.sav"), "new").unwrap();
    let disk = spielstand_agent::disk::read(&engine, "Hades").unwrap();
    let blob = dir.path().join("v.tar.zst");
    let packed = spielstand_core::pack::pack(&disk.sources, &blob).unwrap();
    std::fs::write(saves.join("Profile1.sav"), "old").unwrap();
    std::fs::write(saves.join(".Profile1.sav.spielstand-new"), "left over").unwrap();
    let meta = spielstand_protocol::VersionMeta {
        id: "v".into(),
        game: "Hades".into(),
        parent: None,
        device: "d".into(),
        created_at: chrono::Utc::now(),
        trigger: spielstand_protocol::Trigger::Manual,
        session_seconds: None,
        content_hash: packed.content_hash,
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix: None,
    };
    let dirs = spielstand_agent::config::Dirs::under(&dir.path().join("agent"));
    spielstand_agent::disk::write(&dirs, &engine, &meta, &blob, Some(&prefix)).unwrap();
    assert_eq!(
        std::fs::read_to_string(saves.join("Profile1.sav")).unwrap(),
        "new"
    );
    assert!(!dirs.trash().exists(), "nothing was taken for a stray save");
}
