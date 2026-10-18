//! Backup mode, other programs copying saves, and the overwrite alert.

mod common;

use common::{GAME, pc, server, server_contents, syncthing_folder};
use spielstand_agent::{Mode, Outcome};
use spielstand_protocol::SaveMode;

#[tokio::test]
async fn backup_mode_never_writes_a_save() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    s.set_mode(GAME, Some("backup"), &[&desktop, &deck]).await;

    desktop.write_save("desktop 1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.write_save("deck old");
    // Sync mode would ask which state to keep; backup mode only records the deck's, as a
    // second state next to the desktop's.
    assert!(matches!(
        deck.agent.prepare_launch(GAME).await,
        Outcome::Conflict { .. }
    ));
    assert!(deck.ui.asked.lock().unwrap().is_empty(), "nothing is asked");
    assert_eq!(deck.read_save().as_deref(), Some("deck old"));
    assert_eq!(
        deck.agent.sync_game(GAME, Mode::Background).await.unwrap(),
        Outcome::UpToDate
    );
    assert_eq!(deck.read_save().as_deref(), Some("deck old"));
    let contents = server_contents(&s, GAME).await;
    assert!(contents.contains(&"desktop 1".to_string()) && contents.contains(&"deck old".to_string()));
}

#[tokio::test]
async fn a_newer_state_copied_by_another_program_is_adopted_not_uploaded_again() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    s.set_mode(GAME, Some("backup"), &[&desktop, &deck]).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.write_save("v1"); // Stellwerk copied it
    deck.agent.observe(GAME).await.unwrap();
    desktop.write_save("v2");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.write_save("v2");
    let before = s.version_count(GAME).await;
    assert_eq!(deck.agent.observe(GAME).await.unwrap(), Outcome::UpToDate);
    assert_eq!(s.version_count(GAME).await, before, "nothing new to store");
    let game = s.game(GAME).await;
    let deck_row = game["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["deviceName"] == "deck")
        .unwrap();
    assert_eq!(
        deck_row["isCurrent"], true,
        "the server knows the deck holds the newest state"
    );
    assert!(game["alerts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_older_state_over_a_newer_one_raises_an_alert_and_can_be_put_back() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    s.set_mode(GAME, Some("backup"), &[&desktop, &deck]).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.write_save("v1");
    deck.agent.observe(GAME).await.unwrap();
    deck.write_save("v2 on the deck");
    deck.agent.after_session(GAME, 60).await.unwrap();

    // The other program brings the desktop's older v1 back over the deck's v2.
    deck.write_save("v1");
    assert_eq!(deck.agent.observe(GAME).await.unwrap(), Outcome::UpToDate);
    assert_eq!(
        deck.read_save().as_deref(),
        Some("v1"),
        "backup mode does not fight back"
    );
    let game = s.game(GAME).await;
    let alerts = game["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0]["deviceName"], "deck");
    assert!(
        deck.ui
            .notices
            .lock()
            .unwrap()
            .iter()
            .any(|n| n.contains("älterer Spielstand"))
    );

    // One click in the web UI loads the lost state; the alert settles.
    let lost = alerts[0]["lostVersion"].as_str().unwrap().to_string();
    deck.agent.restore_version(GAME, &lost).await.unwrap();
    assert_eq!(deck.read_save().as_deref(), Some("v2 on the deck"));
    assert!(s.game(GAME).await["alerts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn sync_mode_puts_the_newest_state_back_after_an_overwrite() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.agent.prepare_launch(GAME).await;
    desktop.write_save("v2");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.agent.sync_game(GAME, Mode::Background).await.unwrap();
    assert_eq!(deck.read_save().as_deref(), Some("v2"));

    deck.write_save("v1"); // copied over by hand from an old backup
    let outcome = deck.agent.sync_game(GAME, Mode::Background).await.unwrap();
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(deck.read_save().as_deref(), Some("v2"));
    // Healed at once, so no alert stays open; the activity still tells what happened.
    assert!(s.game(GAME).await["alerts"].as_array().unwrap().is_empty());
    let activity: serde_json::Value = s
        .admin(reqwest::Method::GET, "/api/activity")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        activity
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "overwritten")
    );
}

#[tokio::test]
async fn saves_another_program_still_syncs_are_only_backed_up_until_excluded() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.write_save("deck own");
    let synced = deck.home.join(".local/share");
    syncthing_folder(&synced, "");

    let (mode, foreign) = deck.agent.effective_mode(GAME);
    assert_eq!(
        mode,
        SaveMode::Backup,
        "sync is configured, but Syncthing still writes there"
    );
    assert!(!foreign.unwrap().excluded);
    assert!(matches!(
        deck.agent.prepare_launch(GAME).await,
        Outcome::Conflict { .. }
    ));
    assert_eq!(deck.read_save().as_deref(), Some("deck own"), "nothing written");

    syncthing_folder(&synced, "/EldenRing/ER0000.sl2\n");
    let (mode, foreign) = deck.agent.effective_mode(GAME);
    assert_eq!(mode, SaveMode::Sync);
    assert!(foreign.unwrap().excluded);
}

#[tokio::test]
async fn the_device_report_says_where_saves_are_and_which_mode_applies() {
    let s = server().await;
    let deck = pc(&s, "deck", false).await;
    deck.write_save("x");
    syncthing_folder(&deck.home.join(".local/share"), "");
    deck.agent.observe(GAME).await.unwrap(); // the watcher saw the save
    let report = deck.agent.report().await.unwrap();
    let game = report.games.iter().find(|g| g.game == GAME).unwrap();
    assert_eq!(
        game.location.as_deref(),
        Some(deck.home.join(".local/share/EldenRing").to_str().unwrap())
    );
    assert_eq!(game.effective_mode, SaveMode::Backup);
    assert_eq!(game.foreign_sync.as_ref().unwrap().tool, "syncthing");

    let detail = s.game(GAME).await;
    // Without a version the game page does not exist yet; the device list shows the report.
    assert!(detail.get("locations").is_none() || detail["locations"].is_array());
    let devices: serde_json::Value = s
        .admin(reqwest::Method::GET, "/api/devices")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(devices[0]["prefixr"], false);
}

#[tokio::test]
async fn a_game_that_is_not_installed_here_gets_no_saves() {
    let s = server().await;
    let desktop = common::pc_with(&s, "desktop", |home| {
        let mut games = std::collections::BTreeMap::new();
        games.insert("Other Game".to_string(), vec![home.join(".local/share/Other")]);
        let scanner = spielstand_agent::engine::FakeScanner {
            anchors: spielstand_core::portable::Anchors {
                home: home.to_path_buf(),
                roots: vec![],
            },
            games,
            ..Default::default()
        };
        (scanner, spielstand_agent::settings::Sources::default())
    })
    .await;
    std::fs::create_dir_all(desktop.home.join(".local/share/Other")).unwrap();
    std::fs::write(desktop.home.join(".local/share/Other/s.dat"), "x").unwrap();
    desktop.agent.after_session("Other Game", 60).await.unwrap();
    let deck = pc(&s, "deck", false).await;
    let follow = deck.agent.games_to_follow().await;
    assert!(!follow.contains(&"Other Game".to_string()), "{follow:?}");
}

/// A custom game a PC proposed is only backed up there: a newer state from another PC is not
/// loaded until the web UI takes the game over for every PC.
#[tokio::test]
async fn a_proposed_custom_game_is_only_backed_up_until_it_is_taken_over() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    let game = spielstand_protocol::CustomGame {
        name: GAME.into(),
        files: vec!["<xdgData>/EldenRing".into()],
        ..Default::default()
    };
    let settings = desktop.agent.client.propose(&game).await.unwrap();
    desktop.agent.apply_settings(settings);
    assert_eq!(desktop.agent.effective_mode(GAME).0, SaveMode::Backup);
    assert!(desktop.agent.game_report(GAME).proposed);
    assert_eq!(
        deck.agent.effective_mode(GAME).0,
        SaveMode::Sync,
        "not the deck's proposal"
    );

    desktop.write_save("desktop 1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    deck.agent.sync_game(GAME, Mode::Background).await.unwrap();
    deck.write_save("deck 2");
    deck.agent.after_session(GAME, 60).await.unwrap();

    desktop.agent.sync_game(GAME, Mode::Background).await.unwrap();
    assert_eq!(
        desktop.read_save().as_deref(),
        Some("desktop 1"),
        "only backed up"
    );

    let res = s
        .admin(reqwest::Method::POST, "/api/custom-games/Elden%20Ring/take-over")
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    desktop.agent.refresh_settings().await;
    assert_eq!(desktop.agent.effective_mode(GAME).0, SaveMode::Sync);
    desktop.agent.sync_game(GAME, Mode::Background).await.unwrap();
    assert_eq!(
        desktop.read_save().as_deref(),
        Some("deck 2"),
        "taken over: synced"
    );
}
