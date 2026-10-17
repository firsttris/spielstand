//! The background service: live connection to the server, outbox, running games, watched save
//! folders, catch-up and the reports for the web UI.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{SinkExt, StreamExt};
use spielstand_protocol::{AgentMessage, ServerMessage, Trigger, paths};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use crate::{
    agent::Agent,
    disk, emulators,
    running::{self, Watcher},
    sync::{Mode, Outcome},
    watch::{self, Debounce},
};

/// How often the service does what; shorter in tests.
#[derive(Clone, Debug)]
pub struct Timings {
    pub catch_up: Duration,
    pub full_scan: Duration,
    pub process_poll: Duration,
    /// How long a game's save files must be quiet before the new state is recorded.
    pub quiet: Duration,
    /// How often the watched folders are worked out again (games installed, saves moved).
    pub rewatch: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
}

impl Timings {
    /// The defaults; `SPIELSTAND_QUIET_SECONDS` sets how long save files must be quiet before
    /// a change is backed up.
    pub fn from_env() -> Self {
        let mut timings = Self::default();
        if let Some(seconds) = std::env::var("SPIELSTAND_QUIET_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
        {
            timings.quiet = Duration::from_secs_f64(seconds);
        }
        timings
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            catch_up: Duration::from_secs(30 * 60),
            full_scan: Duration::from_secs(6 * 3600),
            process_poll: Duration::from_secs(5),
            quiet: Duration::from_secs(10),
            rewatch: Duration::from_secs(10 * 60),
            reconnect_min: Duration::from_secs(5),
            reconnect_max: Duration::from_secs(60),
        }
    }
}

type Playing = Arc<Mutex<BTreeMap<String, (u32, Instant)>>>;

fn busy(agent: &Agent, game: &str, playing: &BTreeMap<String, (u32, Instant)>) -> bool {
    playing.contains_key(game) || running::wrapped(&agent.dirs).iter().any(|g| g == game)
}

async fn report(agent: &Agent) {
    if let Err(e) = agent.report().await {
        tracing::warn!("report: {e:#}");
    }
}

