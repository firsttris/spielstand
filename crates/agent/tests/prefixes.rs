//! Games with their own Wine prefix (Prefixr, game folders): saves are found inside the prefix,
//! counted once despite linked profile folders, and land in the right prefix on another PC.

mod common;

use std::path::{Path, PathBuf};

use common::{Pc, Server, make_prefix, pc_with, server, syncthing_folder};
use spielstand_agent::{Mode, Outcome, engine::FakeScanner, settings::Sources};
use spielstand_core::portable::Anchors;
use spielstand_protocol::GameProblem;

const HADES: &str = "Hades";
const IN_PROFILE: &str = "AppData/Roaming/Supergiant Games/Hades";

fn prefixr_config(home: &Path, games: &[(&str, PathBuf)]) -> PathBuf {
    let path = home.join(".config/com.tristan.prefixr/config.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let games: Vec<serde_json::Value> = games
        .iter()
        .map(|(name, prefix)| {
            serde_json::json!({ "id": "x", "name": name, "exe_path": "/x.exe", "prefix_path": prefix,
                                "runner_id": "ge-proton", "notes": "" })
        })
        .collect();
    std::fs::write(
        &path,
        serde_json::json!({ "prefixes": [], "games": games }).to_string(),
    )
    .unwrap();
    path
}

fn scanner(home: &Path) -> FakeScanner {
    let mut profile_saves = std::collections::BTreeMap::new();
    profile_saves.insert(HADES.to_string(), IN_PROFILE.to_string());
    FakeScanner {
        anchors: Anchors {
            home: home.to_path_buf(),
            roots: vec![],
        },
        profile_saves,
        ..Default::default()
    }
}

/// A PC with Prefixr and Hades in `prefix` (relative to home), with the given profiles.
async fn prefixr_pc(s: &Server, name: &str, prefix: &str, users: &[&str], stellwerk: bool) -> (Pc, PathBuf) {
    let mut at = PathBuf::new();
    let pc = pc_with(s, name, |home| {
        at = home.join(prefix);
        make_prefix(&at, users);
        let config = prefixr_config(home, &[(HADES, at.clone())]);
        let stellwerk_dir = home.join(".local/share/stellwerk-agent");
        if stellwerk {
            std::fs::create_dir_all(&stellwerk_dir).unwrap();
            std::fs::write(stellwerk_dir.join("agent.json"), "{}").unwrap();
        }
        (
            scanner(home),
            Sources {
                prefixr_config: Some(config),
                stellwerk_dir: Some(stellwerk_dir),
            },
        )
    })
    .await;
    (pc, at)
}

fn write(prefix: &Path, user: &str, content: &str) -> PathBuf {
    let file = prefix
        .join("drive_c/users")
        .join(user)
        .join(IN_PROFILE)
        .join("Profile1.sav");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, content).unwrap();
    file
}

fn read(prefix: &Path, user: &str) -> Option<String> {
    std::fs::read_to_string(
        prefix
            .join("drive_c/users")
            .join(user)
            .join(IN_PROFILE)
            .join("Profile1.sav"),
    )
    .ok()
}

#[tokio::test]
async fn a_prefixr_game_moves_to_a_prefix_elsewhere_on_another_pc() {
    let s = server().await;
    // Desktop: prefix in ~/Games, profile linked as Prefixr does it.
    let (desktop, desktop_prefix) = prefixr_pc(
        &s,
        "desktop",
        "Games/Hades/prefix",
        &["tristan->steamuser"],
        false,
    )
    .await;
    // Deck: the game on the SD card, a fresh prefix without saves yet.
    let (deck, deck_prefix) =
        prefixr_pc(&s, "deck", "sd/Games/Hades/prefix", &["deck->steamuser"], false).await;

    write(&desktop_prefix, "steamuser", "run 1");
    desktop.agent.after_session(HADES, 60).await.unwrap();
    let detail = s.game(HADES).await;
    let files = detail["versions"][0]["meta"]["files"].as_array().unwrap();
    assert_eq!(
        files.len(),
        1,
        "the linked profile does not count twice: {files:?}"
    );
    assert_eq!(
        files[0]["path"],
        format!("<prefix>/drive_c/users/<user>/{IN_PROFILE}/Profile1.sav")
    );

    assert!(
        deck.agent.games_to_follow().await.contains(&HADES.to_string()),
        "installed through Prefixr, no saves yet: the deck follows it"
    );
    let outcome = deck.agent.prepare_launch(HADES).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(read(&deck_prefix, "steamuser").as_deref(), Some("run 1"));
    assert_eq!(
        read(&deck_prefix, "deck").as_deref(),
        Some("run 1"),
        "seen through the link too"
    );
    let report = deck.agent.report().await.unwrap();
    let hades = report.games.iter().find(|g| g.game == HADES).unwrap();
    assert_eq!(hades.source.as_deref(), Some("prefixr"));
    assert_eq!(hades.prefix.as_deref(), Some(deck_prefix.to_str().unwrap()));
    assert!(report.prefixr);
}

