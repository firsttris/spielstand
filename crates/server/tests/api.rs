mod common;

use common::{start, start_with, version};
use futures::{SinkExt, StreamExt};
use reqwest::Method;
use spielstand_protocol::{HeadsResponse, PromoteOutcome, UploadStatus, paths};
use spielstand_server::{mirror, switch_titles};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[tokio::test]
async fn heads_move_only_by_compare_and_swap() {
    let s = start().await;
    let a = s.pair("zentrale").await;
    let b = s.pair("laptop").await;
    let work = s.dir.path();

    let (v1, blob1) = version(work, "Elden Ring", "one", None);
    assert_eq!(s.upload_ok(&a, &v1, &blob1).await.status, UploadStatus::Head);
    let (v2, blob2) = version(work, "Elden Ring", "two", Some(&v1.id));
    assert_eq!(s.upload_ok(&a, &v2, &blob2).await.status, UploadStatus::Head);

    // B played offline on top of v1: kept as a branch, never as head.
    let (b1, blob_b1) = version(work, "Elden Ring", "laptop", Some(&v1.id));
    let outcome = s.upload_ok(&b, &b1, &blob_b1).await;
    assert_eq!(outcome.status, UploadStatus::Branch);
    assert_eq!(outcome.head.as_deref(), Some(v2.id.as_str()));

    let heads: HeadsResponse = s
        .agent(&b, Method::GET, paths::HEADS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(heads.heads["Elden Ring"].id, v2.id);
    assert_eq!(heads.open_branches, vec![b1.id.clone()]);

    // A retried upload is recognised and changes nothing.
    assert_eq!(s.upload_ok(&a, &v2, &blob2).await.status, UploadStatus::Known);

    // "Keep mine" with a stale expectation fails; with the real head it wins.
    let stale: PromoteOutcome = s
        .agent(&b, Method::POST, &paths::version_promote(&b1.id))
        .json(&serde_json::json!({ "expectedHead": v1.id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!stale.promoted);
    let promoted: PromoteOutcome = s
        .agent(&b, Method::POST, &paths::version_promote(&b1.id))
        .json(&serde_json::json!({ "expectedHead": v2.id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(promoted.promoted);
    let heads: HeadsResponse = s
        .agent(&b, Method::GET, paths::HEADS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(heads.heads["Elden Ring"].id, b1.id);
    assert!(heads.open_branches.is_empty());

    // Nothing was lost: all three versions are in the timeline.
    let detail: serde_json::Value = s
        .admin(Method::GET, "/api/games/Elden%20Ring")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["versions"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn parallel_uploads_on_one_head_give_one_head_and_one_branch() {
    for _ in 0..10 {
        let s = start().await;
        let a = s.pair("a").await;
        let b = s.pair("b").await;
        let (v1, blob1) = version(s.dir.path(), "G", "base", None);
        s.upload_ok(&a, &v1, &blob1).await;
        let (va, ba) = version(s.dir.path(), "G", "from a", Some(&v1.id));
        let (vb, bb) = version(s.dir.path(), "G", "from b", Some(&v1.id));
        let (ra, rb) = tokio::join!(s.upload_ok(&a, &va, &ba), s.upload_ok(&b, &vb, &bb));
        let mut statuses = [ra.status, rb.status];
        statuses.sort_by_key(|x| format!("{x:?}"));
        assert_eq!(statuses, [UploadStatus::Branch, UploadStatus::Head]);
    }
}

#[tokio::test]
async fn damaged_uploads_are_refused() {
    let s = start().await;
    let a = s.pair("a").await;
    let (mut v1, blob1) = version(s.dir.path(), "G", "x", None);
    v1.blob_sha256 = "0".repeat(64);
    assert_eq!(s.upload(&a, &v1, &blob1).await.status(), 400);
    let (mut v2, blob2) = version(s.dir.path(), "G", "y", None);
    v2.content_hash = "0".repeat(64);
    assert_eq!(s.upload(&a, &v2, &blob2).await.status(), 400);
}

#[tokio::test]
async fn agents_without_valid_token_are_rejected() {
    let s = start().await;
    let res = s
        .http
        .get(format!("{}{}", s.url, paths::HEADS))
        .bearer_auth("nope")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = s.http.get(format!("{}/api/games", s.url)).send().await.unwrap();
    assert_eq!(res.status(), 401);
    // A pairing code works once.
    let code: serde_json::Value = s
        .admin(Method::POST, "/api/pair-codes")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pair = |code: String| {
        let url = format!("{}{}", s.url, paths::PAIR);
        let http = s.http.clone();
        async move {
            http.post(url)
                .json(&serde_json::json!({"code": code, "name": "x", "hostname": "x", "os": "linux", "agentVersion": "t"}))
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    let c = code["code"].as_str().unwrap().to_string();
    assert!(pair(c.clone()).await.is_success());
    assert_eq!(pair(c).await, 401);
}

#[tokio::test]
async fn presence_is_seen_by_other_devices() {
    let s = start().await;
    let a = s.pair("zentrale").await;
    let b = s.pair("laptop").await;
    let mut req = format!("{}{}", s.url.replace("http", "ws"), paths::WS)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {}", a.token).parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let hello = ws.next().await.unwrap().unwrap();
    assert!(
        hello.to_text().unwrap().contains("\"type\":\"settings\""),
        "settings come first"
    );
    ws.send(Message::Text(r#"{"type":"playing","game":"Hades II"}"#.into()))
        .await
        .unwrap();
    ws.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
    let pong = ws.next().await.unwrap().unwrap();
    assert!(pong.to_text().unwrap().contains("pong"));

    let heads: HeadsResponse = s
        .agent(&b, Method::GET, paths::HEADS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        heads.playing_elsewhere.get("Hades II").map(String::as_str),
        Some("zentrale")
    );
    let mine: HeadsResponse = s
        .agent(&a, Method::GET, paths::HEADS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(mine.playing_elsewhere.is_empty());
}

#[tokio::test]
async fn new_heads_are_pushed_to_other_devices() {
    let s = start().await;
    let a = s.pair("a").await;
    let b = s.pair("b").await;
    let mut req = format!("{}{}", s.url.replace("http", "ws"), paths::WS)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {}", b.token).parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let hello = ws.next().await.unwrap().unwrap();
    assert!(
        hello.to_text().unwrap().contains("\"type\":\"settings\""),
        "settings come first"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (v1, blob1) = version(s.dir.path(), "G", "x", None);
    s.upload_ok(&a, &v1, &blob1).await;
    let pushed = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(pushed.to_text().unwrap().contains(&v1.id));
}

#[tokio::test]
async fn a_lost_server_comes_back_from_its_mirror() {
    let mirror_dir = tempfile::tempdir().unwrap();
    let spec = mirror_dir.path().to_string_lossy().to_string();
    let s = start_with(|settings| settings.mirror = Some(spec.clone())).await;
    let a = s.pair("zentrale").await;
    let (v1, blob1) = version(s.dir.path(), "Stardew Valley", "farm", None);
    s.upload_ok(&a, &v1, &blob1).await;
    let (v2, blob2) = version(s.dir.path(), "Stardew Valley", "farm 2", Some(&v1.id));
    s.upload_ok(&a, &v2, &blob2).await;
    let remote = mirror::remote(&spec, "rclone");
    assert_eq!(mirror::sync_once(&s.state, remote.as_ref()).await.unwrap(), 2);

    let fresh = start().await;
    assert_eq!(mirror::restore(&fresh.state, remote.as_ref()).await.unwrap(), 2);
    let detail: serde_json::Value = fresh
        .admin(Method::GET, "/api/games/Stardew%20Valley")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["head"].as_str(), Some(v2.id.as_str()));
    assert_eq!(detail["versions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn install_script_points_at_this_server() {
    let s = start().await;
    let script = s
        .http
        .get(format!("{}/install.sh", s.url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(script.starts_with("#!/bin/sh"));
    assert!(script.contains(&format!("SERVER=\"{}\"", s.url)));
    assert!(!script.contains("__SPIELSTAND_"));
}

#[tokio::test]
async fn guessing_passwords_and_pairing_codes_is_slowed_down() {
    let s = start().await;
    let login = |password: &'static str| {
        let (http, url) = (s.http.clone(), format!("{}/api/login", s.url));
        async move {
            http.post(url)
                .json(&serde_json::json!({ "password": password }))
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    let guesses = futures::future::join_all((0..12).map(|_| login("wrong guess"))).await;
    assert!(guesses.iter().any(|s| *s == 429), "{guesses:?}");
    assert_eq!(login("correct horse").await, 429, "even the right one waits now");

    let pair = |code: String| {
        let (http, url) = (s.http.clone(), format!("{}{}", s.url, paths::PAIR));
        async move {
            http.post(url)
                .json(&serde_json::json!({"code": code, "name": "x", "hostname": "x", "os": "linux", "agentVersion": "t"}))
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    let tries = futures::future::join_all((0..25).map(|n| pair(format!("AAA-{n:03}")))).await;
    assert_eq!(tries.iter().filter(|s| **s == 429).count(), 5, "{tries:?}");
}

#[tokio::test]
async fn switch_games_without_a_name_get_one_from_the_title_list() {
    let list = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        list.path(),
        r#"{"70010000000029": {"id": "0100000000010000", "name": "Super Mario Odyssey™"}}"#,
    )
    .unwrap();
    let source = list.path().to_str().unwrap().to_string();
    let s = start_with(|settings| settings.switch_titles = source).await;
    let a = s.pair("zentrale").await;
    let (v, blob) = version(s.dir.path(), "Switch 0100000000010000", "one", None);
    s.upload_ok(&a, &v, &blob).await;
    let (v, blob) = version(s.dir.path(), "Switch 0100152000022000", "one", None);
    s.upload_ok(&a, &v, &blob).await;

    assert_eq!(switch_titles::refresh(&s.state).await.unwrap(), 1);

    let games: serde_json::Value = s
        .admin(Method::GET, "/api/games")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let title = |id: &str| games.as_array().unwrap().iter().find(|g| g["id"] == id).unwrap()["title"].clone();
    assert_eq!(title("Switch 0100000000010000"), "Super Mario Odyssey");
    assert!(title("Switch 0100152000022000").is_null());
    let detail: serde_json::Value = s
        .admin(Method::GET, "/api/games/Switch%200100000000010000")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["title"], "Super Mario Odyssey");
}