async fn on_message(agent: &Agent, message: ServerMessage, playing: &BTreeMap<String, (u32, Instant)>) {
    let result = match message {
        ServerMessage::NewHead { game, .. } if !busy(agent, &game, playing) => {
            let result = agent.sync_game(&game, Mode::Background).await.map(|_| ());
            report(agent).await;
            result
        }
        ServerMessage::Restore { game, version } => {
            let result = if busy(agent, &game, playing) {
                agent
                    .ui
                    .info(
                        &format!("{game} läuft gerade"),
                        "Der Stand wurde nicht geladen. Beende das Spiel und versuche es erneut.",
                    )
                    .await;
                Ok(())
            } else {
                agent.restore_version(&game, &version).await
            };
            report(agent).await;
            result
        }
        ServerMessage::BackUp { game } => back_up_now(agent, &game).await,
        ServerMessage::Settings { settings } => {
            agent.apply_settings(settings);
            discover(agent, playing).await;
            report(agent).await;
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!("server message: {e:#}");
    }
}

async fn back_up_now(agent: &Agent, game: &str) -> anyhow::Result<()> {
    let scanner = agent.scanner.clone();
    disk::snapshot(
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
async fn connect(agent: &Agent, playing: &Playing) -> anyhow::Result<()> {
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
    let _ = agent.flush().await;
    catch_up(agent, &playing.lock().await.clone()).await;
    report(agent).await;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = ping.tick() => tx.send(Message::Text(serde_json::to_string(&AgentMessage::Ping)?.into())).await?,
            incoming = rx.next() => {
                let Some(message) = incoming else { return Ok(()) };
                if let Message::Text(text) = message?
                    && let Ok(parsed) = serde_json::from_str::<ServerMessage>(&text)
                {
                    let snapshot = playing.lock().await.clone();
                    on_message(agent, parsed, &snapshot).await;
                }
            }
        }
    }
}

/// Syncs every game to follow that is not running; uploads changes, fetches new heads.
async fn catch_up(agent: &Agent, playing: &BTreeMap<String, (u32, Instant)>) {
    let games = agent.games_to_follow().await;
    for game in games.iter().filter(|g| !busy(agent, g, playing)) {
        match agent.sync_game(game, Mode::Background).await {
            Ok(Outcome::Offline) => return,
            Ok(_) => {}
            Err(e) => tracing::warn!("{game}: {e:#}"),
        }
    }
}

/// Games with saves this agent does not know yet.
async fn discover(agent: &Agent, playing: &BTreeMap<String, (u32, Instant)>) {
    let scanner = agent.scanner.clone();
    let Ok(found) = tokio::task::spawn_blocking(move || scanner.games_with_saves()).await else {
        return;
    };
    let known = agent.store.tracked_games().await.unwrap_or_default();
    for game in found
        .iter()
        .filter(|g| !known.contains(g) && !busy(agent, g, playing))
    {
        if let Err(e) = agent.sync_game(game, Mode::Background).await {
            tracing::warn!("{game}: {e:#}");
        }
    }
}

async fn watch_processes(agent: &Agent, playing: &Playing, every: Duration) {
    let mut watcher = Watcher::default();
    let mut installs = vec![];
    let mut installs_at = Instant::now() - Duration::from_secs(3600);
    loop {
        if installs_at.elapsed() > Duration::from_secs(600) {
            let a = agent.clone();
            installs = tokio::task::spawn_blocking(move || a.install_dirs())
                .await
                .unwrap_or_default();
            installs_at = Instant::now();
        }
        let wrapped = running::wrapped(&agent.dirs);
        let mut now_running: BTreeMap<String, u32> = watcher
            .running(&installs)
            .into_iter()
            .filter(|(g, _)| !wrapped.contains(g))
            .collect();
        // Which title an emulator plays is not visible from outside: while one runs, every
        // Switch game here counts as open, so none is written to.
        if let Some(pid) = watcher.program(emulators::PROCESSES) {
            for game in agent.scanner.emulator_games() {
                now_running.entry(game).or_insert(pid);
            }
        }
        let mut state = playing.lock().await;
        for (game, pid) in &now_running {
            if !state.contains_key(game) {
                tracing::info!("{game} started");
                if emulators::title_id(game).is_none() {
                    let _ = agent.client.presence(Some(game)).await;
                }
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
            // A new state that arrived while the game was open was not loaded then; now it is.
            if let Err(e) = agent.sync_game(&game, Mode::Background).await {
                tracing::warn!("{game}: sync after session: {e:#}");
            }
        }
        drop(state);
        tokio::time::sleep(every).await;
    }
}

/// The folders to watch: where each game's saves are now.
async fn watched_folders(agent: &Agent) -> Vec<(PathBuf, String)> {
    let games = agent.games_here().await;
    let agent = agent.clone();
    tokio::task::spawn_blocking(move || {
        games
            .into_iter()
            .filter_map(|g| {
                let files: Vec<PathBuf> = disk::read(agent.scanner.as_ref(), &g)
                    .ok()?
                    .sources
                    .into_iter()
                    .map(|s| s.path)
                    .collect();
                Some((crate::report::common_dir(&files)?, g))
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Records a game's new state once its save files have been quiet; never writes.
async fn watch_saves(agent: &Agent, timings: &Timings) {
    loop {
        let folders = watched_folders(agent).await;
        let dirs: Vec<PathBuf> = folders.iter().map(|(d, _)| d.clone()).collect();
        let Ok((_watcher, mut changes)) = watch::watch(&dirs) else {
            tokio::time::sleep(timings.rewatch).await;
            continue;
        };
        let mut debounce = Debounce::new(timings.quiet);
        // A change made while no watcher was set up is not lost: every watched game is looked
        // at once after each (re)start.
        for (_, game) in &folders {
            debounce.touch(game, Instant::now());
        }
        let until = Instant::now() + timings.rewatch;
        while Instant::now() < until {
            let wait = debounce
                .next_in(Instant::now())
                .unwrap_or(timings.rewatch)
                .min(until.saturating_duration_since(Instant::now()));
            tokio::select! {
                _ = agent.places_changed.notified() => break,
                Some(path) = changes.recv() => {
                    if let Some(game) = watch::game_for(&path, &folders) {
                        debounce.touch(game, Instant::now());
                    }
                }
                _ = tokio::time::sleep(wait) => {
                    let due = debounce.due(Instant::now());
                    for game in &due {
                        if let Err(e) = agent.observe(game).await {
                            tracing::warn!("{game}: {e:#}");
                        }
                    }
                    if !due.is_empty() {
                        report(agent).await;
                    }
                }
            }
        }
    }
}

pub async fn run(agent: Agent) -> anyhow::Result<()> {
    run_with(agent, Timings::from_env()).await
}

pub async fn run_with(agent: Agent, timings: Timings) -> anyhow::Result<()> {
    let playing: Playing = Arc::default();
    agent.refresh_settings().await;
    discover(&agent, &BTreeMap::new()).await;
    report(&agent).await;

    let (a, p, every) = (agent.clone(), playing.clone(), timings.process_poll);
    tokio::spawn(async move { watch_processes(&a, &p, every).await });

    let (a, t) = (agent.clone(), timings.clone());
    tokio::spawn(async move { watch_saves(&a, &t).await });

    let (a, p, t) = (agent.clone(), playing.clone(), timings.clone());
    tokio::spawn(async move {
        let mut last_full = Instant::now();
        loop {
            tokio::time::sleep(t.catch_up).await;
            let _ = a.flush().await;
            let snapshot = p.lock().await.clone();
            catch_up(&a, &snapshot).await;
            if last_full.elapsed() > t.full_scan {
                a.configure_scanner();
                discover(&a, &snapshot).await;
                last_full = Instant::now();
            }
            report(&a).await;
        }
    });

    let mut backoff = timings.reconnect_min;
    loop {
        match connect(&agent, &playing).await {
            Ok(()) => backoff = timings.reconnect_min,
            Err(e) => tracing::warn!("live connection: {e:#}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(timings.reconnect_max);
        let _ = agent.flush().await;
    }
}
