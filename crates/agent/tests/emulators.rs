//! Switch games across emulators and devices: a save from Eden on the desktop loads into
//! Ryujinx on the Deck and back, each into its own profile, with the emulators' own files left
//! out; what Ryujinx needs first and which emulator to use are questions, not guesses.

mod common;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use common::{Pc, Server, pc_with_scanner, server};
use spielstand_agent::{
    Outcome,
    emulators::{Xdg, key, scanner::WithEmulators, yuzu::profile_folder},
    engine::{FakeScanner, Scanner},
    settings::Sources,
};
use spielstand_core::portable::Anchors;
use spielstand_protocol::GameProblem;

const ZELDA: &str = "0100F2C0115B6000";
const ZELDA_NAME: &str = "The Legend of Zelda: Tears of the Kingdom";

/// Eden with one profile, as it is after its first start.
fn eden(home: &Path, uuid: [u8; 16]) -> PathBuf {
    let nand = home.join(".local/share/eden/nand");
    let profiles = nand.join("system/save/8000000000000010/su/avators/profiles.dat");
    std::fs::create_dir_all(profiles.parent().unwrap()).unwrap();
    let mut data = vec![0u8; 0x650];
    data[0x10..0x20].copy_from_slice(&uuid);
    data[0x20..0x30].copy_from_slice(&uuid);
    std::fs::write(&profiles, data).unwrap();
    let names = home.join(".cache/eden/game_list");
    std::fs::create_dir_all(&names).unwrap();
    std::fs::write(
        names.join(format!("{ZELDA}.appname.txt")),
        format!("{ZELDA_NAME}\n"),
    )
    .unwrap();
    nand.join("user/save/0000000000000000")
        .join(profile_folder(&uuid))
        .join(ZELDA)
}

const RYU_PLAYER: &str = "00000000000000010000000000000000";

/// Ryujinx with its default profile; `save` gives it a save folder for Zelda (as starting the
/// game there once does).
fn ryujinx(home: &Path, save: bool) -> PathBuf {
    let base = home.join(".config/Ryujinx");
    std::fs::create_dir_all(base.join("system")).unwrap();
    std::fs::write(
        base.join("system/Profiles.json"),
        serde_json::json!({ "profiles": [{ "user_id": RYU_PLAYER, "name": "RyuPlayer" }], "last_opened": RYU_PLAYER })
            .to_string(),
    )
    .unwrap();
    let folder = base.join("bis/user/save/0000000000000001");
    if save {
        std::fs::create_dir_all(folder.join("0")).unwrap();
        let mut extra = vec![0u8; 0x200];
        extra[..8].copy_from_slice(&u64::from_str_radix(ZELDA, 16).unwrap().to_le_bytes());
        extra[8..16].copy_from_slice(&1u64.to_le_bytes());
        extra[0x20] = 1;
        std::fs::write(folder.join("ExtraData0"), extra).unwrap();
    }
    folder.join("0")
}

async fn switch_pc(s: &Server, name: &str, setup: impl FnOnce(&Path)) -> Pc {
    pc_with_scanner(s, name, |home| {
        setup(home);
        let disk = FakeScanner {
            anchors: Anchors {
                home: home.to_path_buf(),
                roots: vec![],
            },
            ..Default::default()
        };
        let scanner: Arc<dyn Scanner> = Arc::new(WithEmulators::new(Arc::new(disk), Xdg::of(home)));
        (scanner, Sources::default())
    })
    .await
}

fn write(dir: &Path, rel: &str, content: &str) {
    let file = dir.join(rel);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, content).unwrap();
}

fn read(dir: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(rel)).ok()
}

