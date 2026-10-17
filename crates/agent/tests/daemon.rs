//! The background service with short timings against a real server: the file watcher, pushes
//! from the server, settings, reports and reconnecting after the server was gone.

mod common;

use std::{sync::Arc, time::Duration};

use common::{GAME, Pc, pc, server, server_contents};
use spielstand_agent::{
    client::Client,
    daemon::{Timings, run_with},
    sync::Outcome,
};
use spielstand_protocol::SaveMode;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

fn fast() -> Timings {
    Timings {
        catch_up: Duration::from_secs(3600),
        full_scan: Duration::from_secs(3600),
        process_poll: Duration::from_millis(200),
        quiet: Duration::from_millis(300),
        rewatch: Duration::from_millis(500),
        reconnect_min: Duration::from_millis(100),
        reconnect_max: Duration::from_millis(200),
    }
}

fn start(pc: &Pc) -> JoinHandle<()> {
    let agent = pc.agent.clone();
    tokio::spawn(async move {
        let _ = run_with(agent, fast()).await;
    })
}

/// Waits up to five seconds for `check` to hold.
async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test]
async fn a_changed_save_is_backed_up_without_a_session_end() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    desktop.write_save("first");
    let daemon = start(&desktop);
    eventually("the first state", || async {
        server_contents(&s, GAME).await.contains(&"first".into())
    })
    .await;
    // The watcher may only start after the first scan; give it a moment, then change the save.
    tokio::time::sleep(Duration::from_millis(700)).await;
    desktop.write_save("second, while playing");
    eventually("the changed state", || async {
        server_contents(&s, GAME)
            .await
            .contains(&"second, while playing".into())
    })
    .await;
    daemon.abort();
}

#[tokio::test]
async fn a_new_head_is_pushed_and_loaded_on_the_other_pc() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let deck = pc(&s, "deck", false).await;
    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    let daemon = start(&deck);
    eventually("the deck loads v1 on start", || async {
        deck.read_save().as_deref() == Some("v1")
    })
    .await;
    desktop.write_save("v2");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    eventually("the deck loads the pushed v2", || async {
        deck.read_save().as_deref() == Some("v2")
    })
    .await;
    daemon.abort();
}

#[tokio::test]
async fn settings_from_the_web_ui_reach_the_running_agent_and_it_reports_back() {
    let s = server().await;
    let deck = pc(&s, "deck", false).await;
    deck.write_save("x");
    let daemon = start(&deck);
    eventually("the first report", || async {
        let (report,): (Option<String>,) = sqlx::query_as("SELECT report FROM devices")
            .fetch_one(&s.state.db)
            .await
            .unwrap();
        report.is_some_and(|r| r.contains(GAME))
    })
    .await;
    let res = s
        .admin(reqwest::Method::PUT, "/api/settings")
        .json(&serde_json::json!({ "defaultMode": "backup" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    eventually("the pushed default mode", || async {
        deck.agent.configured_mode(GAME) == SaveMode::Backup
    })
    .await;
    eventually("a report in backup mode", || async {
        let (report,): (Option<String>,) = sqlx::query_as("SELECT report FROM devices")
            .fetch_one(&s.state.db)
            .await
            .unwrap();
        report.is_some_and(|r| r.contains("\"effectiveMode\":\"backup\""))
    })
    .await;
    daemon.abort();
}

/// A TCP forwarder that can be cut and opened again on the same port.
struct Proxy {
    port: u16,
    target: String,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Proxy {
    async fn open(port: u16, target: &str) -> Self {
        let proxy = Self {
            port,
            target: target.trim_start_matches("http://").to_string(),
            tasks: Arc::default(),
        };
        proxy.start().await;
        proxy
    }

    async fn start(&self) {
        let listener = TcpListener::bind(("127.0.0.1", self.port)).await.unwrap();
        let (target, tasks) = (self.target.clone(), self.tasks.clone());
        let accept = tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                let target = target.clone();
                let conn = tokio::spawn(async move {
                    if let Ok(mut outbound) = tokio::net::TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
                tasks.lock().await.push(conn);
            }
        });
        self.tasks.lock().await.push(accept);
    }

    async fn cut(&self) {
        for t in self.tasks.lock().await.drain(..) {
            t.abort();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn after_the_server_was_gone_the_agent_reconnects_and_catches_up() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    let mut deck = pc(&s, "deck", false).await;
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let proxy = Proxy::open(port, &s.url).await;
    deck.agent.client = Client::new(&format!("http://127.0.0.1:{port}"), &deck.agent.config.token);

    desktop.write_save("v1");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    let daemon = start(&deck);
    eventually("v1 on the deck", || async {
        deck.read_save().as_deref() == Some("v1")
    })
    .await;

    proxy.cut().await;
    desktop.write_save("v2 while the deck is cut off");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        deck.read_save().as_deref(),
        Some("v1"),
        "nothing can reach the deck"
    );

    proxy.start().await;
    eventually("the deck catches up after reconnecting", || async {
        deck.read_save().as_deref() == Some("v2 while the deck is cut off")
    })
    .await;
    daemon.abort();
}

/// Back up only: loading an older version from the web UI and playing on must not turn every
/// later session into a conflict ("played on two PCs" with one PC).
#[tokio::test]
async fn playing_on_after_loading_a_version_in_backup_mode_is_no_conflict() {
    let s = server().await;
    let desktop = pc(&s, "desktop", false).await;
    s.set_mode(GAME, Some("backup"), &[&desktop]).await;
    desktop.write_save("one");
    desktop.agent.after_session(GAME, 60).await.unwrap();
    let one = s.game(GAME).await["head"].as_str().unwrap().to_string();
    desktop.write_save("two");
    desktop.agent.after_session(GAME, 60).await.unwrap();

    let daemon = start(&desktop);
    let devices: serde_json::Value = s
        .admin(reqwest::Method::GET, "/api/devices")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let device = devices[0]["id"].as_str().unwrap().to_string();
    let restore = || {
        s.admin(
            reqwest::Method::POST,
            &format!("/api/games/{}/restore", common::urlencode(GAME)),
        )
        .json(&serde_json::json!({ "device": device, "version": one }))
        .send()
    };
    eventually("the web UI loads v1", || async {
        restore().await.unwrap().status().is_success()
    })
    .await;
    // Done once the desktop reported v1 as its state, the load's last step.
    eventually("the desktop holds v1", || async {
        desktop.read_save().as_deref() == Some("one")
            && s.game(GAME).await["versions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["meta"]["id"] == one.as_str() && v["onDevices"][0] == "desktop")
    })
    .await;
    daemon.abort();

    for session in ["three", "four"] {
        desktop.write_save(session);
        let outcome = desktop.agent.after_session(GAME, 60).await.unwrap();
        assert_eq!(outcome, Outcome::Uploaded, "{session}");
    }
    assert_eq!(s.game(GAME).await["conflict"], false);
}
