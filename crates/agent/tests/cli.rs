//! The command line an assistant drives, as the real binary against a real server: look at the
//! games, try a custom game out, propose it, choose a prefix, back a game up only.

mod common;

use std::path::{Path, PathBuf};

use serde_json::Value;
use spielstand_agent::{
    client::Client,
    config::{AgentConfig, Dirs},
};
use spielstand_protocol::PairRequest;

const MANIFEST: &str = "Hades:\n  files:\n    <home>/.local/share/Hades:\n      tags: [save]\n";

struct Cli {
    root: PathBuf,
    home: PathBuf,
}

struct Ran {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }
}

impl Cli {
    async fn paired(server: &common::Server) -> Self {
        let root = server.dir.path().join("cli");
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(root.join("manifest.yaml"), MANIFEST).unwrap();
        let code: Value = server
            .admin(reqwest::Method::POST, "/api/pair-codes")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let req = PairRequest {
            code: code["code"].as_str().unwrap().into(),
            name: "desktop".into(),
            hostname: "desktop".into(),
            os: "test".into(),
            agent_version: "test".into(),
        };
        let paired = Client::pair(&server.url, &req).await.unwrap();
        AgentConfig {
            server: server.url.clone(),
            device_id: paired.device_id,
            token: paired.token,
            name: "desktop".into(),
            restore_steam_cloud_games: false,
            question_seconds: 1,
        }
        .save(&Dirs::under(&root.join("agent")))
        .unwrap();
        Self { root, home }
    }

    fn save(&self, rel: &str, content: &str) {
        let file = self.home.join(rel);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }

    async fn run(&self, args: &[&str]) -> Ran {
        let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_spielstand-agent"))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("SPIELSTAND_AGENT_HOME", self.root.join("agent"))
            .env("SPIELSTAND_MANIFEST", self.root.join("manifest.yaml"))
            .env("RUST_LOG", "error")
            .output()
            .await
            .unwrap();
        Ran {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        }
    }
}

fn game<'a>(games: &'a Value, name: &str) -> &'a Value {
    games["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["game"] == name)
        .unwrap_or_else(|| panic!("{name} missing: {games}"))
}

