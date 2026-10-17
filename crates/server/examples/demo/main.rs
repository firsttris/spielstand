//! A server filled with demo data, for the end-to-end tests and the screenshots.
//!
//! `cargo run -p spielstand-server --example demo` listens on `SPIELSTAND_LISTEN`
//! (default 127.0.0.1:8467) with the admin password `spielstand-demo`. Everything lives in a
//! temporary directory and is gone when the process ends.

mod reports;

/// The demo's Switch game: keyed by its title id, named as the emulator knows it.
const ZELDA: &str = "Switch 0100F2C0115B6000";
const ZELDA_NAME: &str = "The Legend of Zelda: Tears of the Kingdom";

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use futures::SinkExt;
use spielstand_core::pack::{self, Source};
use spielstand_protocol::{
    BaseReport, PairRequest, PairResponse, PresenceReport, Trigger, VersionMeta, paths,
};
use spielstand_server::{Settings, framing};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

pub const PASSWORD: &str = "spielstand-demo";

struct Demo {
    url: String,
    http: reqwest::Client,
    cookie: String,
    dir: std::path::PathBuf,
}

struct Device {
    id: String,
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let listen = std::env::var("SPIELSTAND_LISTEN").unwrap_or_else(|_| "127.0.0.1:8467".into());
    let mirror = tmp.path().join("mirror");
    std::fs::create_dir_all(&mirror)?;
    let mut settings = Settings::for_tests(tmp.path().join("data"));
    settings.listen = listen.clone();
    settings.mirror = Some(mirror.display().to_string());
    settings.public_url = Some(format!("http://{listen}"));
    tokio::spawn(spielstand_server::serve(settings));

    let url = format!("http://{listen}");
    let http = reqwest::Client::new();
    while http.get(format!("{url}/healthz")).send().await.is_err() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let res = http
        .post(format!("{url}/api/setup"))
        .header("x-spielstand-csrf", "1")
        .json(&serde_json::json!({ "password": PASSWORD }))
        .send()
        .await?
        .error_for_status()?;
    let cookie = res.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap_or_default()
        .to_string();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work)?;
    let demo = Demo {
        url,
        http,
        cookie,
        dir: work,
    };
    let _connections = demo.seed().await?;
    println!("demo server ready on http://{listen} (password: {PASSWORD})");
    stopped().await;
    Ok(())
}

