//! The whole chain for the end-to-end tests: a real server and two real agent processes
//! ("Desktop" and "Steam Deck"), each with its own home folder, Prefixr library and Wine prefix
//! for Hades, and an offline Ludusavi manifest. Nothing is faked between the browser and the
//! save files on disk.
//!
//! `cargo build -p spielstand-agent && cargo run -p spielstand-server --example stack`
//!
//! - `SPIELSTAND_LISTEN` (default 127.0.0.1:8469): the server, admin password `spielstand-stack`.
//! - `SPIELSTAND_STACK_CONTROL` (default 127.0.0.1:8470): `GET /pcs` lists each PC's save file;
//!   `POST /pcs/{name}/stop` and `/start` switch a PC's agent off and on.
//! - `SPIELSTAND_AGENT_BIN`: the agent binary (default: next to this example in `target/`).

mod pcs;

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use pcs::Pc;
use spielstand_server::Settings;
use tokio::sync::Mutex;

pub const PASSWORD: &str = "spielstand-stack";

type Pcs = Arc<Mutex<BTreeMap<String, Pc>>>;

async fn list(State(pcs): State<Pcs>) -> Json<serde_json::Value> {
    let pcs = pcs.lock().await;
    Json(serde_json::json!(
        pcs.iter()
            .map(|(name, pc)| (
                name.clone(),
                serde_json::json!({ "save": pc.save, "running": pc.running() })
            ))
            .collect::<serde_json::Map<_, _>>()
    ))
}

async fn stop(State(pcs): State<Pcs>, Path(name): Path<String>) -> &'static str {
    if let Some(pc) = pcs.lock().await.get_mut(&name) {
        pc.stop().await;
    }
    "stopped"
}

async fn start(State(pcs): State<Pcs>, Path(name): Path<String>) -> &'static str {
    if let Some(pc) = pcs.lock().await.get_mut(&name) {
        pc.start();
    }
    "started"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let listen = std::env::var("SPIELSTAND_LISTEN").unwrap_or_else(|_| "127.0.0.1:8469".into());
    let control = std::env::var("SPIELSTAND_STACK_CONTROL").unwrap_or_else(|_| "127.0.0.1:8470".into());
    let url = format!("http://{listen}");
    let mut settings = Settings::for_tests(tmp.path().join("server"));
    settings.listen = listen.clone();
    settings.public_url = Some(url.clone());
    tokio::spawn(spielstand_server::serve(settings));

    let http = reqwest::Client::new();
    while http.get(format!("{url}/healthz")).send().await.is_err() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let res = http
        .post(format!("{url}/api/setup"))
        .header("x-spielstand-csrf", "1")
        .json(&serde_json::json!({ "password": PASSWORD }))
        .send()
        .await?
        .error_for_status()?;
    let cookie = res.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap_or_default()
        .to_string();

    let manifest = tmp.path().join("manifest.yaml");
    std::fs::write(&manifest, pcs::MANIFEST)?;
    let mut all = BTreeMap::new();
    for (name, prefix_at, first_save) in [
        ("Desktop", "Games/Hades/prefix", Some("desktop: first run")),
        ("Steam Deck", "sd/Games/Hades/prefix", None),
    ] {
        let code: serde_json::Value = http
            .post(format!("{url}/api/pair-codes"))
            .header("cookie", &cookie)
            .header("x-spielstand-csrf", "1")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut pc = Pc::set_up(
            &tmp.path().join("pcs").join(name.replace(' ', "-")),
            name,
            prefix_at,
            first_save,
            &manifest,
        )?;
        pc.pair(&url, code["code"].as_str().unwrap_or_default()).await?;
        pc.start();
        all.insert(name.to_string(), pc);
    }
    let pcs: Pcs = Arc::new(Mutex::new(all));
    let router = Router::new()
        .route("/pcs", get(list))
        .route("/pcs/{name}/stop", post(stop))
        .route("/pcs/{name}/start", post(start))
        .with_state(pcs.clone());
    let listener = tokio::net::TcpListener::bind(&control).await?;
    tokio::spawn(axum::serve(listener, router).into_future());
    println!("stack ready on {url} (password: {PASSWORD}), control on http://{control}");
    stopped().await;
    for pc in pcs.lock().await.values_mut() {
        pc.stop().await;
    }
    Ok(())
}

/// Ctrl-C, or SIGTERM from whatever started this (Playwright, CI).
async fn stopped() {
    let mut term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
