mod commands;

use std::{sync::Arc, time::Instant};

use clap::{Parser, Subcommand};
use commands::{CustomCommand, SkillCommand, background_scanner, scanner};
use spielstand_agent::{
    Mode,
    client::Client,
    config::{AgentConfig, Dirs},
    daemon,
    desktop::Desktop,
    running,
};
use spielstand_protocol::{PairRequest, PreviousPairing};
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
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Show the pairing and what waits for upload.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Every game this PC has something to do with: where its saves are, its mode, what is unclear.
    Games {
        #[arg(long)]
        json: bool,
    },
    /// One game in detail: its save files, how they are stored, why it is synced or only backed up.
    Explain {
        game: String,
        #[arg(long)]
        json: bool,
    },
    /// Games the manifest lacks: list, try out and propose, withdraw.
    Custom {
        #[command(subcommand)]
        command: CustomCommand,
    },
    /// Choose the Wine prefix (or a Switch game's emulator save folder) of a game on this PC.
    Choose { game: String, path: std::path::PathBuf },
    /// Only back a game up, on every PC (syncing again is switched on in the web UI).
    BackupOnly { game: String },
    /// The instructions for Claude Code that come with this agent.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
}

/// One server is often reachable under several URLs (port 8467 and a proxy with TLS); its host
/// name tells them apart from another server, so the current token is only sent to that host.
fn same_host(a: &str, b: &str) -> bool {
    let host = |url: &str| reqwest::Url::parse(url).ok()?.host_str().map(str::to_lowercase);
    host(a).is_some() && host(a) == host(b)
}

async fn pair(dirs: &Dirs, server: String, code: String, name: Option<String>) -> anyhow::Result<()> {
    let server = server.trim_end_matches('/').to_string();
    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "pc".into());
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| "Linux".into());
    // Pairing again with the same server keeps the device; another server starts afresh.
    let current = AgentConfig::load(dirs)
        .ok()
        .filter(|c| same_host(&c.server, &server));
    let name = name
        .filter(|n| !n.trim().is_empty())
        .or_else(|| current.as_ref().map(|c| c.name.clone()))
        .unwrap_or_else(|| hostname.clone());
    let req = PairRequest {
        code,
        name: name.clone(),
        hostname,
        os,
        agent_version: env!("CARGO_PKG_VERSION").into(),
        previous: current.as_ref().map(|c| PreviousPairing {
            device_id: c.device_id.clone(),
            token: c.token.clone(),
        }),
    };
    let res = Client::pair(&server, &req).await?;
    let kept = current.as_ref().is_some_and(|c| c.device_id == res.device_id);
    let config = AgentConfig {
        server,
        device_id: res.device_id,
        token: res.token,
        name: name.clone(),
        restore_steam_cloud_games: current.as_ref().is_some_and(|c| c.restore_steam_cloud_games),
        question_seconds: current.as_ref().map_or(60, |c| c.question_seconds),
    };
    config.save(dirs)?;
    if kept {
        println!("\"{name}\" ist erneut gekoppelt (dasselbe Gerät).");
    } else {
        println!("\"{name}\" ist gekoppelt.");
    }
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
            let agent =
                spielstand_agent::open(dirs.clone(), background_scanner(&dirs)?, Arc::new(Desktop)).await?;
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
        Command::Scan { json } => commands::scan(&dirs, json),
        Command::Status { json } => commands::status(&dirs, json).await,
        Command::Games { json } => commands::games(&dirs, json).await,
        Command::Explain { game, json } => commands::explain(&dirs, &game, json).await,
        Command::Custom { command } => commands::custom_command(&dirs, command).await,
        Command::Choose { game, path } => commands::choose(&dirs, game, path).await,
        Command::BackupOnly { game } => commands::backup_only(&dirs, &game).await,
        Command::Skill { command } => commands::skill_command(command),
    }
}

#[cfg(test)]
mod tests {
    use super::same_host;

    #[test]
    fn the_same_server_under_another_url_is_the_same_host() {
        assert!(same_host(
            "http://spielstand.home:8467",
            "https://Spielstand.home/"
        ));
        assert!(!same_host(
            "http://spielstand.home:8467",
            "http://other.home:8467"
        ));
        assert!(!same_host("not a url", "not a url"));
    }
}