impl Demo {
    async fn seed(&self) -> anyhow::Result<Vec<impl Sized + use<>>> {
        let living = self.pair("Wohnzimmer-PC", "bazzite").await?;
        let deck = self.pair("Steam Deck", "steamos").await?;
        let office = self.pair("Arbeitszimmer", "fedora").await?;
        let now = Utc::now();
        let h = |hours: i64| now - Duration::hours(hours);

        // Elden Ring: a long history, the Steam Deck is one version behind.
        let mut parent = None;
        for (i, (device, at, minutes)) in [
            (&living, h(170), 95),
            (&deck, h(120), 40),
            (&living, h(50), 180),
            (&living, h(3), 72),
        ]
        .iter()
        .enumerate()
        {
            let id = self
                .upload(
                    device,
                    "ELDEN RING",
                    &format!("er-{i}"),
                    parent.as_deref(),
                    *at,
                    *minutes,
                )
                .await?;
            if i == 2 {
                self.base(&deck, "ELDEN RING", &id).await?;
            }
            parent = Some(id);
        }
        self.base(&living, "ELDEN RING", parent.as_deref().unwrap_or_default())
            .await?;

        // Hades II: running on the Steam Deck right now.
        let hades = self
            .upload(&living, "Hades II", "hades-0", None, h(26), 55)
            .await?;
        self.base(&deck, "Hades II", &hades).await?;
        self.presence(&deck, Some("Hades II")).await?;

        // Stardew Valley: played on two PCs without a sync in between.
        let farm = self
            .upload(&office, "Stardew Valley", "farm-0", None, h(80), 30)
            .await?;
        self.base(&living, "Stardew Valley", &farm).await?;
        self.upload(&office, "Stardew Valley", "farm-office", Some(&farm), h(6), 45)
            .await?;
        self.upload(&living, "Stardew Valley", "farm-living", Some(&farm), h(2), 65)
            .await?;

        // Two games everyone agrees on.
        let hk = self
            .upload(&office, "Hollow Knight", "hk-0", None, h(300), 120)
            .await?;
        for d in [&living, &deck] {
            self.base(d, "Hollow Knight", &hk).await?;
        }
        // Baldur's Gate 3: the Arbeitszimmer played on, then another program put the older
        // state from the Wohnzimmer back over it.
        let bg = self
            .upload(&living, "Baldur's Gate 3", "bg3-0", None, h(30), 210)
            .await?;
        self.base(&office, "Baldur's Gate 3", &bg).await?;
        let newer = self
            .upload(&office, "Baldur's Gate 3", "bg3-office", Some(&bg), h(20), 150)
            .await?;
        self.agent_post(
            &office,
            paths::OVERWRITES,
            serde_json::json!({ "game": "Baldur's Gate 3", "lost": newer, "found": bg }),
        )
        .await?;
        self.base(&office, "Baldur's Gate 3", &bg).await?;

        // Hollow Knight only backed up; game folders and what each device found.
        self.admin_put(
            "/api/games/Hollow%20Knight/mode",
            serde_json::json!({ "mode": "backup" }),
        )
        .await?;
        for (device, folders, report) in [
            (&living, vec!["~/Games"], reports::living_room()),
            (
                &deck,
                vec!["~/Games", "/run/media/deck/SD/Games"],
                reports::steam_deck(),
            ),
            (&office, vec![], reports::office()),
        ] {
            self.admin_put(
                &format!("/api/devices/{}/settings", device.id),
                serde_json::json!({ "gameFolders": folders }),
            )
            .await?;
            self.agent_post(device, paths::REPORT, report).await?;
        }

        // A Switch game from Eden in the Wohnzimmer; the deck's Ryujinx has not started it yet.
        let zelda = self
            .upload_switch(&living, ZELDA_NAME, "zelda-0", h(10), 95)
            .await?;
        self.base(&living, ZELDA, &zelda).await?;

        // Wohnzimmer and Steam Deck stay connected; the Arbeitszimmer is switched off.
        Ok(vec![self.connect(&living).await?, self.connect(&deck).await?])
    }

    async fn pair(&self, name: &str, os: &str) -> anyhow::Result<Device> {
        let code: serde_json::Value = self
            .http
            .post(format!("{}/api/pair-codes", self.url))
            .header("cookie", &self.cookie)
            .header("x-spielstand-csrf", "1")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let req = PairRequest {
            code: code["code"].as_str().unwrap_or_default().to_string(),
            name: name.into(),
            hostname: name.to_lowercase().replace(' ', "-"),
            os: os.into(),
            agent_version: env!("CARGO_PKG_VERSION").into(),
        };
        let res: PairResponse = self
            .http
            .post(format!("{}{}", self.url, paths::PAIR))
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(Device {
            id: res.device_id,
            token: res.token,
        })
    }

    async fn upload(
        &self,
        device: &Device,
        game: &str,
        content: &str,
        parent: Option<&str>,
        at: DateTime<Utc>,
        minutes: u64,
    ) -> anyhow::Result<String> {
        let (meta, blob) = version(&self.dir, game, content, parent, at, minutes)?;
        self.send(device, meta, blob).await
    }

    /// A Switch game's version: keyed by title id, named by the emulator, files in `<switch>/`.
    async fn upload_switch(
        &self,
        device: &Device,
        title: &str,
        content: &str,
        at: DateTime<Utc>,
        minutes: u64,
    ) -> anyhow::Result<String> {
        let (mut meta, _) = version(&self.dir, ZELDA, content, None, at, minutes)?;
        let save = self.dir.join(format!("{}.switch", meta.id));
        std::fs::write(&save, content.repeat(512))?;
        let blob = self.dir.join(format!("{}.switch.tar.zst", meta.id));
        let packed = pack::pack(
            &[Source {
                portable: "<switch>/slot_0/progress.sav".into(),
                path: save,
            }],
            &blob,
        )?;
        meta.content_hash = packed.content_hash;
        meta.blob_sha256 = packed.blob_sha256;
        meta.blob_size = packed.blob_size;
        meta.files = packed.files;
        meta.title = Some(title.into());
        self.send(device, meta, blob).await
    }

