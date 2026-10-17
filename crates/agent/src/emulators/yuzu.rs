//! The yuzu family (yuzu, Eden, Citron, Suyu, Sudachi): one save folder per profile and title,
//! `<save base>/user/save/0000000000000000/<profile>/<title id>/`, used as it is.
//!
//! Layouts from the emulators' sources: `src/core/file_sys/savedata_factory.cpp` (GetFullPath),
//! `src/core/hle/service/acc/profile_manager.cpp` (profiles.dat), `src/common/fs/path_util.cpp`
//! (folders), `src/yuzu/game_list_worker.cpp` (name cache).

use std::path::{Path, PathBuf};

use super::{Install, Kind, ini_value, is_title_id, list_files};

/// Profiles live in this system save, always under the NAND folder.
const PROFILES: &str = "system/save/8000000000000010/su/avators/profiles.dat";
const ENTRIES_AT: usize = 0x10;
const ENTRY_SIZE: usize = 0xC8;
const MAX_USERS: usize = 8;

/// Files the emulator keeps in a save folder for itself, not part of the game's save.
pub const OWN_FILES: &[&str] = &[
    ".yuzu_save_size",
    ".citron_save_size",
    "ExtraData0",
    "ExtraData1",
    "ExtraData_",
];

/// The UUIDs in a `profiles.dat`, in their order; empty slots (all zero) are left out.
pub fn profiles(data: &[u8]) -> Vec<[u8; 16]> {
    (0..MAX_USERS)
        .filter_map(|i| {
            let at = ENTRIES_AT + i * ENTRY_SIZE;
            let uuid: [u8; 16] = data.get(at..at + 16)?.try_into().ok()?;
            (uuid != [0; 16]).then_some(uuid)
        })
        .collect()
}

/// The profile's folder name: the u128 the UUID's bytes make, as 32 upper-case hex digits
/// (`{:016X}{:016X}` of its high and low half), i.e. the bytes in reverse order.
pub fn profile_folder(uuid: &[u8; 16]) -> String {
    uuid.iter().rev().map(|b| format!("{b:02X}")).collect()
}

/// A value from `qt-config.ini`, unless it is marked as the default.
fn setting(ini: &str, group: &str, key: &str) -> Option<String> {
    if ini_value(ini, group, &format!("{key}\\default")).as_deref() == Some("true") {
        return None;
    }
    ini_value(ini, group, key).filter(|v| !v.is_empty())
}

fn nand_dir(install: &Install, ini: &str) -> PathBuf {
    setting(ini, "Data%20Storage", "nand_directory")
        .map(PathBuf::from)
        .unwrap_or_else(|| install.data.join("nand"))
}

/// The folder that holds the profile folders of `install`'s current profile, and its name.
pub fn user_saves(install: &Install) -> Option<PathBuf> {
    let ini = std::fs::read_to_string(install.config.join("qt-config.ini")).unwrap_or_default();
    let nand = nand_dir(install, &ini);
    let users = profiles(&std::fs::read(nand.join(PROFILES)).ok()?);
    let current: usize = setting(&ini, "System", "current_user")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let uuid = users.get(current).or(users.first())?;
    // Eden can keep saves apart from the NAND (`save_directory`); profiles stay in the NAND.
    let base = match install.kind {
        Kind::Eden => setting(&ini, "Data%20Storage", "save_directory")
            .map(PathBuf::from)
            .unwrap_or_else(|| nand.clone()),
        _ => nand,
    };
    Some(base.join("user/save/0000000000000000").join(profile_folder(uuid)))
}

/// The title ids with saves of the current profile.
pub fn titles(install: &Install) -> Vec<String> {
    let Some(dir) = user_saves(install) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_title_id(n))
        .filter(|n| !save_files(install, n).is_empty())
        .collect();
    ids.sort();
    ids
}

/// Where the current profile's save of `title` is, or goes: yuzu opens it by path, so a folder
/// written before the game first starts is used.
pub fn save_dir(install: &Install, title: &str) -> Option<PathBuf> {
    user_saves(install).map(|d| d.join(title))
}

pub fn save_files(install: &Install, title: &str) -> Vec<PathBuf> {
    save_dir(install, title)
        .map(|d| list_files(&d, OWN_FILES))
        .unwrap_or_default()
}

/// The name the emulator's game list cached (`game_list/<title id>.appname.txt`), or Citron's
/// `game_metadata_cache.json`.
pub fn name(install: &Install, title: &str) -> Option<String> {
    let cache = install.cache.join("game_list");
    if let Ok(text) = std::fs::read_to_string(cache.join(format!("{title}.appname.txt"))) {
        let name = text.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cache.join("game_metadata_cache.json")).ok()?).ok()?;
    let wanted = u64::from_str_radix(title, 16).ok()?;
    json["entries"].as_array()?.iter().find_map(|e| {
        let id = u64::from_str_radix(e["program_id"].as_str()?, 16).ok()?;
        (id == wanted)
            .then(|| e["title"].as_str().map(str::to_string))
            .flatten()
    })
}

/// Whether `dir` is where this install keeps its data.
pub fn is_install(data: &Path) -> bool {
    data.join("nand").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `profiles.dat` as the emulator writes it: 0x10 bytes header, 8 entries of 0xC8 bytes.
    pub fn profiles_dat(uuids: &[[u8; 16]]) -> Vec<u8> {
        let mut data = vec![0u8; ENTRIES_AT + MAX_USERS * ENTRY_SIZE];
        for (i, uuid) in uuids.iter().enumerate() {
            let at = ENTRIES_AT + i * ENTRY_SIZE;
            data[at..at + 16].copy_from_slice(uuid);
            data[at + 16..at + 32].copy_from_slice(uuid);
            data[at + 0x28..at + 0x2C].copy_from_slice(b"Eden");
        }
        data
    }

    #[test]
    fn profiles_are_read_from_their_slots() {
        let a = [1u8; 16];
        let mut b = [0u8; 16];
        b[15] = 7;
        let mut data = profiles_dat(&[a, [0; 16], b]);
        assert_eq!(data.len(), 0x650);
        assert_eq!(profiles(&data), vec![a, b]);
        data.truncate(0x20);
        assert_eq!(profiles(&data), vec![a], "a short file reads what it has");
    }

    #[test]
    fn the_profile_folder_is_the_uuid_reversed_in_upper_case() {
        let uuid: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
        ];
        assert_eq!(profile_folder(&uuid), "FEDCBA9876543210EFCDAB8967452301");
    }
}
