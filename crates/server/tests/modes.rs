//! Modes and settings, known versions by ancestry, overwrite alerts, retention and the rclone
//! mirror, against a real server.

mod common;

use chrono::{DateTime, Duration, Utc};
use common::{TestAgent, TestServer, start, start_with, version};
use futures::StreamExt;
use reqwest::Method;
use serde_json::{Value, json};
use spielstand_protocol::{AgentSettings, KnownResponse, SaveMode, paths};
use spielstand_server::{gc, mirror, versions};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const GAME: &str = "Hades";

async fn known(s: &TestServer, a: &TestAgent, hash: &str, base: Option<&str>) -> KnownResponse {
    s.agent(a, Method::POST, paths::KNOWN)
        .json(&json!({ "game": GAME, "contentHash": hash, "base": base }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn get(s: &TestServer, path: &str) -> Value {
    s.admin(Method::GET, path)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn known_versions_are_found_by_content_and_judged_by_ancestry() {
    let s = start().await;
    let a = s.pair("desktop").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;
    let (v2, b2) = version(s.dir.path(), GAME, "two", Some(&v1.id));
    s.upload_ok(&a, &v2, &b2).await;

    let newer = known(&s, &a, &v2.content_hash, Some(&v1.id)).await;
    assert_eq!(newer.version.as_deref(), Some(v2.id.as_str()));
    assert!(newer.descends_from_base);
    let older = known(&s, &a, &v1.content_hash, Some(&v2.id)).await;
    assert_eq!(older.version.as_deref(), Some(v1.id.as_str()));
    assert!(!older.descends_from_base, "v1 is older than v2: an overwrite");
    let fresh = known(&s, &a, &v2.content_hash, None).await;
    assert!(fresh.descends_from_base, "a device without a base loses nothing");
    assert_eq!(known(&s, &a, "unknown", None).await.version, None);

    // An imported version with the same content is never offered for adoption.
    let (mut imported, ib) = version(s.dir.path(), GAME, "three", None);
    imported.trigger = spielstand_protocol::Trigger::Import;
    imported.device = "import".into();
    let sha = spielstand_core::pack::sha256_file(&ib).unwrap();
    spielstand_server::blobs::store_file(&s.state.settings.blobs_dir(), &ib, &sha)
        .await
        .unwrap();
    sqlx::query("INSERT OR IGNORE INTO devices (id, name, created_at) VALUES ('import', 'Ludusavi', '2020-01-01T00:00:00Z')")
        .execute(&s.state.db)
        .await
        .unwrap();
    let hash = imported.content_hash.clone();
    versions::insert_external(
        &s.state,
        versions::NewVersion {
            meta: imported,
            pinned: false,
            label: None,
        },
        false,
    )
    .await
    .unwrap();
    assert_eq!(known(&s, &a, &hash, None).await.version, None);
}

#[tokio::test]
async fn an_overwrite_alert_shows_until_the_lost_state_is_back_or_dismissed() {
    let s = start().await;
    let a = s.pair("desktop").await;
    let deck = s.pair("deck").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;
    let (v2, b2) = version(s.dir.path(), GAME, "two", Some(&v1.id));
    s.upload_ok(&deck, &v2, &b2).await;

    let report = json!({ "game": GAME, "lost": v2.id, "found": v1.id });
    for _ in 0..2 {
        let res = s
            .agent(&deck, Method::POST, paths::OVERWRITES)
            .json(&report)
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
    }
    let detail = get(&s, "/api/games/Hades").await;
    let alerts = detail["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 1, "reported twice, shown once");
    assert_eq!(alerts[0]["deviceName"], "deck");
    assert_eq!(get(&s, "/api/overview").await["alerts"], 1);
    let games = get(&s, "/api/games").await;
    assert_eq!(games[0]["overwritten"], true);

    // Putting the lost state back on the deck settles it.
    let res = s
        .agent(&deck, Method::POST, paths::BASES)
        .json(&json!({ "game": GAME, "version": v2.id }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert!(
        get(&s, "/api/games/Hades").await["alerts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Dismissing works once; versions of another game are refused.
    s.agent(&deck, Method::POST, paths::OVERWRITES)
        .json(&report)
        .send()
        .await
        .unwrap();
    let id = get(&s, "/api/games/Hades").await["alerts"][0]["id"]
        .as_i64()
        .unwrap();
    let dismiss = |id: i64| s.admin(Method::POST, &format!("/api/alerts/{id}/dismiss")).send();
    assert!(dismiss(id).await.unwrap().status().is_success());
    assert_eq!(dismiss(id).await.unwrap().status(), 404);
    let (other, ob) = version(s.dir.path(), "Other", "x", None);
    s.upload_ok(&a, &other, &ob).await;
    let wrong = s
        .agent(&deck, Method::POST, paths::OVERWRITES)
        .json(&json!({ "game": GAME, "lost": v2.id, "found": other.id }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 400);
}

/// The deck lost v2, then caught up past it to v3 (the desktop played on): it holds a newer
/// state than the lost one, so the alert goes, and its button can no longer roll every PC back.
#[tokio::test]
async fn an_overwrite_alert_settles_once_the_device_holds_a_newer_state() {
    let s = start().await;
    let a = s.pair("desktop").await;
    let deck = s.pair("deck").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;
    let (v2, b2) = version(s.dir.path(), GAME, "two", Some(&v1.id));
    s.upload_ok(&a, &v2, &b2).await;
    let (v3, b3) = version(s.dir.path(), GAME, "three", Some(&v2.id));
    s.upload_ok(&a, &v3, &b3).await;
    s.agent(&deck, Method::POST, paths::OVERWRITES)
        .json(&json!({ "game": GAME, "lost": v2.id, "found": v1.id }))
        .send()
        .await
        .unwrap();
    let base = |version: String| {
        s.agent(&deck, Method::POST, paths::BASES)
            .json(&json!({ "game": GAME, "version": version }))
            .send()
    };
    let open = || async {
        get(&s, "/api/games/Hades").await["alerts"]
            .as_array()
            .unwrap()
            .len()
    };

    assert!(base(v1.id.clone()).await.unwrap().status().is_success());
    assert_eq!(open().await, 1, "still the older state");
    assert!(base(v3.id.clone()).await.unwrap().status().is_success());
    assert_eq!(open().await, 0, "v3 is built on the lost v2");
}

#[tokio::test]
async fn modes_and_folders_are_set_in_the_web_ui_and_pushed_to_agents() {
    let s = start().await;
    let a = s.pair("desktop").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;

    let mut req = format!("{}{}", s.url.replace("http", "ws"), paths::WS)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {}", a.token).parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let first = ws.next().await.unwrap().unwrap();
    assert!(
        first.to_text().unwrap().contains("\"type\":\"settings\""),
        "settings on connect"
    );

    assert_eq!(get(&s, "/api/settings").await["defaultMode"], "sync");
    s.admin(Method::PUT, "/api/settings")
        .json(&json!({ "defaultMode": "backup" }))
        .send()
        .await
        .unwrap();
    let pushed = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(pushed.to_text().unwrap().contains("\"defaultMode\":\"backup\""));

    s.admin(Method::PUT, "/api/games/Hades/mode")
        .json(&json!({ "mode": "sync" }))
        .send()
        .await
        .unwrap();
    let settings: AgentSettings = s
        .agent(&a, Method::GET, paths::SETTINGS)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(settings.default_mode, SaveMode::Backup);
    assert_eq!(settings.game_modes.get(GAME), Some(&SaveMode::Sync));
    let detail = get(&s, "/api/games/Hades").await;
    assert_eq!(detail["mode"], "sync");
    assert_eq!(detail["modeIsDefault"], false);

    s.admin(Method::PUT, "/api/games/Hades/mode")
        .json(&json!({ "mode": null }))
        .send()
        .await
        .unwrap();
    let detail = get(&s, "/api/games/Hades").await;
    assert_eq!(detail["mode"], "backup");
    assert_eq!(detail["modeIsDefault"], true);

    let devices = get(&s, "/api/devices").await;
    let id = devices[0]["id"].as_str().unwrap();
    let saved: Value = s
        .admin(Method::PUT, &format!("/api/devices/{id}/settings"))
        .json(&json!({ "gameFolders": [" ~/Games ", "", "/run/media/deck/SD/Games"] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        saved["gameFolders"],
        json!(["~/Games", "/run/media/deck/SD/Games"])
    );
    assert_eq!(
        get(&s, "/api/devices").await[0]["gameFolders"],
        saved["gameFolders"]
    );
    let unknown = s
        .admin(Method::PUT, "/api/devices/nope/settings")
        .json(&json!({ "gameFolders": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);
    let without_login = s
        .http
        .put(format!("{}/api/settings", s.url))
        .header("x-spielstand-csrf", "1")
        .json(&json!({ "defaultMode": "sync" }))
        .send()
        .await
        .unwrap();
    assert_eq!(without_login.status(), 401);
}

async fn backdate(s: &TestServer, id: &str, at: DateTime<Utc>) {
    sqlx::query("UPDATE versions SET created_at = ? WHERE id = ?")
        .bind(spielstand_server::db::ts(at))
        .bind(id)
        .execute(&s.state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn retention_thins_old_versions_keeps_protected_ones_and_purges_the_trash() {
    let mirror_dir = tempfile::tempdir().unwrap();
    let spec = mirror_dir.path().to_string_lossy().to_string();
    let s = start_with(|settings| settings.mirror = Some(spec.clone())).await;
    let a = s.pair("desktop").await;
    // Three versions on one old day, a pinned one, and the head.
    let mut parent: Option<String> = None;
    let mut ids = vec![];
    for n in 0..5 {
        let (v, b) = version(s.dir.path(), GAME, &format!("state {n}"), parent.as_deref());
        s.upload_ok(&a, &v, &b).await;
        parent = Some(v.id.clone());
        ids.push(v.id);
    }
    // A second apart: timestamps keep only milliseconds, and a tie would let the pinned one
    // count as the newest of the old day.
    let old = Utc::now() - Duration::days(40);
    for (n, id) in ids[..4].iter().enumerate() {
        backdate(&s, id, old + Duration::seconds(n as i64)).await;
    }
    s.admin(Method::PATCH, &format!("/api/versions/{}", ids[0]))
        .json(&json!({ "pinned": true }))
        .send()
        .await
        .unwrap();
    let remote = mirror::remote(&spec, "rclone");
    mirror::sync_once(&s.state, remote.as_ref()).await.unwrap();

    let trashed = gc::trash_expired(&s.state, Default::default()).await.unwrap();
    // ids[1..4] share a day older than a week: one of them stays; ids[0] is pinned; ids[4] is
    // the head and the device's base.
    assert_eq!(trashed, 2);
    let alive: Vec<(String,)> = sqlx::query_as("SELECT id FROM versions WHERE deleted_at IS NULL")
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    let alive: Vec<String> = alive.into_iter().map(|(id,)| id).collect();
    assert!(alive.contains(&ids[0]) && alive.contains(&ids[4]));

    assert_eq!(
        gc::purge(&s.state).await.unwrap(),
        0,
        "the trash keeps them for a while"
    );
    sqlx::query("UPDATE versions SET deleted_at = ? WHERE deleted_at IS NOT NULL")
        .bind(spielstand_server::db::ts(
            Utc::now() - Duration::days(gc::TRASH_DAYS + 1),
        ))
        .execute(&s.state.db)
        .await
        .unwrap();
    assert_eq!(gc::purge(&s.state).await.unwrap(), 2);
    let mirrored = remote.list().await.unwrap();
    let blobs = mirrored.iter().filter(|f| f.ends_with(".tar.zst")).count();
    assert_eq!(blobs, 3, "purged versions leave the mirror too: {mirrored:?}");
}

#[tokio::test]
async fn the_mirror_works_through_rclone() {
    if std::process::Command::new("rclone")
        .arg("version")
        .output()
        .is_err()
    {
        eprintln!("rclone is not installed; skipped (CI installs it)");
        return;
    }
    let target = tempfile::tempdir().unwrap();
    // rclone's on-the-fly local backend: the same code path as `gdrive:Spielstand`.
    let spec = format!(":local:{}", target.path().display());
    let s = start_with(|settings| settings.mirror = Some(spec.clone())).await;
    let a = s.pair("desktop").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;
    let remote = mirror::remote(&spec, "rclone");
    assert_eq!(mirror::sync_once(&s.state, remote.as_ref()).await.unwrap(), 1);
    assert!(
        target
            .path()
            .join("hades")
            .join(format!("{}.tar.zst", v1.id))
            .is_file()
    );

    let fresh = start().await;
    assert_eq!(mirror::restore(&fresh.state, remote.as_ref()).await.unwrap(), 1);
    let detail: Value = fresh
        .admin(Method::GET, "/api/games/Hades")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["head"], v1.id);
}

#[tokio::test]
async fn loading_a_version_continues_from_it_in_both_modes() {
    let s = start().await;
    let a = s.pair("desktop").await;
    let (v1, b1) = version(s.dir.path(), GAME, "one", None);
    s.upload_ok(&a, &v1, &b1).await;
    let (v2, b2) = version(s.dir.path(), GAME, "two", Some(&v1.id));
    s.upload_ok(&a, &v2, &b2).await;
    let device = get(&s, "/api/devices").await[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let restore = |version: String| {
        s.admin(Method::POST, "/api/games/Hades/restore")
            .json(&json!({ "device": device, "version": version }))
            .send()
    };
    assert_eq!(
        restore(v1.id.clone()).await.unwrap().status(),
        409,
        "the device is offline"
    );

    let mut req = format!("{}{}", s.url.replace("http", "ws"), paths::WS)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {}", a.token).parse().unwrap());
    let (_ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert!(restore(v1.id.clone()).await.unwrap().status().is_success());
    assert_eq!(
        get(&s, "/api/games/Hades").await["head"],
        v1.id,
        "sync: continue from v1"
    );

    s.admin(Method::PUT, "/api/games/Hades/mode")
        .json(&json!({ "mode": "backup" }))
        .send()
        .await
        .unwrap();
    assert!(restore(v2.id.clone()).await.unwrap().status().is_success());
    assert_eq!(
        get(&s, "/api/games/Hades").await["head"],
        v2.id,
        "backup: the device's next upload builds on v2, so v2 is the current state"
    );
}
