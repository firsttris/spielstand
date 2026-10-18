//! Custom games from this device: try one out on the disk (which files it would back up, how
//! they would be stored) and propose it. A proposal is in effect only here, and only backed up,
//! until it is taken over in the web UI.

use std::collections::BTreeSet;

use serde::Serialize;
use spielstand_core::custom::{self, Invalid, TooMuch};
use spielstand_protocol::CustomGame;

use crate::{
    agent::Agent,
    inspect::{self, FoundFile},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub game: CustomGame,
    pub files: Vec<FoundFile>,
    pub total_bytes: u64,
}

/// Why a custom game is not proposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Invalid(Invalid),
    /// The manifest knows the game, or it extends one: a PC proposes only new games.
    InManifest,
    /// The paths match no file on this disk.
    Empty,
    TooMuch(TooMuch),
    /// A file found already belongs to another game here.
    Overlap {
        file: String,
        game: String,
    },
}

/// The custom games of the settings with `game` in place of the one of its name.
fn with(agent: &Agent, game: &CustomGame) -> Vec<CustomGame> {
    let mut games: Vec<CustomGame> = agent
        .settings()
        .custom_games
        .into_iter()
        .filter(|g| g.name != game.name)
        .collect();
    games.push(game.clone());
    games
}

/// The other game here that already backs up one of `files`.
async fn overlap(agent: &Agent, name: &str, files: &[FoundFile]) -> Option<(String, String)> {
    let mine: BTreeSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let mut others: BTreeSet<String> = agent
        .store
        .tracked_games()
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
    others.extend(agent.settings().custom_games.into_iter().map(|g| g.name));
    others.remove(name);
    others.into_iter().find_map(|other| {
        inspect::files(agent, &other)
            .into_iter()
            .find(|f| mine.contains(f.path.as_str()))
            .map(|f| (f.path, other))
    })
}

/// What `game` would back up on this disk, or why it would not be proposed. The agent's scanner
/// holds the settings' custom games again afterwards.
pub async fn preview(agent: &Agent, game: &CustomGame, allow_empty: bool) -> Result<Preview, Refusal> {
    let game = custom::check(game).map_err(Refusal::Invalid)?;
    // Only new games: more places for a known one are added in the web UI (see the server's
    // `custom_games::is_new`).
    if game.extend || agent.scanner.in_manifest(&game.name) {
        return Err(Refusal::InManifest);
    }
    agent.scanner.set_custom_games(&with(agent, &game));
    let files = inspect::files(agent, &game.name);
    agent.configure_scanner();
    if files.is_empty() && !allow_empty {
        return Err(Refusal::Empty);
    }
    let sizes: Vec<u64> = files.iter().map(|f| f.size).collect();
    custom::check_found(&sizes).map_err(Refusal::TooMuch)?;
    if let Some((file, other)) = overlap(agent, &game.name, &files).await {
        return Err(Refusal::Overlap { file, game: other });
    }
    Ok(Preview {
        total_bytes: sizes.iter().sum(),
        game,
        files,
    })
}

/// Proposes a previewed custom game and keeps the settings the server answers with.
pub async fn propose(agent: &Agent, preview: &Preview) -> anyhow::Result<()> {
    let settings = agent.client.propose(&preview.game).await?;
    agent.apply_settings(settings);
    Ok(())
}

/// Withdraws a custom game this device proposed.
pub async fn withdraw(agent: &Agent, name: &str) -> anyhow::Result<()> {
    let settings = agent.client.withdraw(name).await?;
    agent.apply_settings(settings);
    Ok(())
}
