//! What this device knows about its games, for the command line (`games`, `explain`) and the
//! assistants that drive it: the same report the web UI gets, plus what the device keeps in
//! step, what the server holds and, for one game, every save file.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use spielstand_protocol::{GameProblem, GameReport, HeadSummary, SaveMode};

use crate::{agent::Agent, disk, settings};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    #[serde(flatten)]
    pub report: GameReport,
    /// The name to show, for a game whose key is not one (a Switch title id).
    pub title: Option<String>,
    /// This device keeps the game in step (it has a version of it).
    pub tracked: bool,
    /// The server has a current state of the game.
    pub on_server: bool,
    /// The mode set in the web UI; `effectiveMode` says what applies here.
    pub configured_mode: SaveMode,
    /// The game is a custom game (or a known one with places added).
    pub custom: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Games {
    pub device: String,
    pub server: String,
    /// The server answered; without it `onServer` is unknown.
    pub online: bool,
    pub games: Vec<GameView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundFile {
    pub path: String,
    /// How the file is stored, the same on every device (`<home>/…`, `<prefix>/…`).
    pub portable: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explained {
    #[serde(flatten)]
    pub view: GameView,
    pub files: Vec<FoundFile>,
    /// The version this device's disk held at its last sync.
    pub base: Option<String>,
    pub head: Option<HeadSummary>,
}

/// The save files of `game` on this disk, with how each is stored.
pub fn files(agent: &Agent, game: &str) -> Vec<FoundFile> {
    let Ok(disk) = disk::read(agent.scanner.as_ref(), game) else {
        return vec![];
    };
    let sizes: BTreeMap<&str, u64> = disk.files.iter().map(|f| (f.path.as_str(), f.size)).collect();
    disk.sources
        .iter()
        .map(|s| FoundFile {
            path: s.path.display().to_string(),
            portable: s.portable.clone(),
            size: sizes.get(s.portable.as_str()).copied().unwrap_or(0),
        })
        .collect()
}

fn view(agent: &Agent, game: &str, tracked: bool, on_server: bool) -> GameView {
    let settings = agent.settings();
    GameView {
        report: agent.game_report(game),
        title: agent.scanner.title(game),
        tracked,
        on_server,
        configured_mode: settings::mode_of(&settings, game),
        custom: settings.custom_games.iter().any(|g| g.name == game),
    }
}

/// Why a state from the server could not be loaded here, found without loading it: a prefix
/// that is missing or not clear.
async fn load_problem(agent: &Agent, head: &HeadSummary, game: &str) -> Option<GameProblem> {
    let meta = agent.client.meta(&head.id).await.ok()?;
    agent.prefix_for(game, &meta).err()
}

async fn views(
    agent: &Agent,
    games: BTreeSet<String>,
    heads: Option<&BTreeMap<String, HeadSummary>>,
) -> Vec<GameView> {
    let tracked: BTreeSet<String> = agent
        .store
        .tracked_games()
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut out = vec![];
    for game in games {
        let head = heads.and_then(|h| h.get(&game));
        let mut v = view(agent, &game, tracked.contains(&game), head.is_some());
        if v.report.problem.is_none()
            && let Some(head) = head
        {
            v.report.problem = load_problem(agent, head, &game).await;
        }
        out.push(v);
    }
    out
}

/// Every game this device has something to do with: the ones it keeps in step, the ones with
/// saves here, and the ones on the server that are installed here.
pub async fn games(agent: &Agent) -> Games {
    let heads = agent.client.heads().await.ok().map(|h| h.heads);
    let scanner = agent.scanner.clone();
    let (found, installed) =
        tokio::task::spawn_blocking(move || (scanner.games_with_saves(), scanner.installed_games()))
            .await
            .unwrap_or_default();
    let mut all: BTreeSet<String> = agent.games_here().await.into_iter().collect();
    all.extend(found);
    if let Some(heads) = &heads {
        all.extend(
            heads
                .keys()
                .filter(|g| agent.installed_here(g, &installed))
                .cloned(),
        );
    }
    Games {
        device: agent.config.name.clone(),
        server: agent.config.server.clone(),
        online: heads.is_some(),
        games: views(agent, all, heads.as_ref()).await,
    }
}

/// One game in detail; `None` when neither this device nor the server knows it.
pub async fn explain(agent: &Agent, game: &str) -> Option<Explained> {
    let heads = agent.client.heads().await.ok().map(|h| h.heads);
    let head = heads.as_ref().and_then(|h| h.get(game)).cloned();
    let found = files(agent, game);
    let base = agent.store.base(game).await.ok().flatten().map(|b| b.version);
    let known =
        agent.scanner.in_manifest(game) || agent.settings().custom_games.iter().any(|g| g.name == game);
    if found.is_empty() && head.is_none() && base.is_none() && !known {
        return None;
    }
    let view = views(agent, BTreeSet::from([game.to_string()]), heads.as_ref())
        .await
        .pop()?;
    Some(Explained {
        view,
        files: found,
        base,
        head,
    })
}
