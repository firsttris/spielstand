//! What the web UI reads and sends: sessions, devices, games, versions, activity, overview.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Alert, GameLocation, HeadSummary, SaveMode, VersionMeta};

shared! {

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
        pub game_folders: Vec<String>,
        pub stellwerk_agent: bool,
        pub prefixr: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameSummary {
        pub id: String,
        /// The name to show when the id is not one (Switch games are keyed by title id).
        pub title: Option<String>,
        pub head: Option<HeadSummary>,
        pub head_device_name: Option<String>,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub versions: u64,
        pub conflict: bool,
        pub playing_on: Option<String>,
        /// Devices whose disk holds an older version than the head.
        pub behind: Vec<String>,
        /// An open alert: a newer save was replaced by an older one on some device.
        pub overwritten: bool,
        pub mode: SaveMode,
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
        pub title: Option<String>,
        pub head: Option<String>,
        pub versions: Vec<VersionRow>,
        pub conflict: bool,
        pub playing_on: Option<String>,
        pub mode: SaveMode,
        /// The mode is the server's default, not set for this game.
        pub mode_is_default: bool,
        pub locations: Vec<GameLocation>,
        pub alerts: Vec<Alert>,
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
        /// Another tool replaced a newer save with an older one on a device.
        Overwritten,
        ModeChanged,
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
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub alerts: u64,
        pub default_mode: SaveMode,
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
