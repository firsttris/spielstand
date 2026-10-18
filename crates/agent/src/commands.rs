//! The commands that look and decide, for people (German) and assistants (`--json`): part of
//! the binary, so the library stays free of printing.

use std::{path::PathBuf, sync::Arc};

use clap::Subcommand;
use spielstand_agent::{
    Agent, Silent, choices,
    client::Refused,
    config::{AgentConfig, Dirs},
    custom,
    emulators::{Xdg, scanner::WithEmulators},
    engine::{Ludusavi, Scanner},
    inspect, settings, skill, text,
};
use spielstand_protocol::CustomGame;

pub fn scanner(dirs: &Dirs) -> anyhow::Result<Arc<dyn Scanner>> {
    let ludusavi: Arc<dyn Scanner> = Arc::new(Ludusavi::open(&dirs.ludusavi())?);
    Ok(Arc::new(WithEmulators::new(ludusavi, Xdg::from_env(&home()))))
}

#[derive(Subcommand)]
pub enum CustomCommand {
    /// The custom games in effect on this PC.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Propose a custom game for this PC; `--dry-run` only shows what it would back up.
    Add {
        /// The game's usual name (as on Steam or its own site).
        #[arg(long)]
        name: String,
        /// A path with the manifest's placeholders, e.g. `<xdgData>/Game/saves`; repeatable.
        #[arg(long = "path", required = true)]
        paths: Vec<String>,
        /// A folder name the game is installed as (for `<base>`); repeatable.
        #[arg(long = "install-dir")]
        install_dirs: Vec<String>,
        /// Propose it although no file matches yet.
        #[arg(long)]
        allow_empty: bool,
        /// Only show the files it would back up; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Print the files as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Withdraw a custom game this PC proposed.
    Remove { name: String },
}

#[derive(Subcommand)]
pub enum SkillCommand {
    /// Write it to Claude Code's skills folder (`~/.claude/skills/spielstand/SKILL.md`).
    Install {
        /// The skills folder, if not Claude Code's own.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Print it.
    Show,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// The scanner as the service sees it: with the settings' prefixes, game folders and custom
/// games.
fn configured_scanner(dirs: &Dirs) -> anyhow::Result<Arc<dyn Scanner>> {
    let scanner = scanner(dirs)?;
    let sources = settings::Sources::from_env();
    settings::configure(scanner.as_ref(), &settings::load(dirs), &sources, &home());
    Ok(scanner)
}

/// An agent for the commands that look and decide, with the newest settings if the server is
/// there.
async fn inspector(dirs: &Dirs) -> anyhow::Result<Agent> {
    let agent = spielstand_agent::open(dirs.clone(), scanner(dirs)?, Arc::new(Silent)).await?;
    agent.refresh_settings().await;
    Ok(agent)
}

fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// The data as JSON, or in words.
fn output<T: serde::Serialize>(
    json: bool,
    value: &T,
    words: impl FnOnce(&T) -> String,
) -> anyhow::Result<()> {
    match json {
        true => print_json(value),
        false => {
            println!("{}", words(value));
            Ok(())
        }
    }
}

/// Says why and ends with exit code 1.
fn refuse(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

/// A server refusal or a server that is not there, in words; anything else as it is.
fn server_said(result: anyhow::Result<()>) -> anyhow::Result<()> {
    let Err(e) = result else {
        return Ok(());
    };
    if let Some(r) = e.downcast_ref::<Refused>() {
        refuse(&text::refused(r));
    }
    if e.downcast_ref::<reqwest::Error>().is_some() {
        refuse("Der Server ist nicht erreichbar; es wurde nichts geändert.");
    }
    Err(e)
}

pub async fn custom_command(dirs: &Dirs, command: CustomCommand) -> anyhow::Result<()> {
    let agent = inspector(dirs).await?;
    match command {
        CustomCommand::List { json } => {
            let s = agent.settings();
            let list = serde_json::json!({
                "customGames": s.custom_games,
                "proposedGames": s.proposed_games,
            });
            output(json, &list, |_| {
                text::custom_games(&s.custom_games, &s.proposed_games)
            })
        }
        CustomCommand::Add {
            name,
            paths,
            install_dirs,
            allow_empty,
            dry_run,
            json,
        } => {
            let game = CustomGame {
                name,
                files: paths,
                install_dir: install_dirs,
                extend: false,
            };
            let preview = custom::preview(&agent, &game, allow_empty)
                .await
                .unwrap_or_else(|r| refuse(&text::refusal(&r)));
            output(json, &preview, text::preview)?;
            if dry_run {
                return Ok(());
            }
            server_said(custom::propose(&agent, &preview).await)?;
            if !json {
                println!(
                    "\nVorgeschlagen. Es gilt nur auf diesem PC und wird nur gesichert, bis es in der Web-UI \
                     (Einstellungen → Eigene Spiele) für alle PCs übernommen wird."
                );
            }
            Ok(())
        }
        CustomCommand::Remove { name } => {
            server_said(custom::withdraw(&agent, &name).await)?;
            println!("„{name}“ ist zurückgezogen; seine Versionen bleiben.");
            Ok(())
        }
    }
}

pub async fn choose(dirs: &Dirs, game: String, path: PathBuf) -> anyhow::Result<()> {
    let agent = inspector(dirs).await?;
    let path = choices::check(&agent, &game, &path).unwrap_or_else(|r| refuse(&text::choice_refusal(&r)));
    server_said(choices::choose(&agent, &game, &path).await)?;
    println!("{game}: {} ist auf diesem PC gewählt.", path.display());
    Ok(())
}

pub async fn status(dirs: &Dirs, json: bool) -> anyhow::Result<()> {
    let config = AgentConfig::load(dirs)?;
    let store = spielstand_agent::store::Store::open(&dirs.db()).await?;
    let games = store.tracked_games().await?.len();
    let pending = store.pending_count().await?;
    if json {
        return print_json(&serde_json::json!({
            "device": config.name,
            "server": config.server,
            "games": games,
            "pending": pending,
        }));
    }
    println!("Gerät:      {}", config.name);
    println!("Server:     {}", config.server);
    println!("Spiele:     {games}");
    println!("Ausstehend: {pending}");
    Ok(())
}

pub fn skill_command(command: SkillCommand) -> anyhow::Result<()> {
    match command {
        SkillCommand::Install { dir } => {
            let file = skill::install(&dir.unwrap_or_else(|| skill::default_dir(&home())))?;
            println!("Skill für Claude Code geschrieben: {}", file.display());
        }
        SkillCommand::Show => print!("{}", skill::SKILL),
    }
    Ok(())
}

pub fn scan(dirs: &Dirs, json: bool) -> anyhow::Result<()> {
    let games = configured_scanner(dirs)?.games_with_saves();
    output(json, &games, |g| g.join("\n"))
}

pub async fn games(dirs: &Dirs, json: bool) -> anyhow::Result<()> {
    let games = inspect::games(&inspector(dirs).await?).await;
    output(json, &games, text::games)
}

pub async fn explain(dirs: &Dirs, game: &str, json: bool) -> anyhow::Result<()> {
    let agent = inspector(dirs).await?;
    let Some(explained) = inspect::explain(&agent, game).await else {
        let hint = agent
            .scanner
            .find_title(game)
            .map(|t| format!(" Gemeint ist vielleicht „{t}“."))
            .unwrap_or_default();
        refuse(&format!("Weder dieser PC noch der Server kennt „{game}“.{hint}"));
    };
    output(json, &explained, text::explained)
}

pub async fn backup_only(dirs: &Dirs, game: &str) -> anyhow::Result<()> {
    let agent = inspector(dirs).await?;
    server_said(choices::backup_only(&agent, game).await)?;
    println!("{game} wird ab jetzt auf allen PCs nur gesichert.");
    Ok(())
}
