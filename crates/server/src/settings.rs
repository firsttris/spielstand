use std::path::PathBuf;

/// Everything the server reads from flags or the environment.
#[derive(clap::Args, Clone, Debug)]
pub struct Settings {
    /// Folder for the database, the packages and the trash.
    #[arg(long, env = "SPIELSTAND_DATA", default_value = "/data")]
    pub data_dir: PathBuf,

    #[arg(long, env = "SPIELSTAND_LISTEN", default_value = "0.0.0.0:8467")]
    pub listen: String,

    /// The address agents reach the server at, used in the install command.
    #[arg(long, env = "SPIELSTAND_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Offsite mirror: an rclone remote such as `gdrive:Spielstand`, or a local folder.
    #[arg(long, env = "SPIELSTAND_MIRROR")]
    pub mirror: Option<String>,

    #[arg(long, env = "SPIELSTAND_RCLONE", default_value = "rclone")]
    pub rclone: String,

    /// Folder with `spielstand-agent-<arch>-linux` binaries served to `install.sh`.
    #[arg(
        long,
        env = "SPIELSTAND_AGENT_DIR",
        default_value = "/usr/share/spielstand/agent"
    )]
    pub agent_dir: PathBuf,

    /// Mark the session cookie `Secure` (behind HTTPS).
    #[arg(long, env = "SPIELSTAND_SECURE_COOKIES", default_value_t = false)]
    pub secure_cookies: bool,

    /// Where the names of Switch titles come from: a titledb file as URL or path; empty: none.
    #[arg(
        long,
        env = "SPIELSTAND_SWITCH_TITLES",
        default_value = "https://raw.githubusercontent.com/blawar/titledb/master/US.en.json"
    )]
    pub switch_titles: String,
}

impl Settings {
    pub fn for_tests(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            listen: "127.0.0.1:0".into(),
            public_url: None,
            mirror: None,
            rclone: "rclone".into(),
            agent_dir: PathBuf::from("/nonexistent"),
            secure_cookies: false,
            switch_titles: String::new(),
        }
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.data_dir.join("tmp")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("spielstand.db")
    }
}
