//! HTTP routes shared by server and agent, so a renamed route breaks the build, not the wire.

pub const PAIR: &str = "/api/agent/pair";
pub const HEADS: &str = "/api/agent/heads";
pub const VERSIONS: &str = "/api/agent/versions";
pub const BASES: &str = "/api/agent/bases";
pub const WS: &str = "/api/agent/ws";
pub const PRESENCE: &str = "/api/agent/presence";
pub const KNOWN: &str = "/api/agent/known";
pub const OVERWRITES: &str = "/api/agent/overwrites";
pub const SETTINGS: &str = "/api/agent/settings";
pub const REPORT: &str = "/api/agent/report";
pub const CUSTOM_GAMES: &str = "/api/agent/custom-games";
pub const BACKUP_ONLY: &str = "/api/agent/backup-only";
pub const PREFIX: &str = "/api/agent/prefix";

pub fn version_blob(id: &str) -> String {
    format!("{VERSIONS}/{id}/blob")
}

pub fn version_promote(id: &str) -> String {
    format!("{VERSIONS}/{id}/promote")
}

pub fn custom_game(name: &str) -> String {
    format!("{CUSTOM_GAMES}/{}", encode(name))
}

/// Percent-encodes a path segment (a game name may hold spaces, `/` or `?`).
pub fn encode(segment: &str) -> String {
    segment
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
