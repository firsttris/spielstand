//! Types shared by the Spielstand server, the agent and the web UI.
//!
//! The web UI gets them as TypeScript through `ts-rs` (feature `ts`):
//! `cargo test -p spielstand-protocol --features ts` writes `web/src/bindings/`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod paths;

macro_rules! shared {
    ($($item:item)*) => {
        $(
            #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
            $item
        )*
    };
}

shared! {
    /// Why a version was made. Shown as the label in the timeline.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum Trigger {
        /// After a play session ended.
        Session,
        /// During a long session, once the save files were quiet for a while.
        Interim,
        /// Taken right before another version was written to the disk.
        BeforeRestore,
        /// Found on start-up: the disk changed while nothing was watching.
        CatchUp,
        /// Asked for by the user.
        Manual,
        /// Taken over from a Ludusavi backup.
        Import,
    }

    /// One save file inside a version. `path` is portable (see `spielstand_core::portable`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FileEntry {
        pub path: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub size: u64,
        pub sha256: String,
    }

    /// Everything about a version except its packed bytes.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VersionMeta {
        pub id: String,
        pub game: String,
        /// The version this state grew from; `None` for the first version of a game.
        pub parent: Option<String>,
        pub device: String,
        pub created_at: DateTime<Utc>,
        pub trigger: Trigger,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        pub session_seconds: Option<u64>,
        /// Hash over the sorted list of files and their hashes; equal states have equal hashes.
        pub content_hash: String,
        /// SHA-256 of the packed `.tar.zst`.
        pub blob_sha256: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub blob_size: u64,
        pub files: Vec<FileEntry>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct HeadSummary {
        pub id: String,
        pub content_hash: String,
        pub device: String,
        pub created_at: DateTime<Utc>,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct HeadsResponse {
        pub heads: BTreeMap<String, HeadSummary>,
        /// Versions this device uploaded that the server keeps as unresolved branches.
        pub open_branches: Vec<String>,
        /// Game → name of another device that is playing it right now.
        pub playing_elsewhere: BTreeMap<String, String>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum UploadStatus {
        /// The parent was the head, so this version is the new head.
        Head,
        /// The head had moved on; kept as a branch, a conflict is open.
        Branch,
        /// The server already had this version (retried upload).
        Known,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct UploadOutcome {
        pub status: UploadStatus,
        pub head: Option<String>,
    }

    /// Make a branch the head, but only if the head is still `expected_head`.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PromoteRequest {
        pub expected_head: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PromoteOutcome {
        pub promoted: bool,
        pub head: Option<String>,
    }

    /// The agent tells the server which version the disk of this device now holds.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BaseReport {
        pub game: String,
        pub version: String,
    }

    /// The start wrapper says a game started (`Some`) or ended (`None`) on this device.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PresenceReport {
        pub game: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PairRequest {
        pub code: String,
        pub name: String,
        pub hostname: String,
        pub os: String,
        pub agent_version: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PairResponse {
        pub device_id: String,
        pub token: String,
    }

    /// Agent → server over the live connection.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    pub enum AgentMessage {
        #[serde(rename_all = "camelCase")]
        Hello { agent_version: String },
        Playing { game: String },
        Stopped { game: String },
        Ping,
    }

    /// Server → agent over the live connection.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    pub enum ServerMessage {
        /// A game has a new head; the agent may fetch it ahead of the next start.
        NewHead { game: String, version: String, device: String },
        /// Someone asked in the web UI to load this version on this device.
        Restore { game: String, version: String },
        /// Someone asked in the web UI for a backup of this game now.
        BackUp { game: String },
        Pong,
    }

    // ---- web UI ----

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SessionInfo {
        pub setup_required: bool,
        pub logged_in: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PasswordRequest {
        pub password: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Device {
        pub id: String,
        pub name: String,
        pub hostname: String,
        pub os: String,
        pub agent_version: String,
        pub online: bool,
        pub last_seen: Option<DateTime<Utc>>,
        pub playing: Option<String>,
        pub playing_since: Option<DateTime<Utc>>,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub pending_conflicts: u64,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameSummary {
        pub id: String,
        pub head: Option<HeadSummary>,
        pub head_device_name: Option<String>,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub versions: u64,
        pub conflict: bool,
        pub playing_on: Option<String>,
        /// Devices whose disk holds an older version than the head.
        pub behind: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VersionRow {
        pub meta: VersionMeta,
        pub device_name: String,
        pub is_head: bool,
        pub is_branch: bool,
        pub pinned: bool,
        pub label: Option<String>,
        /// Names of the devices whose disk holds this version.
        pub on_devices: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameDetail {
        pub id: String,
        pub head: Option<String>,
        pub versions: Vec<VersionRow>,
        pub conflict: bool,
        pub playing_on: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VersionPatch {
        pub pinned: Option<bool>,
        pub label: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RestoreRequest {
        pub device: String,
        pub version: String,
    }

    /// Choose the version to continue with after a game was played on two devices.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ResolveRequest {
        pub version: String,
    }

    /// What happened, stored as its name; the web UI says it in the reader's language.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum EventKind {
        Uploaded,
        Branched,
        KeptOwn,
        Resolved,
        RestoreRequested,
        Started,
        Stopped,
        Paired,
        Removed,
        Imported,
        /// An event this version does not know (written by a newer one).
        #[serde(other)]
        Other,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Activity {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub id: i64,
        pub at: DateTime<Utc>,
        pub device_name: Option<String>,
        pub game: Option<String>,
        pub kind: EventKind,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PairCode {
        pub code: String,
        pub expires_at: DateTime<Utc>,
        pub command: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct MirrorStatus {
        pub enabled: bool,
        pub remote: Option<String>,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub pending: u64,
        pub last_ok: Option<DateTime<Utc>>,
        pub last_error: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Overview {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub games: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub versions: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub stored_bytes: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub conflicts: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub devices: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub devices_online: u64,
        pub last_version: Option<HeadSummary>,
        pub last_version_game: Option<String>,
        pub last_version_device: Option<String>,
        pub mirror: MirrorStatus,
        pub version: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ImportRequest {
        /// A Ludusavi backup folder on the server, or `remote:path` for rclone.
        pub source: String,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ImportReport {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub games: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub versions: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub skipped: u64,
        pub errors: Vec<String>,
    }
}

/// Header that carries the JSON `VersionMeta` next to the packed body of an upload.
pub const META_HEADER: &str = "x-spielstand-meta";

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::Branched => "branched",
            Self::KeptOwn => "keptOwn",
            Self::Resolved => "resolved",
            Self::RestoreRequested => "restoreRequested",
            Self::Started => "started",
            Self::Stopped => "stopped",
            Self::Paired => "paired",
            Self::Removed => "removed",
            Self::Imported => "imported",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Self {
        serde_json::from_value(serde_json::Value::String(text.into())).unwrap_or(Self::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_messages_are_tagged() {
        let json = serde_json::to_string(&AgentMessage::Playing {
            game: "Elden Ring".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"playing","game":"Elden Ring"}"#);
        let back: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back,
            AgentMessage::Playing {
                game: "Elden Ring".into()
            }
        );
    }
}
