//! Ryujinx (and Ryubing, which keeps its layout): saves are numbered folders,
//! `bis/user/save/<save id>/`, with the committed state in `0`. Which title and profile a folder
//! belongs to is in its `ExtraData0`, which Ryujinx rewrites from its save index at every start
//! (`Ryujinx.HLE/FileSystem/VirtualFileSystem.cs`, FixExtraData). A folder Ryujinx did not create
//! is not in that index and would be ignored, so saves are only ever written into folders that
//! already exist: the game has to be started once in Ryujinx.

use std::path::{Path, PathBuf};

use super::{Install, list_files};

/// `SaveDataType::Account`: a profile's save of a game.
const ACCOUNT: u8 = 1;
/// The save data attribute at the start of `ExtraData0`: program id (u64 LE) at 0, user id
/// (16 bytes) at 8, save type at 0x20.
const ATTRIBUTE_LEN: usize = 0x21;

/// The current profile (`system/Profiles.json`, `last_opened`) as the 16 bytes a save's user id
/// holds: its two halves as little-endian u64s, high half first (`UserId.Write`).
pub fn current_user(install: &Install) -> Option<[u8; 16]> {
    let text = std::fs::read_to_string(install.data.join("system/Profiles.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = json["last_opened"]
        .as_str()
        .or_else(|| json["profiles"].get(0)?["user_id"].as_str())?;
    user_bytes(id)
}

/// `user_id` text (32 hex digits, high half first) → the bytes in a save's attribute.
pub fn user_bytes(id: &str) -> Option<[u8; 16]> {
    if id.len() != 32 {
        return None;
    }
    let high = u64::from_str_radix(&id[..16], 16).ok()?;
    let low = u64::from_str_radix(&id[16..], 16).ok()?;
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&high.to_le_bytes());
    out[8..].copy_from_slice(&low.to_le_bytes());
    Some(out)
}

/// Title id (16 upper-case hex digits) of an account save of `user`, from its `ExtraData0`.
pub fn account_title(extra: &[u8], user: &[u8; 16]) -> Option<String> {
    let attr = extra.get(..ATTRIBUTE_LEN)?;
    let program = u64::from_le_bytes(attr[..8].try_into().ok()?);
    (attr[0x20] == ACCOUNT && &attr[8..24] == user && program != 0).then(|| format!("{program:016X}"))
}

/// Every save folder of the current profile: (title id, its `0` folder).
fn saves(install: &Install) -> Vec<(String, PathBuf)> {
    let Some(user) = current_user(install) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(install.data.join("bis/user/save")) else {
        return vec![];
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let extra = std::fs::read(e.path().join("ExtraData0")).ok()?;
            let title = account_title(&extra, &user)?;
            let committed = e.path().join("0");
            committed.is_dir().then_some((title, committed))
        })
        .collect();
    found.sort();
    found
}

pub fn titles(install: &Install) -> Vec<String> {
    let mut ids: Vec<String> = saves(install)
        .into_iter()
        .filter(|(_, dir)| !list_files(dir, &[]).is_empty())
        .map(|(t, _)| t)
        .collect();
    ids.dedup();
    ids
}

/// The folder Ryujinx made for the current profile's save of `title`, if it made one.
pub fn save_dir(install: &Install, title: &str) -> Option<PathBuf> {
    saves(install)
        .into_iter()
        .find(|(t, _)| t == title)
        .map(|(_, dir)| dir)
}

pub fn save_files(install: &Install, title: &str) -> Vec<PathBuf> {
    save_dir(install, title)
        .map(|d| list_files(&d, &[]))
        .unwrap_or_default()
}

/// The name in Ryujinx's game library (`games/<title id>/gui/metadata.json`).
pub fn name(install: &Install, title: &str) -> Option<String> {
    let path = install
        .data
        .join("games")
        .join(title.to_lowercase())
        .join("gui/metadata.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    json["title"]
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

pub fn is_install(data: &Path) -> bool {
    data.join("system/Profiles.json").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_profile_id_becomes_the_bytes_of_a_save() {
        // RyuPlayer, the profile every fresh Ryujinx has.
        let bytes = user_bytes("00000000000000010000000000000000").unwrap();
        assert_eq!(bytes, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(user_bytes("short"), None);
    }

    #[test]
    fn extra_data_names_title_profile_and_type() {
        let user = user_bytes("00000000000000010000000000000000").unwrap();
        let mut extra = vec![0u8; 0x200];
        extra[..8].copy_from_slice(&0x0100_F2C0_115B_6000u64.to_le_bytes());
        extra[8..24].copy_from_slice(&user);
        extra[0x20] = ACCOUNT;
        assert_eq!(account_title(&extra, &user).as_deref(), Some("0100F2C0115B6000"));
        let other = user_bytes("00000000000000020000000000000000").unwrap();
        assert_eq!(account_title(&extra, &other), None, "another profile's save");
        extra[0x20] = 3;
        assert_eq!(account_title(&extra, &user), None, "a device save");
        assert_eq!(account_title(&extra[..0x10], &user), None);
    }
}