    async fn send(
        &self,
        device: &Device,
        meta: VersionMeta,
        blob: std::path::PathBuf,
    ) -> anyhow::Result<String> {
        let mut body = framing::encode_prefix(&serde_json::to_vec(&meta)?);
        body.extend(std::fs::read(blob)?);
        self.http
            .post(format!("{}{}", self.url, paths::VERSIONS))
            .bearer_auth(&device.token)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(meta.id)
    }

    async fn agent_post(&self, device: &Device, path: &str, body: serde_json::Value) -> anyhow::Result<()> {
        self.http
            .post(format!("{}{path}", self.url))
            .bearer_auth(&device.token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn admin_put(&self, path: &str, body: serde_json::Value) -> anyhow::Result<()> {
        self.http
            .put(format!("{}{path}", self.url))
            .header("cookie", &self.cookie)
            .header("x-spielstand-csrf", "1")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn base(&self, device: &Device, game: &str, version: &str) -> anyhow::Result<()> {
        let report = BaseReport {
            game: game.into(),
            version: version.into(),
        };
        self.http
            .post(format!("{}{}", self.url, paths::BASES))
            .bearer_auth(&device.token)
            .json(&report)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn presence(&self, device: &Device, game: Option<&str>) -> anyhow::Result<()> {
        self.http
            .post(format!("{}{}", self.url, paths::PRESENCE))
            .bearer_auth(&device.token)
            .json(&PresenceReport {
                game: game.map(str::to_string),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// An open WebSocket makes the device show up as online.
    async fn connect(&self, device: &Device) -> anyhow::Result<impl Sized + use<>> {
        let mut req = format!("{}{}", self.url.replacen("http", "ws", 1), paths::WS).into_client_request()?;
        req.headers_mut()
            .insert("authorization", format!("Bearer {}", device.token).parse()?);
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await?;
        let hello = serde_json::json!({ "type": "hello", "agentVersion": env!("CARGO_PKG_VERSION") });
        ws.send(Message::Text(hello.to_string().into())).await?;
        Ok(ws)
    }
}

fn version(
    dir: &Path,
    game: &str,
    content: &str,
    parent: Option<&str>,
    at: DateTime<Utc>,
    minutes: u64,
) -> anyhow::Result<(VersionMeta, std::path::PathBuf)> {
    let id = spielstand_core::new_version_id(at);
    let slug = spielstand_core::slug(game);
    let save = dir.join(format!("{id}.sav"));
    std::fs::write(&save, content.repeat(512))?;
    let settings = dir.join(format!("{id}.ini"));
    std::fs::write(&settings, format!("[{game}]\nvolume=80\n"))?;
    let blob = dir.join(format!("{id}.tar.zst"));
    let packed = pack::pack(
        &[
            Source {
                portable: format!("<root:steam>/userdata/{slug}/save.sav"),
                path: save,
            },
            Source {
                portable: format!("<home>/.config/{slug}/settings.ini"),
                path: settings,
            },
        ],
        &blob,
    )?;
    let meta = VersionMeta {
        id,
        game: game.into(),
        parent: parent.map(str::to_string),
        device: String::new(),
        created_at: at,
        trigger: Trigger::Session,
        session_seconds: Some(minutes * 60),
        content_hash: packed.content_hash,
        blob_sha256: packed.blob_sha256,
        blob_size: packed.blob_size,
        files: packed.files,
        prefix: None,
        title: None,
    };
    Ok((meta, blob))
}

/// Ctrl-C, or SIGTERM from whatever started this (Playwright, CI).
async fn stopped() {
    let mut term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
