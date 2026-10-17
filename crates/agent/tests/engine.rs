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
