//! Where the agent keeps its settings (`~/.config/spielstand`) and data (`~/.local/share/spielstand`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Dirs {
    /// `SPIELSTAND_AGENT_HOME` puts everything below one folder (tests, portable installs).
    pub fn from_env() -> Self {
        if let Ok(home) = std::env::var("SPIELSTAND_AGENT_HOME") {
            let home = PathBuf::from(home);
            return Self {
                config: home.join("config"),
                data: home.join("data"),
            };
        }
        let config = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("spielstand");
        let data = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join("spielstand");
        Self { config, data }
    }

    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("agent.toml")
    }
    pub fn db(&self) -> PathBuf {
        self.data.join("agent.db")
    }
    pub fn outbox(&self) -> PathBuf {
        self.data.join("outbox")
    }
    pub fn cache(&self) -> PathBuf {
        self.data.join("cache")
    }
    pub fn trash(&self) -> PathBuf {
        self.data.join("trash")
    }
    pub fn running(&self) -> PathBuf {
        self.data.join("running")
    }
    /// Its time is when the agent last looked through the whole manifest.
    pub fn last_full_scan(&self) -> PathBuf {
        self.data.join("last-full-scan")
    }
    pub fn ludusavi(&self) -> PathBuf {
        self.data.join("ludusavi")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AgentConfig {
    pub server: String,
    pub device_id: String,
    pub token: String,
    pub name: String,
    /// Games whose saves Steam Cloud syncs are only backed up, unless this is on.
    #[serde(default)]
    pub restore_steam_cloud_games: bool,
    /// How long a conflict question waits for an answer before the local state is kept.
    #[serde(default = "default_question_seconds")]
    pub question_seconds: u64,
}

fn default_question_seconds() -> u64 {
    60
}

impl AgentConfig {
    pub fn load(dirs: &Dirs) -> anyhow::Result<Self> {
        let path = dirs.config_file();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("{}: {e} (run `spielstand-agent pair` first)", path.display()))?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, dirs: &Dirs) -> anyhow::Result<()> {
        std::fs::create_dir_all(&dirs.config)?;
        let path = dirs.config_file();
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}
