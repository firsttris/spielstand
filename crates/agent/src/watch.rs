//! Backing up the moment a save changes: the folders the saves are in are watched, and once a
//! game's files have been quiet for a while (a game writes a save in several steps), the new
//! state is recorded. Nothing here writes saves.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Which game a changed path belongs to: the game of the deepest watched folder containing it.
pub fn game_for<'a>(path: &Path, folders: &'a [(PathBuf, String)]) -> Option<&'a str> {
    if path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains(".spielstand-"))
    {
        return None;
    }
    folders
        .iter()
        .filter(|(dir, _)| path.starts_with(dir))
        .max_by_key(|(dir, _)| dir.as_os_str().len())
        .map(|(_, game)| game.as_str())
}

/// Games whose files changed, each due once it was quiet for `quiet`.
#[derive(Debug)]
pub struct Debounce {
    quiet: Duration,
    last: BTreeMap<String, Instant>,
}

impl Debounce {
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            last: BTreeMap::new(),
        }
    }

    pub fn touch(&mut self, game: &str, now: Instant) {
        self.last.insert(game.to_string(), now);
    }

    /// The games that are due now; they are forgotten until they change again.
    pub fn due(&mut self, now: Instant) -> Vec<String> {
        let due: Vec<String> = self
            .last
            .iter()
            .filter(|(_, at)| now.duration_since(**at) >= self.quiet)
            .map(|(g, _)| g.clone())
            .collect();
        for g in &due {
            self.last.remove(g);
        }
        due
    }

    /// How long until the next game is due, if any is waiting.
    pub fn next_in(&self, now: Instant) -> Option<Duration> {
        self.last
            .values()
            .map(|at| self.quiet.saturating_sub(now.duration_since(*at)))
            .min()
    }
}

/// Watches `folders` (recursively); changed paths arrive on the receiver. The watcher stops
/// when it is dropped.
pub fn watch(
    folders: &[PathBuf],
) -> anyhow::Result<(notify::RecommendedWatcher, mpsc::UnboundedReceiver<PathBuf>)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && !matches!(event.kind, notify::EventKind::Access(_))
        {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })?;
    for folder in folders {
        if let Err(e) = watcher.watch(folder, RecursiveMode::Recursive) {
            tracing::warn!("cannot watch {}: {e}", folder.display());
        }
    }
    Ok((watcher, rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_belongs_to_the_deepest_watched_folder() {
        let folders = vec![
            (PathBuf::from("/h/.local/share"), "A".to_string()),
            (PathBuf::from("/h/.local/share/B"), "B".to_string()),
        ];
        assert_eq!(
            game_for(Path::new("/h/.local/share/B/s.sav"), &folders),
            Some("B")
        );
        assert_eq!(game_for(Path::new("/h/.local/share/x.sav"), &folders), Some("A"));
        assert_eq!(game_for(Path::new("/elsewhere/x"), &folders), None);
        assert_eq!(
            game_for(Path::new("/h/.local/share/B/.s.sav.spielstand-new"), &folders),
            None,
            "the agent's own staging files"
        );
    }

    #[test]
    fn a_game_is_due_once_its_files_are_quiet() {
        let start = Instant::now();
        let mut d = Debounce::new(Duration::from_secs(10));
        d.touch("A", start);
        d.touch("B", start + Duration::from_secs(4));
        assert!(d.due(start + Duration::from_secs(9)).is_empty());
        assert_eq!(
            d.next_in(start + Duration::from_secs(9)),
            Some(Duration::from_secs(1))
        );
        d.touch("A", start + Duration::from_secs(9));
        assert_eq!(d.due(start + Duration::from_secs(14)), vec!["B".to_string()]);
        assert_eq!(d.due(start + Duration::from_secs(19)), vec!["A".to_string()]);
        assert!(d.due(start + Duration::from_secs(60)).is_empty());
        assert_eq!(d.next_in(start), None);
    }

    #[tokio::test]
    async fn changes_on_disk_arrive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("saves");
        std::fs::create_dir_all(&dir).unwrap();
        let (_watcher, mut rx) = watch(std::slice::from_ref(&dir)).unwrap();
        std::fs::write(dir.join("a.sav"), "1").unwrap();
        let path = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(path.starts_with(&dir));
    }
}
