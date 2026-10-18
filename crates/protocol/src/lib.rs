//! Types shared by the Spielstand server, the agent and the web UI.
//!
//! The web UI gets them as TypeScript through `ts-rs` (feature `ts`):
//! `cargo test -p spielstand-protocol --features ts` writes `web/src/bindings/`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod paths;
pub mod refusals;

macro_rules! shared {
    ($($item:item)*) => {
        $(
            #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
            $item
        )*
    };
}

mod settings;
mod ui;
pub use settings::*;
pub use ui::*;

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
        /// Where the Wine prefix of files stored as `<prefix>/…` was, as a portable path
        /// (e.g. `<root:games>/Hades/prefix`): a hint for finding the prefix on another device.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub prefix: Option<String>,
        /// The game's name when its id is not one (`Switch 0100F2C0115B6000`), as the device's
        /// emulator knows it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,
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
        /// The settings for this device changed.
        Settings { settings: AgentSettings },
        Pong,
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
            Self::Overwritten => "overwritten",
            Self::ModeChanged => "modeChanged",
            Self::CustomGameChanged => "customGameChanged",
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
