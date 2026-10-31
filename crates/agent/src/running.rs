//! Which games run on this PC: either started through the wrapper (it leaves a lock file) or
//! recognised by a process whose path or command line lies in the game's install folder.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

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
            // sysinfo 0.40 returns a Result; on Linux it is always Ok
            system: System::new_with_specifics(RefreshKind::nothing()).expect("sysinfo reads the processes"),
        }
    }
}

impl Watcher {
    /// Games whose install folder appears in a running process.
    pub fn running(&mut self, installs: &[(String, PathBuf)]) -> BTreeMap<String, u32> {
        // Every 5 seconds: only what is matched (command line, executable), read once per
        // process, and no threads, which share their process's command line and executable.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .without_tasks(),
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

impl Watcher {
    /// A running process of one of `programs` (by executable name, or by process name, which
    /// Linux cuts to 15 characters), from the processes `running` just read.
    pub fn program(&self, programs: &[&str]) -> Option<u32> {
        self.system.processes().iter().find_map(|(pid, process)| {
            let exe = process
                .exe()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().to_string());
            let name = process.name().to_string_lossy().to_string();
            programs
                .iter()
                .any(|p| exe.as_deref() == Some(*p) || is_process_name(&name, p))
                .then(|| pid.as_u32())
        })
    }
}

/// `name` is what Linux shows for a process of `program`: the first 15 characters.
fn is_process_name(name: &str, program: &str) -> bool {
    let cut: String = program.chars().take(15).collect();
    name == program || name == cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names_are_cut_to_15_characters() {
        assert!(is_process_name("eden", "eden"));
        assert!(is_process_name("Ryujinx.Headles", "Ryujinx.Headless.SDL2"));
        assert!(!is_process_name("edenx", "eden"));
        assert!(!is_process_name("Ryujinx.Headles", "Ryujinx"));
    }

    #[test]
    fn proton_paths_match_install_dirs() {
        assert_eq!(
            normalize(r"Z:\home\tris\.local\share\Steam\steamapps\common\ELDEN RING\Game\eldenring.exe"),
            "/home/tris/.local/share/steam/steamapps/common/elden ring/game/eldenring.exe"
        );
    }

    /// A real process started from inside an install folder (as Proton starts a game's `.exe`)
    /// is recognised, and no longer once it ended.
    #[test]
    fn a_running_process_inside_an_install_folder_is_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("Games/Some Game/bin");
        std::fs::create_dir_all(&install).unwrap();
        // Keeps the name `sleep`: uutils (Ubuntu 26.04) is one binary that picks its tool by name.
        let exe = install.join("sleep");
        std::fs::copy("/bin/sleep", &exe).unwrap();
        // A file just written can be briefly "busy" for exec while another test forks (ETXTBSY).
        let mut child = (0..20)
            .find_map(|_| match std::process::Command::new(&exe).arg("30").spawn() {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    None
                }
                spawned => Some(spawned.unwrap()),
            })
            .expect("the copied binary stayed busy");
        let installs = vec![("Some Game".to_string(), dir.path().join("Games/Some Game"))];
        let mut watcher = Watcher::default();
        // Right after `spawn` the child may not have exec'd yet and still shows the test binary.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut running = watcher.running(&installs);
        while running.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
            running = watcher.running(&installs);
        }
        assert_eq!(running.get("Some Game"), Some(&child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(watcher.running(&installs).is_empty());
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
