//! The Spielstand server: the one place that knows the current state (head) of every game.

pub mod alerts;
pub mod app;
pub mod auth;
pub mod blobs;
pub mod custom_games;
pub mod db;
pub mod error;
pub mod framing;
pub mod gc;
pub mod hub;
pub mod importer;
pub mod known;
pub mod mirror;
pub mod modes;
pub mod presence;
pub mod queries;
pub mod reports;
pub mod routes;
pub mod settings;
pub mod switch_titles;
pub mod throttle;
pub mod versions;

pub use app::{AppState, router};
pub use settings::Settings;

/// Runs the HTTP server with its background jobs until the process ends.
pub async fn serve(settings: Settings) -> anyhow::Result<()> {
    let listen = settings.listen.clone();
    let state = AppState::open(settings).await?;
    tokio::spawn(mirror::run(state.clone()));
    tokio::spawn(gc::run(state.clone()));
    tokio::spawn(switch_titles::run(state.clone()));
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("Spielstand server {} on {listen}", env!("CARGO_PKG_VERSION"));
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal");
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
