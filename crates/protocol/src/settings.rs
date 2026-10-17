//! Modes, device settings and what a device reports about its games: who moves the saves,
//! where they live on each device, and why a device is not in step.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

shared! {
    /// Who moves a game's saves between devices.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum SaveMode {
        /// Spielstand loads the newest state on every device.
        #[default]
        Sync,
        /// Spielstand only backs up; another tool (e.g. Stellwerk) or nobody moves the saves.
        Backup,
    }

    /// What an agent needs to know from the server; sent on connect and whenever it changes.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AgentSettings {
        pub default_mode: SaveMode,
        /// Games whose mode differs from the default.
        pub game_modes: BTreeMap<String, SaveMode>,
        /// Folders on this device whose sub-folders hold games with their own Wine prefix.
        pub game_folders: Vec<String>,
        /// Game → the prefix chosen in the web UI when several were found on this device.
        pub prefix_choices: BTreeMap<String, String>,
    }

    /// "Is this content already a version, and is it newer than what this device had?"
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct KnownRequest {
        pub game: String,
        pub content_hash: String,
        pub base: Option<String>,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct KnownResponse {
        pub version: Option<String>,
        pub descends_from_base: bool,
        pub is_open_branch: bool,
    }

    /// The disk of this device went from `lost` to the older or unrelated version `found`.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct OverwriteReport {
        pub game: String,
        pub lost: String,
        pub found: String,
    }

    /// Another program that synchronises the folder a save lies in.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ForeignSync {
        /// `stellwerk` when its agent runs on the device, else `syncthing`.
        pub tool: String,
        /// Root of the synchronised folder.
        pub folder: String,
        /// Every save file of the game is excluded from that sync.
        pub excluded: bool,
    }

    /// Why a device is not in step for a game, in words the web UI can show with an action.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    pub enum GameProblem {
        /// No Wine prefix for the game was found on this device.
        NoPrefix,
        /// Several prefixes could hold the game; one has to be chosen.
        SeveralPrefixes { candidates: Vec<String> },
        /// Loading failed; the message says why.
        Failed { message: String },
        /// Several emulators on this device could hold a Switch game's save; one has to be
        /// chosen. The candidates are the title's save folders.
        SeveralEmulators { candidates: Vec<String> },
        /// The emulator only uses saves it created itself (Ryujinx): the game has to be started
        /// there once before a save can be loaded.
        StartInEmulator { emulator: String },
    }

    /// What a device knows about one game.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameReport {
        pub game: String,
        /// The folder the saves are in, as shown to the user.
        pub location: Option<String>,
        /// The game's Wine prefix on this device.
        pub prefix: Option<String>,
        /// How the saves were found: `steam`, `prefixr`, `gameFolder`, `heroic`, `lutris`,
        /// `ludusavi`, or the emulator's name (`Eden`, `Ryujinx`, …) for a Switch game.
        pub source: Option<String>,
        /// The mode in effect here; `backup` while another tool still syncs the saves.
        pub effective_mode: SaveMode,
        pub foreign_sync: Option<ForeignSync>,
        pub problem: Option<GameProblem>,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DeviceReport {
        /// The Stellwerk agent is installed here (its exclusions are handled automatically).
        pub stellwerk_agent: bool,
        /// Prefixr's configuration was found here.
        pub prefixr: bool,
        pub games: Vec<GameReport>,
    }

    /// A save that was replaced by an older or unrelated one on a device.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Alert {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        pub id: i64,
        pub game: String,
        pub device_id: String,
        pub device_name: String,
        pub lost_version: String,
        pub lost_at: Option<DateTime<Utc>>,
        pub found_version: String,
        pub created_at: DateTime<Utc>,
    }

    /// One device's view of a game, for the game page.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameLocation {
        pub device_id: String,
        pub device_name: String,
        pub online: bool,
        /// The version this device's disk holds.
        pub version: Option<String>,
        pub is_current: bool,
        pub report: Option<GameReport>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ServerSettings {
        pub default_mode: SaveMode,
    }

    /// `None` follows the default mode.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GameModeRequest {
        pub mode: Option<SaveMode>,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DeviceSettings {
        pub game_folders: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PrefixChoice {
        pub device: String,
        pub prefix: String,
    }
}
