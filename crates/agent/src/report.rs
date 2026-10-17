//! What this device tells the server about its games, and the list of save files Stellwerk
//! should leave alone: both are derived from the same scan.

use std::path::{Path, PathBuf};

use spielstand_protocol::{DeviceReport, GameReport, SaveMode};

use crate::{agent::Agent, disk, foreign, settings};

/// How the saves of a game were found, for the web UI.
fn source(prefix: Option<&Path>, prefixr: &[PathBuf], game_folders: &[PathBuf]) -> Option<String> {
    let prefix = prefix?;
    let text = prefix.to_string_lossy();
    let name = if prefixr.iter().any(|p| p == prefix) {
        "prefixr"
    } else if game_folders.iter().any(|f| prefix.starts_with(f)) {
        "gameFolder"
    } else if text.contains("/steamapps/compatdata/") {
        "steam"
    } else if text.contains("/Heroic/") || text.contains("heroic") {
        "heroic"
    } else if text.contains("lutris") {
        "lutris"
    } else {
        "wine"
    };
    Some(name.to_string())
}

/// The folder the saves are in: the deepest folder all of them share.
pub(crate) fn common_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut dir = files.first()?.parent()?.to_path_buf();
    for f in files.iter().skip(1) {
        while !f.starts_with(&dir) {
            dir = dir.parent()?.to_path_buf();
        }
    }
    Some(dir)
}

impl Agent {
    /// The games this device keeps in step and the ones it has a problem with. Cheap: no scan
    /// of the whole manifest (that is `discover`'s job, which makes found games known).
    pub async fn games_here(&self) -> Vec<String> {
        let mut games = self.store.tracked_games().await.unwrap_or_default();
        games.extend(self.problems.lock().unwrap().keys().cloned());
        games.sort();
        games.dedup();
        games
    }

    pub fn game_report(&self, game: &str) -> GameReport {
        let disk = disk::read(self.scanner.as_ref(), game).ok();
        let files: Vec<PathBuf> = disk
            .as_ref()
            .map(|d| d.sources.iter().map(|s| s.path.clone()).collect())
            .unwrap_or_default();
        let prefix = disk.as_ref().and_then(|d| d.prefix.clone());
        let settings = self.settings();
        let prefixr: Vec<PathBuf> = self
            .sources
            .prefixr_games()
            .unwrap_or_default()
            .into_iter()
            .map(|g| g.prefix)
            .collect();
        let folders = settings::game_folders(&settings, &self.scanner.anchors().home);
        let (effective_mode, foreign_sync) = self.effective_mode(game);
        GameReport {
            game: game.to_string(),
            location: common_dir(&files).map(|d| d.display().to_string()),
            source: source(prefix.as_deref(), &prefixr, &folders)
                .or_else(|| (!files.is_empty()).then(|| "ludusavi".into())),
            prefix: prefix.map(|p| p.display().to_string()),
            effective_mode,
            foreign_sync,
            problem: self.problem(game),
        }
    }

    /// Scans every game, tells the server and refreshes Stellwerk's exclusion list.
    pub async fn report(&self) -> anyhow::Result<DeviceReport> {
        let games = self.games_here().await;
        let agent = self.clone();
        let report = tokio::task::spawn_blocking(move || DeviceReport {
            stellwerk_agent: agent.sources.stellwerk_installed(),
            prefixr: agent.sources.prefixr_games().is_some(),
            games: games.iter().map(|g| agent.game_report(g)).collect(),
        })
        .await?;
        self.update_stellwerk(&report.games.iter().map(|g| g.game.clone()).collect::<Vec<_>>());
        self.client.report(&report).await?;
        Ok(report)
    }

    /// The save files of every game Spielstand moves itself, for Stellwerk to leave alone.
    pub fn update_stellwerk(&self, games: &[String]) {
        let Some(dir) = self.sources.stellwerk_dir.clone() else {
            return;
        };
        let files: Vec<PathBuf> = games
            .iter()
            .filter(|g| self.configured_mode(g) == SaveMode::Sync)
            .flat_map(|g| {
                disk::read(self.scanner.as_ref(), g)
                    .map(|d| d.sources)
                    .unwrap_or_default()
            })
            .map(|s| disk::real_path(&s.path))
            .collect();
        match foreign::write_drop_in(&dir, &foreign::drop_in(&files)) {
            Ok(true) => tracing::info!("updated Stellwerk's exclusions"),
            Ok(false) => {}
            Err(e) => tracing::warn!("Stellwerk exclusions: {e:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_says_where_a_prefix_came_from() {
        let p = |s: &str| PathBuf::from(s);
        let prefixr = vec![p("/g/Hades/prefix")];
        let folders = vec![p("/home/t/Games")];
        assert_eq!(
            source(Some(&p("/g/Hades/prefix")), &prefixr, &folders).as_deref(),
            Some("prefixr")
        );
        assert_eq!(
            source(Some(&p("/home/t/Games/ER/pfx")), &prefixr, &folders).as_deref(),
            Some("gameFolder")
        );
        assert_eq!(
            source(Some(&p("/s/steamapps/compatdata/1/pfx")), &prefixr, &folders).as_deref(),
            Some("steam")
        );
        assert_eq!(source(None, &prefixr, &folders), None);
    }

    #[test]
    fn the_location_is_the_shared_folder() {
        let files = vec![PathBuf::from("/a/b/c/x.sav"), PathBuf::from("/a/b/d/y.sav")];
        assert_eq!(common_dir(&files), Some(PathBuf::from("/a/b")));
        assert_eq!(common_dir(&[]), None);
    }
}
