mod common;

use common::{GAME, pc, server, server_contents};
use spielstand_agent::{Choice, Mode, Outcome};

#[tokio::test]
async fn saves_follow_the_player_between_two_pcs() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    let laptop = pc(&s, "laptop", false).await;

    desktop.write_save("level 1");
    desktop.agent.prepare_launch(GAME).await;
    desktop.write_save("level 2");
    assert_eq!(
        desktop.agent.after_session(GAME, 600).await.unwrap(),
        Outcome::Uploaded
    );

    let outcome = laptop.agent.prepare_launch(GAME).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(laptop.read_save().as_deref(), Some("level 2"));
    laptop.write_save("level 3");
    assert_eq!(
        laptop.agent.after_session(GAME, 900).await.unwrap(),
        Outcome::Uploaded
    );

    assert!(matches!(
        desktop.agent.prepare_launch(GAME).await,
        Outcome::Loaded(_)
    ));
    assert_eq!(desktop.read_save().as_deref(), Some("level 3"));
    assert_eq!(desktop.agent.prepare_launch(GAME).await, Outcome::UpToDate);
    assert!(
        desktop.ui.asked.lock().unwrap().is_empty(),
        "no questions on a clean hand-over"
    );

    let contents = server_contents(&s, GAME).await;
    for c in ["level 1", "level 2", "level 3"] {
        assert!(contents.iter().any(|x| x == c), "{c} lost");
    }
}

#[tokio::test]
async fn playing_on_both_asks_and_keeps_both() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    let laptop = pc(&s, "laptop", false).await;
    desktop.write_save("shared");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    laptop.agent.prepare_launch(GAME).await;

    // Laptop plays offline, desktop plays online.
    laptop.write_save("laptop progress");
    assert_eq!(
        laptop.offline().after_session(GAME, 60).await.unwrap(),
        Outcome::Offline
    );
    desktop.write_save("desktop progress");
    desktop.agent.after_session(GAME, 60).await.unwrap();

    // Back online, the laptop starts the game and chooses the desktop's state.
    laptop.ui.answers.lock().unwrap().push_back(Choice::Theirs);
    let outcome = laptop.agent.prepare_launch(GAME).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(laptop.read_save().as_deref(), Some("desktop progress"));
    assert_eq!(laptop.ui.asked.lock().unwrap().len(), 1);

    let contents = server_contents(&s, GAME).await;
    assert!(
        contents.iter().any(|c| c == "laptop progress"),
        "the laptop's branch must survive"
    );
    assert!(contents.iter().any(|c| c == "desktop progress"));

    // The conflict is closed; the next start is quiet.
    assert_eq!(laptop.agent.prepare_launch(GAME).await, Outcome::UpToDate);
}

#[tokio::test]
async fn keeping_mine_makes_it_the_head_for_everyone() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    let laptop = pc(&s, "laptop", false).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    laptop.agent.prepare_launch(GAME).await;
    laptop.write_save("laptop wins");
    laptop.offline().after_session(GAME, 1).await.unwrap();
    desktop.write_save("desktop");
    desktop.agent.after_session(GAME, 1).await.unwrap();

    laptop.ui.answers.lock().unwrap().push_back(Choice::Mine);
    assert_eq!(laptop.agent.prepare_launch(GAME).await, Outcome::Uploaded);
    assert_eq!(laptop.read_save().as_deref(), Some("laptop wins"));

    assert!(matches!(
        desktop.agent.sync_game(GAME, Mode::Background).await.unwrap(),
        Outcome::Loaded(_)
    ));
    assert_eq!(desktop.read_save().as_deref(), Some("laptop wins"));
}

#[tokio::test]
async fn no_answer_keeps_the_local_state_and_the_conflict_open() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    let laptop = pc(&s, "laptop", false).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    laptop.agent.prepare_launch(GAME).await;
    laptop.write_save("local");
    laptop.offline().after_session(GAME, 1).await.unwrap();
    desktop.write_save("remote");
    desktop.agent.after_session(GAME, 1).await.unwrap();

    assert!(matches!(
        laptop.agent.prepare_launch(GAME).await,
        Outcome::Conflict { .. }
    ));
    assert_eq!(laptop.read_save().as_deref(), Some("local"));
    // In the background the open branch is never overwritten.
    assert!(matches!(
        laptop.agent.sync_game(GAME, Mode::Background).await.unwrap(),
        Outcome::Conflict { .. }
    ));
    assert_eq!(laptop.read_save().as_deref(), Some("local"));
}

