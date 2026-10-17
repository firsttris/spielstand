//! HTTP routes shared by server and agent, so a renamed route breaks the build, not the wire.

pub const PAIR: &str = "/api/agent/pair";
pub const HEADS: &str = "/api/agent/heads";
pub const VERSIONS: &str = "/api/agent/versions";
pub const BASES: &str = "/api/agent/bases";
pub const WS: &str = "/api/agent/ws";
pub const PRESENCE: &str = "/api/agent/presence";

pub fn version_blob(id: &str) -> String {
    format!("{VERSIONS}/{id}/blob")
}

pub fn version_promote(id: &str) -> String {
    format!("{VERSIONS}/{id}/promote")
}
