//! The background service: live connection to the server, outbox, running games, catch-up.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use futures::{SinkExt, StreamExt};
use spielstand_protocol::{AgentMessage, ServerMessage, Trigger, paths};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use crate::{
    running::{self, Watcher},
    sync::{Agent, Mode, Outcome},
};

const CATCH_UP: Duration = Duration::from_secs(30 * 60);
const FULL_SCAN: Duration = Duration::from_secs(6 * 3600);

fn busy(agent: &Agent, game: &str, playing: &BTreeMap<String, (u32, Instant)>) -> bool {
    playing.contains_key(game) || running::wrapped(&agent.dirs).iter().any(|g| g == game)
}

async fn on_message(agent: &Agent, message: ServerMessage, playing: &BTreeMap<String, (u32, Instant)>) {
    let result = match message {
        ServerMessage::NewHead { game, .. } if !busy(agent, &game, playing) => {
            agent.sync_game(&game, Mode::Background).await.map(|_| ())
        }
        ServerMessage::Restore { game, version } => {
            if busy(agent, &game, playing) {
                agent
                    .ui
                    .info(
                        &format!("{game} läuft gerade"),
                        "Der Stand wurde nicht geladen. Beende das Spiel und versuche es erneut.",
                    )
                    .await;
                Ok(())
            } else {
                agent.restore_version(&game, &version).await.map(|_| ())
            }
        }
        ServerMessage::BackUp { game } => back_up_now(agent, &game).await,
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!("server message: {e:#}");
    }
}

async fn back_up_now(agent: &Agent, game: &str) -> anyhow::Result<()> {
    let scanner = agent.scanner.clone();
    crate::disk::snapshot(
        &agent.dirs,
        &agent.store,
        scanner.as_ref(),
        &agent.config.device_id,
        game,
        Trigger::Manual,
        None,
    )
    .await?;
    agent.flush().await?;
    Ok(())
}

fn ws_url(server: &str) -> String {
    let base = server
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{base}{}", paths::WS)
}

/// One live connection; returns when it drops.
async fn connect(
    agent: &Agent,
    playing: &tokio::sync::Mutex<BTreeMap<String, (u32, Instant)>>,
) -> anyhow::Result<()> {
    let mut req = ws_url(agent.client.server()).into_client_request()?;
    req.headers_mut().insert(
        "authorization",
        format!("Bearer {}", agent.client.token()).parse()?,
    );
    let (ws, _) = tokio_tungstenite::connect_async(req).await?;
    let (mut tx, mut rx) = ws.split();
    let hello = AgentMessage::Hello {
        agent_version: env!("CARGO_PKG_VERSION").into(),
    };
    tx.send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;
    tracing::info!("connected to {}", agent.client.server());
    catch_up(agent, &playing.lock().await.clone()).await;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = ping.tick() => tx.send(Message::Text(serde_json::to_string(&AgentMessage::Ping)?.into())).await?,
            incoming = rx.next() => {
                let Some(message) = incoming else { return Ok(()) };
                if let Message::Text(text) = message?
                    && let Ok(parsed) = serde_json::from_str::<ServerMessage>(&text) {
                        let snapshot = playing.lock().await.clone();
                        on_message(agent, parsed, &snapshot).await;
                    }
            }
        }
    }
}

/// Syncs every known game that is not running; uploads changes, fetches new heads.
async fn catch_up(agent: &Agent, playing: &BTreeMap<String, (u32, Instant)>) {
    let Ok(games) = agent.store.tracked_games().await else {
        return;
    };
    for game in games.iter().filter(|g| !busy(agent, g, playing)) {
        match agent.sync_game(game, Mode::Background).await {
            Ok(Outcome::Offline) => return,
            Ok(_) => {}
            Err(e) => tracing::warn!("{game}: {e:#}"),
        }
    }
}

/// First start and every few hours: find games with saves this agent does not know yet.
async fn discover(agent: &Agent) {
    let scanner = agent.scanner.clone();
    let Ok(found) = tokio::task::spawn_blocking(move || scanner.games_with_saves()).await else {
        return;
    };
    let known = agent.store.tracked_games().await.unwrap_or_default();
    for game in found.iter().filter(|g| !known.contains(g)) {
        if let Err(e) = agent.sync_game(game, Mode::Background).await {
            tracing::warn!("{game}: {e:#}");
        }
    }
}

async fn watch_processes(agent: &Agent, playing: &tokio::sync::Mutex<BTreeMap<String, (u32, Instant)>>) {
    let mut watcher = Watcher::default();
    let mut installs = vec![];
    let mut installs_at = Instant::now() - Duration::from_secs(3600);
    loop {
        if installs_at.elapsed() > Duration::from_secs(600) {
            let scanner = agent.scanner.clone();
            installs = tokio::task::spawn_blocking(move || scanner.install_dirs())
                .await
                .unwrap_or_default();
            installs_at = Instant::now();
        }
        let wrapped = running::wrapped(&agent.dirs);
        let now_running: BTreeMap<String, u32> = watcher
            .running(&installs)
            .into_iter()
            .filter(|(g, _)| !wrapped.contains(g))
            .collect();
        let mut state = playing.lock().await;
        for (game, pid) in &now_running {
            if !state.contains_key(game) {
                tracing::info!("{game} started");
                let _ = agent.client.presence(Some(game)).await;
                state.insert(game.clone(), (*pid, Instant::now()));
            }
        }
        let ended: Vec<(String, Instant)> = state
            .iter()
            .filter(|(g, _)| !now_running.contains_key(*g))
            .map(|(g, (_, t))| (g.clone(), *t))
            .collect();
        for (game, started) in ended {
            state.remove(&game);
            tracing::info!("{game} ended");
            if let Err(e) = agent.after_session(&game, started.elapsed().as_secs()).await {
                tracing::warn!("{game}: after session: {e:#}");
            }
        }
        drop(state);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

pub async fn run(agent: Agent) -> anyhow::Result<()> {
    let playing = std::sync::Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));
    discover(&agent).await;

    let (a, p) = (agent.clone(), playing.clone());
    tokio::spawn(async move { watch_processes(&a, &p).await });

    let (a, p) = (agent.clone(), playing.clone());
    tokio::spawn(async move {
        let mut last_full = Instant::now();
        loop {
            tokio::time::sleep(CATCH_UP).await;
            let _ = a.flush().await;
            catch_up(&a, &p.lock().await.clone()).await;
            if last_full.elapsed() > FULL_SCAN {
                discover(&a).await;
                last_full = Instant::now();
            }
        }
    });

    let mut backoff = Duration::from_secs(5);
    loop {
        match connect(&agent, &playing).await {
            Ok(()) => backoff = Duration::from_secs(5),
            Err(e) => tracing::warn!("live connection: {e:#}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
        let _ = agent.flush().await;
    }
}
