use std::sync::{Arc, Mutex};

use axum::{Router, extract::DefaultBodyLimit};
use spielstand_protocol::MirrorStatus;
use tokio::sync::Notify;
use tower_http::trace::TraceLayer;

use crate::{db::Db, hub::Hub, routes, settings::Settings, throttle::Throttle};

/// Packages above this size are refused (saves are small; this guards the disk).
pub const MAX_PACKAGE: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub settings: Arc<Settings>,
    pub hub: Hub,
    /// Poked after every new version so the mirror copies it soon.
    pub mirror_wake: Arc<Notify>,
    pub mirror_status: Arc<Mutex<MirrorStatus>>,
    pub logins: Throttle,
    pub pairings: Throttle,
}

impl AppState {
    pub async fn open(settings: Settings) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&settings.data_dir).await?;
        tokio::fs::create_dir_all(settings.blobs_dir()).await?;
        let db = crate::db::open(&settings.db_path()).await?;
        let mirror_status = MirrorStatus {
            enabled: settings.mirror.is_some(),
            remote: settings.mirror.clone(),
            pending: 0,
            last_ok: None,
            last_error: None,
        };
        Ok(Self {
            db,
            settings: Arc::new(settings),
            hub: Hub::default(),
            mirror_wake: Arc::new(Notify::new()),
            mirror_status: Arc::new(Mutex::new(mirror_status)),
            logins: Throttle::new(10, std::time::Duration::from_secs(60)),
            pairings: Throttle::new(20, std::time::Duration::from_secs(60)),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(routes::agent::router())
        .merge(routes::ui::router())
        .merge(routes::settings::router())
        .merge(routes::assets::router())
        .layer(DefaultBodyLimit::disable())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