#[tokio::test]
async fn game_folders_from_the_web_ui_are_searched_for_prefixes() {
    let s = server().await;
    let desktop = pc_with(&s, "desktop", |home| {
        make_prefix(&home.join("Games/Hades/pfx"), &["steamuser"]);
        (scanner(home), Sources::default())
    })
    .await;
    write(&desktop.home.join("Games/Hades/pfx"), "steamuser", "x");
    assert!(
        desktop.agent.scanner.save_files(HADES).is_empty(),
        "not looked at yet"
    );

    let devices: serde_json::Value = s
        .admin(reqwest::Method::GET, "/api/devices")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = devices[0]["id"].as_str().unwrap();
    let res = s
        .admin(reqwest::Method::PUT, &format!("/api/devices/{id}/settings"))
        .json(&serde_json::json!({ "gameFolders": ["~/Games"] }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    desktop.agent.refresh_settings().await;
    assert_eq!(desktop.agent.scanner.save_files(HADES).len(), 1);
    desktop.agent.observe(HADES).await.unwrap();
    let report = desktop.agent.report().await.unwrap();
    assert_eq!(report.games[0].source.as_deref(), Some("gameFolder"));

    let bad = s
        .admin(reqwest::Method::PUT, &format!("/api/devices/{id}/settings"))
        .json(&serde_json::json!({ "gameFolders": ["relative/path"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
}

#[tokio::test]
async fn a_plain_wine_profile_named_after_the_user_lands_in_the_other_users_profile() {
    let s = server().await;
    let (desktop, desktop_prefix) =
        prefixr_pc(&s, "desktop", "Games/Hades/prefix", &["tristan"], false).await;
    let (deck, deck_prefix) = prefixr_pc(&s, "deck", "Games/Hades/prefix", &["deck"], false).await;
    write(&desktop_prefix, "tristan", "from tristan");
    desktop.agent.after_session(HADES, 60).await.unwrap();
    deck.agent.sync_game(HADES, Mode::Background).await.unwrap();
    assert_eq!(read(&deck_prefix, "deck").as_deref(), Some("from tristan"));
    assert!(!deck_prefix.join("drive_c/users/tristan").exists());
}

#[tokio::test]
async fn without_a_prefix_nothing_is_written_and_the_web_ui_can_choose_one() {
    let s = server().await;
    let (desktop, desktop_prefix) =
        prefixr_pc(&s, "desktop", "Games/Hades/prefix", &["steamuser"], false).await;
    // The deck has the game in a prefix Prefixr does not know and no game folder points at.
    let deck = pc_with(&s, "deck", |home| {
        make_prefix(&home.join("Other/Hades"), &["steamuser"]);
        (scanner(home), Sources::default())
    })
    .await;
    write(&desktop_prefix, "steamuser", "run 1");
    desktop.agent.after_session(HADES, 60).await.unwrap();

    assert!(deck.agent.sync_game(HADES, Mode::Background).await.is_err());
    assert_eq!(deck.agent.problem(HADES), Some(GameProblem::NoPrefix));
    deck.agent.report().await.unwrap();
    let detail = s.game(HADES).await;
    let deck_row = detail["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["deviceName"] == "deck")
        .unwrap()
        .clone();
    assert_eq!(deck_row["report"]["problem"]["kind"], "noPrefix");

    let res = s
        .admin(reqwest::Method::PUT, &format!("/api/games/{HADES}/prefix"))
        .json(&serde_json::json!({ "device": deck_row["deviceId"], "prefix": "~/Other/Hades" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    deck.agent.refresh_settings().await;
    assert!(matches!(
        deck.agent.sync_game(HADES, Mode::Background).await.unwrap(),
        Outcome::Loaded(_)
    ));
    assert_eq!(
        read(&deck.home.join("Other/Hades"), "steamuser").as_deref(),
        Some("run 1")
    );
    assert_eq!(deck.agent.problem(HADES), None);
}

#[tokio::test]
async fn two_prefixr_games_of_the_same_title_are_a_question_not_a_guess() {
    let s = server().await;
    let (desktop, desktop_prefix) =
        prefixr_pc(&s, "desktop", "Games/Hades/prefix", &["steamuser"], false).await;
    let deck = pc_with(&s, "deck", |home| {
        let a = home.join("A/Hades");
        let b = home.join("B/HADES");
        make_prefix(&a, &["steamuser"]);
        make_prefix(&b, &["steamuser"]);
        let config = prefixr_config(home, &[(HADES, a), ("HADES", b)]);
        (
            scanner(home),
            Sources {
                prefixr_config: Some(config),
                stellwerk_dir: None,
            },
        )
    })
    .await;
    write(&desktop_prefix, "steamuser", "run 1");
    desktop.agent.after_session(HADES, 60).await.unwrap();
    assert!(deck.agent.sync_game(HADES, Mode::Background).await.is_err());
    assert!(matches!(
        deck.agent.problem(HADES),
        Some(GameProblem::SeveralPrefixes { candidates }) if candidates.len() == 2
    ));
}

#[tokio::test]
async fn stellwerk_gets_the_save_files_of_games_spielstand_syncs() {
    let s = server().await;
    let (desktop, prefix) =
        prefixr_pc(&s, "desktop", "Games/Hades/prefix", &["tristan->steamuser"], true).await;
    syncthing_folder(&desktop.home.join("Games"), "");
    let save = write(&prefix, "steamuser", "x");
    desktop.agent.observe(HADES).await.unwrap();
    desktop.agent.report().await.unwrap();
    let drop_in = desktop
        .home
        .join(".local/share/stellwerk-agent/ignore.d/spielstand.list");
    let listed: Vec<String> = std::fs::read_to_string(&drop_in)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    assert_eq!(listed, vec![save.canonicalize().unwrap().display().to_string()]);

    s.set_mode(HADES, Some("backup"), &[&desktop]).await;
    desktop.agent.report().await.unwrap();
    let listed = std::fs::read_to_string(&drop_in).unwrap();
    assert!(
        listed.lines().all(|l| l.starts_with('#')),
        "a game in backup mode stays with Stellwerk"
    );
}

/// Hades moved to a new prefix; the old one, still in Prefixr's library under another entry,
/// holds an old save. The game's own prefix decides, not the number or order of save files:
/// the deck gets the current run, and the desktop's old prefix is neither backed up as Hades
/// nor touched.
#[tokio::test]
async fn an_old_prefix_with_an_old_save_does_not_win_over_the_games_own() {
    let s = server().await;
    let mut old = PathBuf::new();
    let mut own = PathBuf::new();
    let desktop = pc_with(&s, "desktop", |home| {
        own = home.join("Games/Hades/prefix");
        // Sorts after the own prefix: a tie used to pick it.
        old = home.join("Games/Old/Hades/prefix");
        make_prefix(&own, &["steamuser"]);
        make_prefix(&old, &["steamuser"]);
        let config = prefixr_config(
            home,
            &[(HADES, own.clone()), ("Hades (before the move)", old.clone())],
        );
        (
            scanner(home),
            Sources {
                prefixr_config: Some(config),
                stellwerk_dir: None,
            },
        )
    })
    .await;
    let (deck, deck_prefix) = prefixr_pc(&s, "deck", "sd/Games/Hades/prefix", &["steamuser"], false).await;
    write(&old, "steamuser", "old run, months ago");
    write(&own, "steamuser", "current run");

    desktop.agent.after_session(HADES, 60).await.unwrap();
    let contents = common::server_contents(&s, HADES).await;
    assert!(
        !contents.contains(&"old run, months ago".to_string()),
        "{contents:?}"
    );
    let outcome = deck.agent.prepare_launch(HADES).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(read(&deck_prefix, "steamuser").as_deref(), Some("current run"));

    write(&deck_prefix, "steamuser", "played on the deck");
    deck.agent.after_session(HADES, 60).await.unwrap();
    let outcome = desktop.agent.prepare_launch(HADES).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(read(&own, "steamuser").as_deref(), Some("played on the deck"));
    assert_eq!(
        read(&old, "steamuser").as_deref(),
        Some("old run, months ago"),
        "the old prefix is left alone"
    );
}