#[tokio::test]
async fn a_save_moves_from_eden_to_ryujinx_and_back() {
    let s = server().await;
    let mut eden_dir = PathBuf::new();
    let desktop = switch_pc(&s, "desktop", |home| eden_dir = eden(home, [7; 16])).await;
    let mut ryu_dir = PathBuf::new();
    let deck = switch_pc(&s, "deck", |home| ryu_dir = ryujinx(home, true)).await;
    let game = key(ZELDA);
    write(&eden_dir, "slot_0/progress.sav", "desktop: shrine 12");
    write(&eden_dir, ".yuzu_save_size", "emulator's own");
    write(&ryu_dir, "slot_0/progress.sav", "deck: fresh start");
    write(&ryu_dir, "slot_9/stale.sav", "only on the deck");

    desktop.agent.after_session(&game, 60).await.unwrap();
    let detail = s.game(&game).await;
    assert_eq!(detail["title"], ZELDA_NAME);
    let files: Vec<&str> = detail["versions"][0]["meta"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        ["<switch>/slot_0/progress.sav"],
        "the emulator's own file stays out"
    );

    // The deck has a Ryujinx save of its own: two states, both kept, a question.
    let outcome = deck.agent.prepare_launch(&game).await;
    assert!(matches!(outcome, Outcome::Conflict { .. }), "{outcome:?}");
    assert_eq!(
        read(&ryu_dir, "slot_0/progress.sav").as_deref(),
        Some("deck: fresh start")
    );
    // The desktop's state is loaded onto the deck from the web UI.
    let head = s.game(&game).await["head"].as_str().unwrap().to_string();
    deck.agent.restore_version(&game, &head).await.unwrap();
    assert_eq!(
        read(&ryu_dir, "slot_0/progress.sav").as_deref(),
        Some("desktop: shrine 12")
    );
    assert_eq!(
        read(&ryu_dir, "slot_9/stale.sav"),
        None,
        "the version has no such file"
    );
    assert!(
        common::server_contents(&s, &game)
            .await
            .contains(&"only on the deck".to_string()),
        "but it was backed up first"
    );

    write(&ryu_dir, "slot_0/progress.sav", "deck: shrine 13");
    deck.agent.after_session(&game, 60).await.unwrap();
    let outcome = desktop.agent.prepare_launch(&game).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(
        read(&eden_dir, "slot_0/progress.sav").as_deref(),
        Some("deck: shrine 13")
    );
    assert_eq!(
        read(&eden_dir, ".yuzu_save_size").as_deref(),
        Some("emulator's own")
    );
}

