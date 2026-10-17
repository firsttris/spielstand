#![allow(dead_code)]

use std::path::Path;

use chrono::Utc;
use spielstand_core::pack::{self, Source};
use spielstand_protocol::{PairRequest, PairResponse, Trigger, UploadOutcome, VersionMeta, paths};
use spielstand_server::{AppState, Settings, framing, router};

pub struct TestServer {
    pub url: String,
    pub state: AppState,
    pub http: reqwest::Client,
    pub dir: tempfile::TempDir,
    pub cookie: String,
}

pub async fn start() -> TestServer {
    start_with(|_| {}).await
}

pub async fn start_with(configure: impl FnOnce(&mut Settings)) -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::for_tests(dir.path().join("data"));
    configure(&mut settings);
    let state = AppState::open(settings).await.unwrap();
    tokio::fs::create_dir_all(state.settings.tmp_dir()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(axum::serve(listener, router(state.clone())).into_future());
    let http = reqwest::Client::new();
    let res = http
        .post(format!("{url}/api/setup"))
        .header("x-spielstand-csrf", "1")
        .json(&serde_json::json!({ "password": "correct horse" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let cookie = res.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    TestServer {
        url,
        state,
        http,
        dir,
        cookie,
    }
}

pub struct TestAgent {
    pub device_id: String,
    pub token: String,
}

impl TestServer {
    pub fn admin(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("cookie", &self.cookie)
            .header("x-spielstand-csrf", "1")
    }

    pub async fn pair(&self, name: &str) -> TestAgent {
        let code: serde_json::Value = self
            .admin(reqwest::Method::POST, "/api/pair-codes")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let req = PairRequest {
            code: code["code"].as_str().unwrap().to_string(),
            name: name.into(),
            hostname: name.into(),
            os: "linux".into(),
            agent_version: "test".into(),
        };
        let res: PairResponse = self
            .http
            .post(format!("{}{}", self.url, paths::PAIR))
            .json(&req)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        TestAgent {
            device_id: res.device_id,
            token: res.token,
        }
    }

    pub fn agent(&self, agent: &TestAgent, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(&agent.token)
    }

    pub async fn upload(&self, agent: &TestAgent, meta: &VersionMeta, blob: &Path) -> reqwest::Response {
        let mut body = framing::encode_prefix(&serde_json::to_vec(meta).unwrap());
        body.extend(std::fs::read(blob).unwrap());
        self.agent(agent, reqwest::Method::POST, paths::VERSIONS)
            .body(body)
            .send()
            .await
            .unwrap()
    }

    pub async fn upload_ok(&self, agent: &TestAgent, meta: &VersionMeta, blob: &Path) -> UploadOutcome {
        let res = self.upload(agent, meta, blob).await;
        assert!(
            res.status().is_success(),
            "upload failed: {}",
            res.text().await.unwrap()
        );
        res.json().await.unwrap()
    }
}

/// Packs one save file with the given content into a version of `game`.
pub fn version(
    dir: &Path,
    game: &str,
    content: &str,
    parent: Option<&str>,
) -> (VersionMeta, std::path::PathBuf) {
    let id = spielstand_core::new_version_id(Utc::now());
    let file = dir.join(format!("{id}.sav"));
    std::fs::write(&file, content).unwrap();
    let blob = dir.join(format!("{id}.tar.zst"));
    let packed = pack::pack(
        &[Source {
            portable: "<home>/.local/share/G/save.dat".into(),
            path: file,
        }],
        &blob,
    )
    .unwrap();
    let meta = VersionMeta {
        id,
        game: game.into(),
        parent: parent.map(str::to_string),
        device: String::new(),
        created_at: Utc::now(),
        trigger: Trigger::Session,
        session_seconds: Some(60),
        content_hash: packed.content_hash,
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix: None,
        title: None,
    };
    (meta, blob)
}
