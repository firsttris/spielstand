//! One PC of the stack: a home folder with a Prefixr library and a Wine prefix for Hades, and
//! the real agent binary running against it.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::{Child, Command};

/// The one game of the stack, found in a Wine prefix the way the real manifest describes it.
pub const MANIFEST: &str = r#"
Hades:
  files:
    <winAppData>/Supergiant Games/Hades:
      tags: [save]
"#;

const IN_PROFILE: &str = "AppData/Roaming/Supergiant Games/Hades";

pub struct Pc {
    pub name: String,
    pub root: PathBuf,
    pub home: PathBuf,
    /// The save file as the game sees it.
    pub save: PathBuf,
    manifest: PathBuf,
    child: Option<Child>,
}

fn agent_bin() -> PathBuf {
    if let Some(bin) = std::env::var_os("SPIELSTAND_AGENT_BIN") {
        return PathBuf::from(bin);
    }
    let me = std::env::current_exe().unwrap_or_default();
    // target/<profile>/examples/stack → target/<profile>/spielstand-agent
    me.parent()
        .and_then(Path::parent)
        .map(|dir| dir.join("spielstand-agent"))
        .unwrap_or_else(|| PathBuf::from("spielstand-agent"))
}

/// A prefix as Prefixr makes it: `steamuser`, and the user's name linked to it.
fn make_prefix(prefix: &Path, user: &str) -> std::io::Result<()> {
    let users = prefix.join("drive_c/users");
    std::fs::create_dir_all(users.join("steamuser"))?;
    std::os::unix::fs::symlink("steamuser", users.join(user))?;
    std::fs::write(prefix.join("system.reg"), "WINE REGISTRY Version 2\n")
}

impl Pc {
    pub fn set_up(
        root: &Path,
        name: &str,
        prefix_at: &str,
        first_save: Option<&str>,
        manifest: &Path,
    ) -> anyhow::Result<Self> {
        let home = root.join("home");
        let prefix = home.join(prefix_at);
        make_prefix(&prefix, "gamer")?;
        let exe_dir = prefix.parent().unwrap_or(&prefix).to_path_buf();
        std::fs::create_dir_all(&exe_dir)?;
        let config = home.join(".config/com.tristan.prefixr/config.json");
        std::fs::create_dir_all(config.parent().unwrap_or(&home))?;
        std::fs::write(
            &config,
            serde_json::json!({
                "prefixes": [{ "path": prefix }],
                "games": [{ "id": "1", "name": "Hades", "exe_path": exe_dir.join("Hades.exe"),
                            "prefix_path": prefix, "runner_id": "ge-proton", "notes": "" }],
            })
            .to_string(),
        )?;
        let save = prefix
            .join("drive_c/users/steamuser")
            .join(IN_PROFILE)
            .join("Profile1.sav");
        if let Some(content) = first_save {
            std::fs::create_dir_all(save.parent().unwrap_or(&prefix))?;
            std::fs::write(&save, content)?;
        }
        Ok(Self {
            name: name.into(),
            root: root.to_path_buf(),
            home,
            save,
            manifest: manifest.to_path_buf(),
            child: None,
        })
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(agent_bin());
        cmd.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("SPIELSTAND_AGENT_HOME", self.root.join("agent"))
            .env("SPIELSTAND_MANIFEST", &self.manifest)
            .env("SPIELSTAND_QUIET_SECONDS", "0.5")
            .env("RUST_LOG", "info,ludusavi=warn")
            .env_remove("SPIELSTAND_PREFIXR_CONFIG")
            .env_remove("SPIELSTAND_STELLWERK_DIR");
        cmd
    }

    pub async fn pair(&self, server: &str, code: &str) -> anyhow::Result<()> {
        let bin = agent_bin();
        anyhow::ensure!(
            bin.is_file(),
            "agent binary {} not found: run `cargo build -p spielstand-agent` or set SPIELSTAND_AGENT_BIN",
            bin.display()
        );
        let status = self
            .command()
            .args(["pair", "--server", server, "--code", code, "--name", &self.name])
            .stdout(Stdio::null())
            .status()
            .await?;
        anyhow::ensure!(status.success(), "pairing {} failed", self.name);
        Ok(())
    }

    pub fn start(&mut self) {
        if self.running() {
            return;
        }
        let log = std::fs::File::create(self.root.join("agent.log")).ok();
        let mut cmd = self.command();
        cmd.arg("run").kill_on_drop(true);
        if let Some(log) = log {
            if let Ok(clone) = log.try_clone() {
                cmd.stdout(clone);
            }
            cmd.stderr(log);
        }
        self.child = cmd.spawn().ok();
    }

    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }
}
