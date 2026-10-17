//! Which games run on this PC: either started through the wrapper (it leaves a lock file) or
//! recognised by a process whose path or command line lies in the game's install folder.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use crate::config::Dirs;

fn lock_path(dirs: &Dirs, game: &str) -> PathBuf {
    dirs.running()
        .join(format!("{}.lock", spielstand_core::slug(game)))
}

pub struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Marks `game` as started through the wrapper until the returned guard is dropped.
pub fn lock(dirs: &Dirs, game: &str) -> anyhow::Result<Lock> {
    std::fs::create_dir_all(dirs.running())?;
    let path = lock_path(dirs, game);
    std::fs::write(&path, format!("{}\n{game}\n", std::process::id()))?;
    Ok(Lock(path))
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Games currently held by a living wrapper.
pub fn wrapped(dirs: &Dirs) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dirs.running()) else {
        return vec![];
    };
    entries
        .flatten()
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let mut lines = text.lines();
            let pid: u32 = lines.next()?.trim().parse().ok()?;
            let game = lines.next()?.to_string();
            if pid_alive(pid) {
                Some(game)
            } else {
                let _ = std::fs::remove_file(e.path());
                None
            }
        })
        .collect()
}

/// Proton passes Windows paths like `Z:\home\…\steamapps\common\Game\game.exe`.
fn normalize(s: &str) -> String {
    let s = s.replace('\\', "/").to_lowercase();
    s.strip_prefix("z:").map(str::to_string).unwrap_or(s)
}

pub struct Watcher {
    system: System,
}

impl Default for Watcher {
    fn default() -> Self {
        Self {
            system: System::new_with_specifics(RefreshKind::nothing()),
        }
    }
}

impl Watcher {
    /// Games whose install folder appears in a running process.
    pub fn running(&mut self, installs: &[(String, PathBuf)]) -> BTreeMap<String, u32> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
        let dirs: Vec<(String, String)> = installs
            .iter()
            .filter(|(_, dir)| dir.components().count() > 3)
            .map(|(game, dir)| (game.clone(), format!("{}/", normalize(&dir.to_string_lossy()))))
            .collect();
        let mut found = BTreeMap::new();
        for (pid, process) in self.system.processes() {
            let mut texts: Vec<String> = process
                .cmd()
                .iter()
                .map(|c| normalize(&c.to_string_lossy()))
                .collect();
            if let Some(exe) = process.exe() {
                texts.push(normalize(&exe.to_string_lossy()));
            }
            for (game, dir) in &dirs {
                if texts.iter().any(|t| t.contains(dir.as_str())) {
                    found.entry(game.clone()).or_insert(pid.as_u32());
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_paths_match_install_dirs() {
        assert_eq!(
            normalize(r"Z:\home\tris\.local\share\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe"),
            "/home/tris/.local/share/steam/steamapps/common/elden ring/game/eldenring.exe"
        );
    }

    #[test]
    fn wrapper_locks_are_released() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::under(dir.path());
        {
            let _lock = lock(&dirs, "Hades II").unwrap();
            assert_eq!(wrapped(&dirs), vec!["Hades II".to_string()]);
        }
        assert!(wrapped(&dirs).is_empty());
    }
}
