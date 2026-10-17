use std::{sync::Arc, time::Instant};

use clap::{Parser, Subcommand};
use spielstand_agent::{
    Mode,
    client::Client,
    config::{AgentConfig, Dirs},
    daemon,
    desktop::Desktop,
    emulators::{Xdg, scanner::WithEmulators},
    engine::{Ludusavi, Scanner},
    running,
};
use spielstand_protocol::PairRequest;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "spielstand-agent",
    version,
    about = "Keeps this PC's game saves in sync with the Spielstand server"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair this PC with the server (the install script does this).
    Pair {
        #[arg(long)]
        server: String,
        #[arg(long)]
        code: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Run in the background (the systemd user service).
    Run,
    /// Steam launch option: `spielstand-agent launch %command%`. Loads before, backs up after.
    Launch {
        /// Game name as in the Ludusavi manifest; Steam games are found by `SteamAppId`.
        #[arg(long)]
        game: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Sync one game now, or every known game.
    Sync { game: Option<String> },
    /// List the games with saves on this PC.
    Scan,
    /// Show the pairing and what waits for upload.
    Status,
}

fn scanner(dirs: &Dirs) -> anyhow::Result<Arc<dyn Scanner>> {
    let ludusavi: Arc<dyn Scanner> = Arc::new(Ludusavi::open(&dirs.ludusavi())?);
    let home = dirs::home_dir().unwrap_or_default();
    Ok(Arc::new(WithEmulators::new(ludusavi, Xdg::from_env(&home))))
}

async fn pair(dirs: &Dirs, server: String, code: String, name: Option<String>) -> anyhow::Result<()> {
    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "pc".into());
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| "Linux".into());
    let name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| hostname.clone());
    let req = PairRequest {
        code,
        name: name.clone(),
        hostname,
        os,
        agent_version: env!("CARGO_PKG_VERSION").into(),
    };
    let res = Client::pair(&server, &req).await?;
    let config = AgentConfig {
        server: server.trim_end_matches('/').to_string(),
        device_id: res.device_id,
        token: res.token,
        name: name.clone(),
        restore_steam_cloud_games: false,
        question_seconds: 60,
    };
    config.save(dirs)?;
    println!("\"{name}\" ist gekoppelt.");
    Ok(())
}

async fn launch(dirs: Dirs, game: Option<String>, command: Vec<String>) -> anyhow::Result<i32> {
    let opened = async {
        let scanner = scanner(&dirs)?;
        let game = game.or_else(|| {
            let app: u32 = std::env::var("SteamAppId").ok()?.parse().ok()?;
            scanner.game_for_steam_app(app)
        });
        anyhow::Ok((
            spielstand_agent::open(dirs.clone(), scanner, Arc::new(Desktop)).await?,
            game,
        ))
    }
    .await;
    let (agent, game) = match opened {
        Ok((agent, Some(game))) => (Some(agent), Some(game)),
        Ok((_, None)) => (None, None),
        Err(e) => {
            tracing::error!("not syncing: {e:#}");
            (None, None)
        }
    };
    let _lock = match (&agent, &game) {
        (Some(_), Some(g)) => Some(running::lock(&dirs, g)?),
        _ => None,
    };
    if let (Some(agent), Some(game)) = (&agent, &game) {
        let outcome = agent.prepare_launch(game).await;
        tracing::info!("{game}: before start: {outcome:?}");
    }
    let started = Instant::now();
    let status = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .status()
        .await?;
    if let (Some(agent), Some(game)) = (&agent, &game) {
        let outcome = agent.after_session(game, started.elapsed().as_secs()).await?;
        tracing::info!("{game}: after session: {outcome:?}");
    }
    Ok(status.code().unwrap_or(1))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,ludusavi=warn")),
        )
        .init();
    let dirs = Dirs::from_env();
    match Cli::parse().command {
        Command::Pair { server, code, name } => pair(&dirs, server, code, name).await,
        Command::Run => {
            let agent = spielstand_agent::open(dirs.clone(), scanner(&dirs)?, Arc::new(Desktop)).await?;
            daemon::run(agent).await
        }
        Command::Launch { game, command } => std::process::exit(launch(dirs, game, command).await?),
        Command::Sync { game } => {
            let agent = spielstand_agent::open(dirs.clone(), scanner(&dirs)?, Arc::new(Desktop)).await?;
            let games = match game {
                Some(g) => vec![g],
                None => agent.store.tracked_games().await?,
            };
            for g in games {
                println!("{g}: {:?}", agent.sync_game(&g, Mode::Launch).await?);
            }
            Ok(())
        }
        Command::Scan => {
            for game in scanner(&dirs)?.games_with_saves() {
                println!("{game}");
            }
            Ok(())
        }
        Command::Status => {
            let config = AgentConfig::load(&dirs)?;
            let store = spielstand_agent::store::Store::open(&dirs.db()).await?;
            println!("Gerät:      {}", config.name);
            println!("Server:     {}", config.server);
            println!("Spiele:     {}", store.tracked_games().await?.len());
            println!("Ausstehend: {}", store.pending_count().await?);
            Ok(())
        }
    }
}
