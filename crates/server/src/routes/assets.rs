//! `install.sh`, the agent binaries, and the built web UI (embedded into the binary).

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use rust_embed::Embed;
use tokio_util::io::ReaderStream;

use crate::{app::AppState, routes::ui::server_url};

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
struct Web;

const INSTALL_SCRIPT: &str = include_str!("../../../../install/install.sh");
const RELEASES: &str = "https://github.com/firsttris/spielstand/releases/download";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/install.sh", get(install))
        .route("/download/agent/{arch}", get(agent))
        .fallback(web)
}

pub fn render_install(server: &str) -> String {
    INSTALL_SCRIPT
        .replace("__SPIELSTAND_SERVER__", server)
        .replace("__SPIELSTAND_VERSION__", env!("CARGO_PKG_VERSION"))
}

async fn install(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let script = render_install(&server_url(&state, &headers));
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        script,
    )
        .into_response()
}

fn agent_file_name(arch: &str) -> Option<String> {
    matches!(arch, "x86_64" | "aarch64").then(|| format!("spielstand-agent-{arch}-linux"))
}

/// Serves the binary shipped in the image; without one, sends to the matching GitHub release.
async fn agent(State(state): State<AppState>, Path(arch): Path<String>) -> Response {
    let Some(name) = agent_file_name(&arch) else {
        return (StatusCode::NOT_FOUND, "unknown architecture").into_response();
    };
    let path = state.settings.agent_dir.join(&name);
    match tokio::fs::File::open(&path).await {
        Ok(file) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            Body::from_stream(ReaderStream::new(file)),
        )
            .into_response(),
        Err(_) => {
            Redirect::temporary(&format!("{RELEASES}/v{}/{name}", env!("CARGO_PKG_VERSION"))).into_response()
        }
    }
}

async fn web(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let file = Web::get(path)
        .map(|f| (path.to_string(), f))
        .or_else(|| Web::get("index.html").map(|f| ("index.html".into(), f)));
    match file {
        Some((name, content)) => {
            let mime = mime_guess::from_path(&name).first_or_octet_stream();
            let cache = if name.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    (header::CACHE_CONTROL, cache.to_string()),
                ],
                content.data,
            )
                .into_response()
        }
        None => (
            StatusCode::OK,
            "Spielstand server is running; the web UI was not built into this binary.",
        )
            .into_response(),
    }
}
