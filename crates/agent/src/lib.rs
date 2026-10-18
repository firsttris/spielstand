//! The Spielstand agent: one per PC. Finds saves with Ludusavi, snapshots before every write,
//! uploads to the server and, for games it synchronises, loads newer states from other PCs.

pub mod agent;
pub mod choices;
pub mod client;
pub mod config;
pub mod custom;
pub mod daemon;
pub mod desktop;
pub mod disk;
pub mod emulators;
pub mod engine;
pub mod foreign;
pub mod inspect;
pub mod prefixes;
pub mod report;
pub mod running;
pub mod settings;
pub mod skill;
pub mod store;
pub mod sync;
pub mod text;
pub mod watch;

use std::sync::Arc;

pub use agent::Agent;
pub use sync::{Choice, Mode, Outcome, Silent, Ui};

/// Builds an agent from its saved configuration.
pub async fn open(
    dirs: config::Dirs,
    scanner: Arc<dyn engine::Scanner>,
    ui: Arc<dyn Ui>,
) -> anyhow::Result<Agent> {
    let config = config::AgentConfig::load(&dirs)?;
    let store = store::Store::open(&dirs.db()).await?;
    let client = client::Client::new(&config.server, &config.token);
    Ok(Agent::new(
        dirs,
        config,
        store,
        client,
        scanner,
        ui,
        settings::Sources::from_env(),
    ))
}