async fn rows(server: &common::Server) -> Vec<Value> {
    server
        .admin(reqwest::Method::GET, "/api/custom-games")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_assistant_tries_a_custom_game_out_and_proposes_it_for_this_pc() {
    let server = common::server().await;
    let cli = Cli::paired(&server).await;
    cli.save(".local/share/Hades/Profile1.sav", "hades");
    cli.save(".local/share/Indie/saves/slot1", "indie");
    cli.save(".local/share/Indie/shader-cache/big.bin", "not a save");
    let indie = [
        "custom",
        "add",
        "--name",
        "Indie Game",
        "--path",
        "<xdgData>/Indie/saves",
    ];

    let tried = cli.run(&[&indie[..], &["--dry-run", "--json"]].concat()).await;
    assert!(tried.ok, "{}", tried.stderr);
    let files = tried.json()["files"].as_array().unwrap().clone();
    assert_eq!(files.len(), 1, "only the save folder: {files:?}");
    assert_eq!(files[0]["portable"], "<home>/.local/share/Indie/saves/slot1");
    assert!(rows(&server).await.is_empty(), "a dry run changes nothing");

    let broad = cli
        .run(&["custom", "add", "--name", "Indie Game", "--path", "<home>/*"])
        .await;
    assert!(!broad.ok);
    assert!(broad.stderr.contains("Ordner mit Namen"), "{}", broad.stderr);
    let known = cli
        .run(&["custom", "add", "--name", "Hades", "--path", "<home>/Hades"])
        .await;
    assert!(!known.ok);
    assert!(known.stderr.contains("--extend"), "{}", known.stderr);
    let nothing = cli
        .run(&["custom", "add", "--name", "Other", "--path", "<home>/Other"])
        .await;
    assert!(!nothing.ok);
    assert!(nothing.stderr.contains("keine Datei"), "{}", nothing.stderr);

    let proposed = cli.run(&indie).await;
    assert!(proposed.ok, "{}", proposed.stderr);
    assert!(proposed.stdout.contains("Web-UI"), "{}", proposed.stdout);
    let listed = rows(&server).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["proposedByName"], "desktop");

    let games = cli.run(&["games", "--json"]).await;
    assert!(games.ok, "{}", games.stderr);
    let games = games.json();
    assert_eq!(games["online"], true);
    let hades = game(&games, "Hades");
    assert_eq!(hades["source"], "ludusavi");
    assert_eq!(hades["tracked"], false);
    let custom = game(&games, "Indie Game");
    assert_eq!(custom["source"], "custom");
    assert_eq!(custom["proposed"], true);
    assert_eq!(custom["configuredMode"], "sync");
    assert_eq!(
        custom["effectiveMode"], "backup",
        "only backed up until taken over"
    );

    let res = server
        .admin(reqwest::Method::POST, "/api/custom-games/Indie%20Game/take-over")
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let explained = cli.run(&["explain", "Indie Game", "--json"]).await;
    assert!(explained.ok, "{}", explained.stderr);
    let explained = explained.json();
    assert_eq!(explained["proposed"], false);
    assert_eq!(explained["effectiveMode"], "sync");
    assert_eq!(explained["files"].as_array().unwrap().len(), 1);
    let words = cli.run(&["explain", "Indie Game"]).await;
    assert!(
        words
            .stdout
            .contains("gespeichert als <home>/.local/share/Indie/saves/slot1"),
        "{}",
        words.stdout
    );

    let removed = cli.run(&["custom", "remove", "Indie Game"]).await;
    assert!(!removed.ok, "taken over: only the web UI removes it");
    assert!(removed.stderr.contains("Web-UI"), "{}", removed.stderr);

    let unknown = cli.run(&["explain", "hades"]).await;
    assert!(
        unknown.stderr.contains("„Hades“"),
        "a hint at the manifest's name: {}",
        unknown.stderr
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prefix_is_chosen_a_game_backed_up_only_and_the_skill_installed() {
    let server = common::server().await;
    let cli = Cli::paired(&server).await;
    let not_a_prefix = cli.home.join("Games/Hades");
    std::fs::create_dir_all(&not_a_prefix).unwrap();
    let path = |p: &Path| p.to_string_lossy().to_string();

    let refused = cli.run(&["choose", "Hades", &path(&not_a_prefix)]).await;
    assert!(!refused.ok);
    assert!(refused.stderr.contains("kein Wine-Prefix"), "{}", refused.stderr);
    let prefix = cli.home.join("Games/Hades/pfx");
    common::make_prefix(&prefix, &["steamuser"]);
    let chosen = cli.run(&["choose", "Hades", "~/Games/Hades/pfx"]).await;
    assert!(chosen.ok, "{}", chosen.stderr);
    let (stored,): (String,) = sqlx::query_as("SELECT prefix FROM prefix_choices WHERE game = 'Hades'")
        .fetch_one(&server.state.db)
        .await
        .unwrap();
    assert_eq!(Path::new(&stored), prefix);

    let backed_up = cli.run(&["backup-only", "Hades"]).await;
    assert!(backed_up.ok, "{}", backed_up.stderr);
    let (mode,): (String,) = sqlx::query_as("SELECT mode FROM game_modes WHERE game = 'Hades'")
        .fetch_one(&server.state.db)
        .await
        .unwrap();
    assert_eq!(mode, "backup");

    let skills = cli.root.join("skills");
    let installed = cli.run(&["skill", "install", "--dir", &path(&skills)]).await;
    assert!(installed.ok, "{}", installed.stderr);
    let text = std::fs::read_to_string(skills.join("spielstand/SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: spielstand\n"));
    for command in ["games --json", "custom add", "--dry-run", "choose", "backup-only"] {
        assert!(text.contains(command), "the skill names `{command}`");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_server_nothing_is_proposed_and_it_says_so() {
    let server = common::server().await;
    let cli = Cli::paired(&server).await;
    cli.save(".local/share/Indie/saves/slot1", "indie");
    let dirs = Dirs::under(&cli.root.join("agent"));
    let mut config = AgentConfig::load(&dirs).unwrap();
    config.server = "http://127.0.0.1:9".into();
    config.save(&dirs).unwrap();

    let tried = cli
        .run(&[
            "custom",
            "add",
            "--name",
            "Indie",
            "--path",
            "<xdgData>/Indie/saves",
            "--dry-run",
        ])
        .await;
    assert!(tried.ok, "a dry run needs no server: {}", tried.stderr);
    let proposed = cli
        .run(&[
            "custom",
            "add",
            "--name",
            "Indie",
            "--path",
            "<xdgData>/Indie/saves",
        ])
        .await;
    assert!(!proposed.ok);
    assert!(
        proposed.stderr.contains("nicht erreichbar"),
        "{}",
        proposed.stderr
    );
}
