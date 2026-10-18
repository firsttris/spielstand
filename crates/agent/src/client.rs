//! HTTP calls to the server.

use std::{path::Path, time::Duration};

use futures::StreamExt;
use spielstand_protocol::{
    AgentSettings, BackupOnlyRequest, BaseReport, CustomGame, DeviceReport, HeadsResponse, KnownRequest,
    KnownResponse, OverwriteReport, OwnPrefixChoice, PairRequest, PairResponse, PresenceReport,
    PromoteOutcome, PromoteRequest, UploadOutcome, VersionMeta, paths,
};
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    server: String,
    token: String,
}

fn framed_prefix(meta: &VersionMeta) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(meta)?;
    let mut out = (json.len() as u32).to_be_bytes().to_vec();
    out.extend(json);
    Ok(out)
}

/// The server refused a request; `message` is its `error` text.
#[derive(Debug)]
pub struct Refused {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server answered {}: {}", self.status, self.message)
    }
}

impl std::error::Error for Refused {}

async fn ok(res: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or(text);
    Err(Refused { status, message }.into())
}

impl Client {
    pub fn new(server: &str, token: &str) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(600))
            .build()
            .expect("http client");
        Self {
            http,
            server: server.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    pub async fn pair(server: &str, req: &PairRequest) -> anyhow::Result<PairResponse> {
        let res = reqwest::Client::new()
            .post(format!("{}{}", server.trim_end_matches('/'), paths::PAIR))
            .json(req)
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    pub async fn heads(&self) -> anyhow::Result<HeadsResponse> {
        let res = self
            .http
            .get(self.url(paths::HEADS))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    pub async fn upload(&self, meta: &VersionMeta, blob: &Path) -> anyhow::Result<UploadOutcome> {
        let prefix = framed_prefix(meta)?;
        let file = tokio::fs::File::open(blob).await?;
        let body = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes::Bytes::from(prefix)) })
            .chain(tokio_util::io::ReaderStream::new(file));
        let res = self
            .http
            .post(self.url(paths::VERSIONS))
            .bearer_auth(&self.token)
            .body(reqwest::Body::wrap_stream(body))
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    pub async fn meta(&self, id: &str) -> anyhow::Result<VersionMeta> {
        let res = self
            .http
            .get(self.url(&format!("{}/{id}/meta", paths::VERSIONS)))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    /// Downloads a package to `target` (written to a temporary name first).
    pub async fn download(&self, id: &str, target: &Path) -> anyhow::Result<()> {
        let res = ok(self
            .http
            .get(self.url(&paths::version_blob(id)))
            .bearer_auth(&self.token)
            .send()
            .await?)
        .await?;
        if let Some(dir) = target.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let tmp = target.with_extension("partial");
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut stream = res.bytes_stream();
        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk?).await?;
        }
        file.sync_all().await?;
        tokio::fs::rename(&tmp, target).await?;
        Ok(())
    }

    pub async fn promote(&self, id: &str, expected_head: Option<&str>) -> anyhow::Result<PromoteOutcome> {
        let res = self
            .http
            .post(self.url(&paths::version_promote(id)))
            .bearer_auth(&self.token)
            .json(&PromoteRequest {
                expected_head: expected_head.map(str::to_string),
            })
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    pub async fn report_base(&self, game: &str, version: &str) -> anyhow::Result<()> {
        let res = self
            .http
            .post(self.url(paths::BASES))
            .bearer_auth(&self.token)
            .json(&BaseReport {
                game: game.into(),
                version: version.into(),
            })
            .send()
            .await?;
        ok(res).await.map(|_| ())
    }

    pub async fn presence(&self, game: Option<&str>) -> anyhow::Result<()> {
        let res = self
            .http
            .post(self.url(paths::PRESENCE))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(10))
            .json(&PresenceReport {
                game: game.map(str::to_string),
            })
            .send()
            .await?;
        ok(res).await.map(|_| ())
    }

    async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> anyhow::Result<reqwest::Response> {
        let res = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30))
            .json(body)
            .send()
            .await?;
        ok(res).await
    }

    /// Which version has this content, and does it descend from `base`?
    pub async fn known(
        &self,
        game: &str,
        content_hash: &str,
        base: Option<&str>,
    ) -> anyhow::Result<KnownResponse> {
        let req = KnownRequest {
            game: game.into(),
            content_hash: content_hash.into(),
            base: base.map(str::to_string),
        };
        Ok(self.post(paths::KNOWN, &req).await?.json().await?)
    }

    pub async fn overwritten(&self, game: &str, lost: &str, found: &str) -> anyhow::Result<()> {
        let req = OverwriteReport {
            game: game.into(),
            lost: lost.into(),
            found: found.into(),
        };
        self.post(paths::OVERWRITES, &req).await.map(|_| ())
    }

    pub async fn settings(&self) -> anyhow::Result<AgentSettings> {
        let res = self
            .http
            .get(self.url(paths::SETTINGS))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    pub async fn report(&self, report: &DeviceReport) -> anyhow::Result<()> {
        self.post(paths::REPORT, report).await.map(|_| ())
    }

    /// Proposes a custom game for this device; answers with this device's new settings.
    pub async fn propose(&self, game: &CustomGame) -> anyhow::Result<AgentSettings> {
        Ok(self.post(paths::CUSTOM_GAMES, game).await?.json().await?)
    }

    /// Withdraws a custom game this device proposed.
    pub async fn withdraw(&self, name: &str) -> anyhow::Result<AgentSettings> {
        let res = self
            .http
            .delete(self.url(&paths::custom_game(name)))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }

    /// Makes a game only backed up, on every device.
    pub async fn backup_only(&self, game: &str) -> anyhow::Result<AgentSettings> {
        let req = BackupOnlyRequest { game: game.into() };
        Ok(self.post(paths::BACKUP_ONLY, &req).await?.json().await?)
    }

    /// Chooses the prefix (or a Switch game's save folder) of a game on this device.
    pub async fn choose_prefix(&self, game: &str, prefix: &str) -> anyhow::Result<AgentSettings> {
        let req = OwnPrefixChoice {
            game: game.into(),
            prefix: prefix.into(),
        };
        let res = self
            .http
            .put(self.url(paths::PREFIX))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30))
            .json(&req)
            .send()
            .await?;
        Ok(ok(res).await?.json().await?)
    }
}
