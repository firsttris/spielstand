//! Custom games and what a device may decide for itself, against a real server: a device's
//! proposal is in effect only there until the web UI takes it over.

mod common;

use common::{TestAgent, TestServer, start};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use spielstand_protocol::{AgentSettings, CustomGameRow, SaveMode, paths};

const INDIE: &str = "Indie Game";

fn indie(path: &str) -> Value {
    json!({ "name": INDIE, "files": [path] })
}

async fn settings(s: &TestServer, a: &TestAgent) -> AgentSettings {
    s.agent(a, Method::GET, paths::SETTINGS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn propose(s: &TestServer, a: &TestAgent, game: &Value) -> reqwest::Response {
    s.agent(a, Method::POST, paths::CUSTOM_GAMES)
        .json(game)
        .send()
        .await
        .unwrap()
}

async fn rows(s: &TestServer) -> Vec<CustomGameRow> {
    s.admin(Method::GET, "/api/custom-games")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_proposal_is_in_effect_only_on_its_device_until_it_is_taken_over() {
    let s = start().await;
    let desktop = s.pair("desktop").await;
    let deck = s.pair("deck").await;

    let res = propose(&s, &desktop, &indie(" <xdgData>/Indie/saves ")).await;
    assert!(res.status().is_success(), "{}", res.text().await.unwrap());
    let mine: AgentSettings = res.json().await.unwrap();
    assert_eq!(mine.custom_games.len(), 1);
    assert_eq!(mine.custom_games[0].files, ["<xdgData>/Indie/saves"], "trimmed");
    assert_eq!(mine.proposed_games, [INDIE]);
    assert!(
        settings(&s, &deck).await.custom_games.is_empty(),
        "not on the deck yet"
    );

    let listed = rows(&s).await;
    assert_eq!(listed[0].proposed_by_name.as_deref(), Some("desktop"));

    // Neither the deck nor the desktop can change what the other one may change.
    let res = propose(&s, &deck, &indie("<xdgData>/Elsewhere")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = s
        .agent(&deck, Method::DELETE, &paths::custom_game(INDIE))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let res = s
        .admin(Method::POST, "/api/custom-games/Indie%20Game/take-over")
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let on_deck = settings(&s, &deck).await;
    assert_eq!(on_deck.custom_games.len(), 1, "taken over: on every device");
    assert!(on_deck.proposed_games.is_empty());
    assert!(settings(&s, &desktop).await.proposed_games.is_empty());

    // Once it is every device's, only the web UI changes or removes it.
    let res = propose(&s, &desktop, &indie("<xdgData>/Indie/other")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = s
        .admin(Method::DELETE, "/api/custom-games/Indie%20Game")
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert!(settings(&s, &deck).await.custom_games.is_empty());

    let activity: Value = s
        .admin(Method::GET, "/api/activity")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let changes = activity
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["kind"] == "customGameChanged")
        .count();
    assert_eq!(changes, 3, "proposed, taken over, removed");
}

#[tokio::test]
async fn a_device_withdraws_its_own_proposal_and_the_web_ui_adds_for_everyone() {
    let s = start().await;
    let desktop = s.pair("desktop").await;
    let deck = s.pair("deck").await;
    assert!(
        propose(&s, &desktop, &indie("<home>/Indie"))
            .await
            .status()
            .is_success()
    );
    let res = s
        .agent(&desktop, Method::DELETE, &paths::custom_game(INDIE))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert!(rows(&s).await.is_empty());

    let res = s
        .admin(Method::PUT, "/api/custom-games")
        .json(&json!({ "name": "Hades", "files": ["<home>/Games/Hades/saves"], "extend": true }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    for device in [&desktop, &deck] {
        let got = settings(&s, device).await;
        assert!(got.custom_games[0].extend);
        assert!(got.proposed_games.is_empty());
    }
}

#[tokio::test]
async fn paths_that_catch_too_much_are_refused_from_both_sides() {
    let s = start().await;
    let desktop = s.pair("desktop").await;
    for path in [
        "<home>/*",
        "/home/t/.ssh",
        "<home>/.ssh/id_ed25519",
        "<home>/a/../b",
    ] {
        let res = propose(&s, &desktop, &indie(path)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path}");
        let res = s
            .admin(Method::PUT, "/api/custom-games")
            .json(&indie(path))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    assert!(rows(&s).await.is_empty());
}

#[tokio::test]
async fn a_device_chooses_its_own_prefix_and_only_ever_switches_syncing_off() {
    let s = start().await;
    let desktop = s.pair("desktop").await;
    let deck = s.pair("deck").await;

    let res = s
        .agent(&desktop, Method::PUT, paths::PREFIX)
        .json(&json!({ "game": "Hades", "prefix": "/home/t/Games/Hades/pfx" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let mine: AgentSettings = res.json().await.unwrap();
    assert_eq!(mine.prefix_choices["Hades"], "/home/t/Games/Hades/pfx");
    assert!(
        settings(&s, &deck).await.prefix_choices.is_empty(),
        "only for itself"
    );
    let res = s
        .agent(&desktop, Method::PUT, paths::PREFIX)
        .json(&json!({ "game": "Hades", "prefix": "relative" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = s
        .agent(&desktop, Method::POST, paths::BACKUP_ONLY)
        .json(&json!({ "game": "Hades" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert_eq!(settings(&s, &deck).await.game_modes["Hades"], SaveMode::Backup);
}
