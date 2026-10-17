//! HTTP calls to the server.

use std::{path::Path, time::Duration};

use futures::StreamExt;
use spielstand_protocol::{
    BaseReport, HeadsResponse, PairRequest, PairResponse, PresenceReport, PromoteOutcome, PromoteRequest,
    UploadOutcome, VersionMeta, paths,
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

async fn ok(res: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    anyhow::bail!("server answered {status}: {text}")
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
}
