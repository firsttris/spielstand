#![allow(dead_code)]

use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use spielstand_agent::{
    Agent, Choice, Ui,
    client::Client,
    config::{AgentConfig, Dirs},
    engine::{FakeScanner, Scanner},
    settings::Sources,
    store::Store,
};
use spielstand_core::portable::Anchors;
use spielstand_protocol::PairRequest;
use spielstand_server::{AppState, Settings, router};

pub const GAME: &str = "Elden Ring";

pub struct Server {
    pub url: String,
    pub state: AppState,
    pub cookie: String,
    pub dir: tempfile::TempDir,
}

pub async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::open(Settings::for_tests(dir.path().join("server")))
        .await
        .unwrap();
    tokio::fs::create_dir_all(state.settings.tmp_dir()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(axum::serve(listener, router(state.clone())).into_future());
    let res = reqwest::Client::new()
        .post(format!("{url}/api/setup"))
        .header("x-spielstand-csrf", "1")
        .json(&serde_json::json!({ "password": "correct horse" }))
        .send()
        .await
        .unwrap();
    let cookie = res.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    Server {
        url,
        state,
        cookie,
        dir,
    }
}

/// Answers conflict questions from a script; `NoAnswer` once it runs out.
#[derive(Default)]
pub struct Scripted {
    pub answers: Mutex<VecDeque<Choice>>,
    pub asked: Mutex<Vec<String>>,
    pub notices: Mutex<Vec<String>>,
}

#[async_trait]
impl Ui for Scripted {
    async fn ask_conflict(&self, game: &str, _other: &str, _timeout: Duration) -> Choice {
        self.asked.lock().unwrap().push(game.to_string());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Choice::NoAnswer)
    }
    async fn warn_playing_elsewhere(&self, game: &str, device: &str) {
        self.notices.lock().unwrap().push(format!("{game} on {device}"));
    }
    async fn info(&self, title: &str, _body: &str) {
        self.notices.lock().unwrap().push(title.to_string());
    }
}

pub struct Pc {
    pub agent: Agent,
    pub ui: Arc<Scripted>,
    pub home: PathBuf,
    pub server_url: String,
}

impl Pc {
    pub fn save_file(&self) -> PathBuf {
        self.home.join(".local/share/EldenRing/ER0000.sl2")
    }

    pub fn write_save(&self, content: &str) {
        let f = self.save_file();
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, content).unwrap();
    }

    pub fn read_save(&self) -> Option<String> {
        std::fs::read_to_string(self.save_file()).ok()
    }

    /// The same PC, but the server can't be reached.
    pub fn offline(&self) -> Agent {
        let mut agent = self.agent.clone();
        agent.client = Client::new("http://127.0.0.1:9", &self.agent.config.token);
        agent
    }
}

pub async fn pc(server: &Server, name: &str, steam_cloud: bool) -> Pc {
    pc_with(server, name, |home| {
        let mut games = BTreeMap::new();
        games.insert(GAME.to_string(), vec![home.join(".local/share/EldenRing")]);
        let scanner = FakeScanner {
            anchors: Anchors {
                home: home.to_path_buf(),
                roots: vec![],
            },
            games,
            steam_cloud: if steam_cloud { vec![GAME.into()] } else { vec![] },
            ..Default::default()
        };
        (scanner, Sources::default())
    })
    .await
}

/// A paired PC whose disk and outside programs `setup` describes, given its home folder.
pub async fn pc_with(server: &Server, name: &str, setup: impl FnOnce(&Path) -> (FakeScanner, Sources)) -> Pc {
    pc_with_scanner(server, name, |home| {
        let (scanner, sources) = setup(home);
        (Arc::new(scanner) as Arc<dyn Scanner>, sources)
    })
    .await
}

