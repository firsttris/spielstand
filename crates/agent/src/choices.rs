//! What this device may decide for itself from the command line: which prefix (or, for a Switch
//! game, which emulator's save folder) is a game's here, and that a game is only backed up.
//! Syncing a game again is switched on in the web UI.

use std::path::{Path, PathBuf};

use spielstand_protocol::GameProblem;

use crate::{agent::Agent, disk, emulators, prefixes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Missing(PathBuf),
    NotAPrefix(PathBuf),
    /// A Switch game's place is one of its emulators' save folders.
    NotACandidate(Vec<String>),
}

/// The save folders a Switch game could be in here.
fn candidates(agent: &Agent, game: &str) -> Vec<String> {
    match agent.scanner.emulator_place(game) {
        Some(Ok(place)) => vec![place.dir.display().to_string()],
        Some(Err(GameProblem::SeveralEmulators { candidates })) => candidates,
        _ => vec![],
    }
}

/// `path` as the folder to choose for `game` here, or why it does not fit.
pub fn check(agent: &Agent, game: &str, path: &Path) -> Result<PathBuf, Refusal> {
    let path = prefixes::expand_home(&path.to_string_lossy(), &agent.scanner.anchors().home);
    // The server stores absolute paths: the same folder whichever directory the command ran in.
    let path = std::path::absolute(&path).unwrap_or(path);
    if !path.is_dir() {
        return Err(Refusal::Missing(path));
    }
    let real = disk::real_path(&path);
    if emulators::title_id(game).is_some() {
        let options = candidates(agent, game);
        return match options.iter().any(|c| disk::real_path(Path::new(c)) == real) {
            true => Ok(path),
            false => Err(Refusal::NotACandidate(options)),
        };
    }
    match prefixes::is_wine_prefix(&path) {
        true => Ok(path),
        false => Err(Refusal::NotAPrefix(path)),
    }
}

pub async fn choose(agent: &Agent, game: &str, path: &Path) -> anyhow::Result<()> {
    let settings = agent
        .client
        .choose_prefix(game, &path.display().to_string())
        .await?;
    agent.apply_settings(settings);
    Ok(())
}

pub async fn backup_only(agent: &Agent, game: &str) -> anyhow::Result<()> {
    let settings = agent.client.backup_only(game).await?;
    agent.apply_settings(settings);
    Ok(())
}
