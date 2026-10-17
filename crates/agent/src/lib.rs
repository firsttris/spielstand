//! The Spielstand agent: one per PC. Finds saves with Ludusavi, snapshots before every write,
//! uploads to the server and loads newer states from other PCs.

pub mod client;
pub mod config;
pub mod daemon;
pub mod desktop;
pub mod disk;
pub mod engine;
pub mod running;
pub mod store;
pub mod sync;

use std::sync::Arc;

pub use sync::{Agent, Choice, Mode, Outcome, Silent, Ui};

/// Builds an agent from its saved configuration.
pub async fn open(
    dirs: config::Dirs,
    scanner: Arc<dyn engine::Scanner>,
    ui: Arc<dyn Ui>,
) -> anyhow::Result<Agent> {
    let config = config::AgentConfig::load(&dirs)?;
    let store = store::Store::open(&dirs.db()).await?;
    let client = client::Client::new(&config.server, &config.token);
    Ok(Agent {
        dirs,
        config,
        store,
        client,
        scanner,
        ui,
    })
}