#[tokio::test]
async fn a_new_pc_with_eden_gets_the_save_in_its_own_profile() {
    let s = server().await;
    let mut there = PathBuf::new();
    let desktop = switch_pc(&s, "desktop", |home| there = eden(home, [7; 16])).await;
    let mut here = PathBuf::new();
    let laptop = switch_pc(&s, "laptop", |home| here = eden(home, [9; 16])).await;
    let game = key(ZELDA);
    write(&there, "progress.sav", "desktop run");
    desktop.agent.after_session(&game, 60).await.unwrap();

    assert!(here != there && !here.exists(), "another profile id, no save yet");
    assert!(laptop.agent.games_to_follow().await.contains(&game));
    let outcome = laptop.agent.prepare_launch(&game).await;
    assert!(matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    assert_eq!(read(&here, "progress.sav").as_deref(), Some("desktop run"));
}

#[tokio::test]
async fn ryujinx_needs_the_game_started_once() {
    let s = server().await;
    let mut there = PathBuf::new();
    let desktop = switch_pc(&s, "desktop", |home| there = eden(home, [7; 16])).await;
    let deck = switch_pc(&s, "deck", |home| {
        ryujinx(home, false);
    })
    .await;
    let game = key(ZELDA);
    write(&there, "progress.sav", "desktop run");
    desktop.agent.after_session(&game, 60).await.unwrap();

    assert!(
        deck.agent.games_to_follow().await.contains(&game),
        "an emulator is here"
    );
    let outcome = deck.agent.prepare_launch(&game).await;
    assert!(!matches!(outcome, Outcome::Loaded(_)), "{outcome:?}");
    let report = deck.agent.report().await.unwrap();
    let zelda = report.games.iter().find(|g| g.game == game).unwrap();
    assert_eq!(
        zelda.problem,
        Some(GameProblem::StartInEmulator {
            emulator: "Ryujinx".into()
        })
    );
}

#[tokio::test]
async fn two_emulators_with_a_save_are_a_question_until_one_is_chosen() {
    let s = server().await;
    let (mut eden_dir, mut ryu_dir) = (PathBuf::new(), PathBuf::new());
    let pc = switch_pc(&s, "desktop", |home| {
        eden_dir = eden(home, [7; 16]);
        ryu_dir = ryujinx(home, true);
    })
    .await;
    let game = key(ZELDA);
    write(&eden_dir, "progress.sav", "in Eden");
    write(&ryu_dir, "progress.sav", "in Ryujinx");

    let report = pc.agent.report().await.unwrap();
    let zelda = report.games.iter().find(|g| g.game == game).unwrap();
    assert!(
        matches!(&zelda.problem, Some(GameProblem::SeveralEmulators { candidates }) if candidates.len() == 2),
        "{:?}",
        zelda.problem
    );
    pc.agent.after_session(&game, 60).await.unwrap();
    assert_eq!(
        s.version_count(&game).await,
        0,
        "nothing is backed up as Zelda yet"
    );

    let devices: serde_json::Value = s
        .admin(reqwest::Method::GET, "/api/devices")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let device = devices[0]["id"].as_str().unwrap();
    let res = s
        .admin(
            reqwest::Method::PUT,
            &format!("/api/games/{}/prefix", common::urlencode(&game)),
        )
        .json(&serde_json::json!({ "device": device, "prefix": ryu_dir.display().to_string() }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success(), "{}", res.text().await.unwrap());
    pc.agent.refresh_settings().await;

    pc.agent.after_session(&game, 60).await.unwrap();
    assert_eq!(
        common::server_contents(&s, &game).await,
        ["in Ryujinx"],
        "the chosen emulator's save, and only that"
    );
    let report = pc.agent.report().await.unwrap();
    let zelda = report.games.iter().find(|g| g.game == game).unwrap();
    assert_eq!(zelda.problem, None);
    assert_eq!(zelda.source.as_deref(), Some("Ryujinx"));
}

/// While an emulator runs, no Switch save is written, whichever title it plays; once it is
/// closed, the new state is loaded.
#[tokio::test]
async fn nothing_is_written_while_an_emulator_runs() {
    use spielstand_agent::daemon::{Timings, run_with};
    use std::time::Duration;

    let s = server().await;
    let mut there = PathBuf::new();
    let desktop = switch_pc(&s, "desktop", |home| there = eden(home, [7; 16])).await;
    let mut here = PathBuf::new();
    let laptop = switch_pc(&s, "laptop", |home| here = eden(home, [9; 16])).await;
    let game = key(ZELDA);
    write(&here, "progress.sav", "laptop run");
    laptop.agent.after_session(&game, 60).await.unwrap();
    write(&there, "progress.sav", "desktop run");
    let head = s.game(&game).await["head"].as_str().unwrap().to_string();
    desktop.agent.restore_version(&game, &head).await.unwrap();

    // A process the system shows as `eden`.
    let bin = desktop.home.join("eden");
    std::fs::copy("/bin/sleep", &bin).unwrap();
    let mut emulator = std::process::Command::new(&bin).arg("30").spawn().unwrap();
    let timings = Timings {
        catch_up: Duration::from_secs(3600),
        full_scan: Duration::from_secs(3600),
        process_poll: Duration::from_millis(100),
        quiet: Duration::from_millis(300),
        rewatch: Duration::from_millis(500),
        reconnect_min: Duration::from_millis(100),
        reconnect_max: Duration::from_millis(200),
    };
    let agent = desktop.agent.clone();
    let daemon = tokio::spawn(async move {
        let _ = run_with(agent, timings).await;
    });
    tokio::time::sleep(Duration::from_millis(800)).await;

    write(&here, "progress.sav", "laptop: newer");
    laptop.agent.after_session(&game, 60).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        read(&there, "progress.sav").as_deref(),
        Some("laptop run"),
        "Eden is open"
    );

    emulator.kill().unwrap();
    emulator.wait().unwrap();
    for _ in 0..100 {
        if read(&there, "progress.sav").as_deref() == Some("laptop: newer") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        read(&there, "progress.sav").as_deref(),
        Some("laptop: newer"),
        "loaded after it closed"
    );
    daemon.abort();
}