/// Like [`pc_with`], with any scanner (e.g. the fake disk with the Switch emulators added).
pub async fn pc_with_scanner(
    server: &Server,
    name: &str,
    setup: impl FnOnce(&Path) -> (Arc<dyn Scanner>, Sources),
) -> Pc {
    let http = reqwest::Client::new();
    let code: serde_json::Value = http
        .post(format!("{}/api/pair-codes", server.url))
        .header("cookie", &server.cookie)
        .header("x-spielstand-csrf", "1")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let req = PairRequest {
        code: code["code"].as_str().unwrap().into(),
        name: name.into(),
        hostname: name.into(),
        os: "test".into(),
        agent_version: "test".into(),
    };
    let paired = Client::pair(&server.url, &req).await.unwrap();
    let root = server.dir.path().join(name);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let dirs = Dirs::under(&root);
    let config = AgentConfig {
        server: server.url.clone(),
        device_id: paired.device_id,
        token: paired.token.clone(),
        name: name.into(),
        restore_steam_cloud_games: false,
        question_seconds: 1,
    };
    let (scanner, sources) = setup(&home);
    let ui = Arc::new(Scripted::default());
    let agent = Agent::new(
        dirs,
        config,
        Store::open(&root.join("data/agent.db")).await.unwrap(),
        Client::new(&server.url, &paired.token),
        scanner,
        ui.clone(),
        sources,
    );
    Pc {
        agent,
        ui,
        home,
        server_url: server.url.clone(),
    }
}

/// Every save content the server holds for the game, across all versions.
pub async fn server_contents(server: &Server, game: &str) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM versions WHERE game = ?")
        .bind(game)
        .fetch_all(&server.state.db)
        .await
        .unwrap();
    let mut out = vec![];
    for (id,) in rows {
        let meta = spielstand_server::versions::meta(&server.state.db, &id)
            .await
            .unwrap()
            .unwrap();
        let blob = spielstand_server::blobs::path(&server.state.settings.blobs_dir(), &meta.blob_sha256);
        for bytes in spielstand_core::pack::unpack(&blob, &meta.files)
            .unwrap()
            .into_values()
        {
            out.push(String::from_utf8(bytes).unwrap());
        }
    }
    out
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}

impl Server {
    pub fn admin(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .header("cookie", &self.cookie)
            .header("x-spielstand-csrf", "1")
    }

    /// Sets a game's mode in the web UI and lets `pcs` pick it up.
    pub async fn set_mode(&self, game: &str, mode: Option<&str>, pcs: &[&Pc]) {
        let res = self
            .admin(
                reqwest::Method::PUT,
                &format!("/api/games/{}/mode", urlencode(game)),
            )
            .json(&serde_json::json!({ "mode": mode }))
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success(), "{}", res.text().await.unwrap());
        for pc in pcs {
            pc.agent.refresh_settings().await;
        }
    }

    pub async fn game(&self, game: &str) -> serde_json::Value {
        self.admin(reqwest::Method::GET, &format!("/api/games/{}", urlencode(game)))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn version_count(&self, game: &str) -> i64 {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM versions WHERE game = ?")
            .bind(game)
            .fetch_one(&self.state.db)
            .await
            .unwrap();
        n
    }
}

pub fn urlencode(s: &str) -> String {
    s.replace('%', "%25").replace(' ', "%20").replace('\'', "%27")
}

/// A Wine prefix with the given profile folders (`"tristan->steamuser"` makes a link, as
/// Proton and Prefixr do).
pub fn make_prefix(dir: &Path, users: &[&str]) {
    std::fs::create_dir_all(dir.join("drive_c/users")).unwrap();
    std::fs::write(dir.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
    for u in users {
        match u.split_once("->") {
            Some((link, target)) => {
                std::fs::create_dir_all(dir.join("drive_c/users").join(target)).unwrap();
                std::os::unix::fs::symlink(target, dir.join("drive_c/users").join(link)).unwrap();
            }
            None => std::fs::create_dir_all(dir.join("drive_c/users").join(u)).unwrap(),
        }
    }
}

/// Marks `dir` as a Syncthing folder with the given `.stignore`.
pub fn syncthing_folder(dir: &Path, stignore: &str) {
    std::fs::create_dir_all(dir.join(".stfolder")).unwrap();
    std::fs::write(dir.join(".stignore"), stignore).unwrap();
}