#[tokio::test]
async fn a_missed_session_end_is_caught_up() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    desktop.write_save("before crash");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    desktop.write_save("written, then power cut");
    assert_eq!(
        desktop.agent.sync_game(GAME, Mode::Background).await.unwrap(),
        Outcome::Uploaded
    );
    assert!(
        server_contents(&s, GAME)
            .await
            .iter()
            .any(|c| c == "written, then power cut")
    );
}

#[tokio::test]
async fn steam_cloud_games_are_backed_up_but_not_overwritten() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", true).await;
    let laptop = pc(&s, "laptop", true).await;
    desktop.write_save("cloud game");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    assert_eq!(
        laptop.agent.prepare_launch(GAME).await,
        Outcome::SkippedSteamCloud
    );
    assert_eq!(laptop.read_save(), None);
}

#[tokio::test]
async fn loading_keeps_what_was_on_disk_and_moves_extras_to_the_trash() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    let laptop = pc(&s, "laptop", false).await;
    desktop.write_save("the real one");
    desktop.agent.after_session(GAME, 1).await.unwrap();

    // The laptop has an unrelated extra file and no base: that is a conflict, choose theirs.
    let extra = laptop.home.join(".local/share/EldenRing/old-backup.sl2");
    std::fs::create_dir_all(extra.parent().unwrap()).unwrap();
    std::fs::write(&extra, "stray").unwrap();
    laptop.ui.answers.lock().unwrap().push_back(Choice::Theirs);
    laptop.agent.prepare_launch(GAME).await;
    assert_eq!(laptop.read_save().as_deref(), Some("the real one"));
    assert!(!extra.exists());
    let trashed = std::fs::read_dir(laptop.agent.dirs.trash()).unwrap().count();
    assert_eq!(trashed, 1);
    assert!(
        server_contents(&s, GAME).await.iter().any(|c| c == "stray"),
        "the stray state was snapshotted"
    );
}

#[tokio::test]
async fn the_server_being_away_never_blocks_the_game() {
    let s = server().await;
    let desktop = pc(&s, "zentrale", false).await;
    desktop.write_save("before the trip");
    let offline = desktop.offline();
    // The state found before the game starts is kept even without the server...
    assert_eq!(offline.prepare_launch(GAME).await, Outcome::Offline);
    desktop.write_save("offline play");
    // ...and so is the one the session leaves behind.
    assert_eq!(offline.after_session(GAME, 5).await.unwrap(), Outcome::Offline);
    assert_eq!(desktop.agent.store.pending_count().await.unwrap(), 2);
    desktop.agent.flush().await.unwrap();
    assert_eq!(desktop.agent.store.pending_count().await.unwrap(), 0);
    let contents = server_contents(&s, GAME).await;
    assert!(contents.iter().any(|c| c == "before the trip"));
    assert!(contents.iter().any(|c| c == "offline play"));
}

/// Keys and passwords are never a save: a path that leads there backs nothing up, and a version
/// holding such a file, whoever made it, is not written.
#[tokio::test]
async fn nothing_is_backed_up_from_or_written_into_the_folders_for_keys() {
    let s = server().await;
    let leaky = common::pc_with(&s, "leaky", |home| {
        let mut games = std::collections::BTreeMap::new();
        games.insert(GAME.to_string(), vec![home.join(".ssh")]);
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
    std::fs::create_dir_all(leaky.home.join(".ssh")).unwrap();
    std::fs::write(leaky.home.join(".ssh/id_ed25519"), "SECRET").unwrap();
    leaky.agent.after_session(GAME, 60).await.unwrap();
    assert_eq!(s.version_count(GAME).await, 0, "nothing was backed up");

    // A version made by hand with a file for `~/.ssh`.
    let file = s.dir.path().join("authorized_keys");
    std::fs::write(&file, "ssh-ed25519 AAAA intruder").unwrap();
    let blob = s.dir.path().join("v.tar.zst");
    let packed = spielstand_core::pack::pack(
        &[spielstand_core::pack::Source {
            portable: "<home>/.ssh/authorized_keys".into(),
            path: file,
        }],
        &blob,
    )
    .unwrap();
    let meta = spielstand_protocol::VersionMeta {
        id: spielstand_core::new_version_id(chrono::Utc::now()),
        game: GAME.into(),
        parent: None,
        device: leaky.agent.config.device_id.clone(),
        created_at: chrono::Utc::now(),
        trigger: spielstand_protocol::Trigger::Manual,
        session_seconds: None,
        content_hash: packed.content_hash,
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix: None,
        title: None,
    };
    leaky.agent.client.upload(&meta, &blob).await.unwrap();
    let deck = pc(&s, "deck", false).await;
    assert!(deck.agent.sync_game(GAME, Mode::Background).await.is_err());
    assert!(!deck.home.join(".ssh/authorized_keys").exists(), "not written");
}
