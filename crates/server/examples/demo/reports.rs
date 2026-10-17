//! What the demo devices report about their games: where the saves are, how they were found,
//! and the cases the game page explains (another program still syncing, several prefixes).

use serde_json::{Value, json};

fn game(name: &str, location: &str, prefix: Option<&str>, source: &str) -> Value {
    json!({
        "game": name, "location": location, "prefix": prefix, "source": source,
        "effectiveMode": "sync", "foreignSync": null, "problem": null,
    })
}

const STEAM: &str = "/home/tristan/.local/share/Steam/steamapps/compatdata";

pub fn living_room() -> Value {
    let mut hollow = game(
        "Hollow Knight",
        "/home/tristan/Games/Hollow Knight/pfx/drive_c/users/steamuser/AppData/LocalLow/Team Cherry/Hollow Knight",
        Some("/home/tristan/Games/Hollow Knight/pfx"),
        "gameFolder",
    );
    hollow["effectiveMode"] = json!("backup");
    json!({
        "stellwerkAgent": true,
        "prefixr": true,
        "games": [
            game(
                "ELDEN RING",
                &format!("{STEAM}/1245620/pfx/drive_c/users/steamuser/AppData/Roaming/EldenRing/76561198000000000"),
                Some(&format!("{STEAM}/1245620/pfx")),
                "steam",
            ),
            game(
                "Hades II",
                "/home/tristan/Games/Hades II/prefix/drive_c/users/steamuser/AppData/Roaming/Hades II",
                Some("/home/tristan/Games/Hades II/prefix"),
                "prefixr",
            ),
            game("Stardew Valley", "/home/tristan/.config/StardewValley/Saves", None, "ludusavi"),
            game(
                "Baldur's Gate 3",
                &format!("{STEAM}/1086940/pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/PlayerProfiles"),
                Some(&format!("{STEAM}/1086940/pfx")),
                "steam",
            ),
            hollow,
        ],
    })
}

pub fn steam_deck() -> Value {
    let deck_steam = "/home/deck/.local/share/Steam/steamapps/compatdata";
    let mut hollow = game("Hollow Knight", "", None, "gameFolder");
    hollow["location"] = Value::Null;
    hollow["source"] = Value::Null;
    hollow["problem"] = json!({
        "kind": "severalPrefixes",
        "candidates": ["/home/deck/Games/Hollow Knight/pfx", "/run/media/deck/SD/Games/Hollow Knight/pfx"],
    });
    json!({
        "stellwerkAgent": false,
        "prefixr": true,
        "games": [
            game(
                "ELDEN RING",
                &format!("{deck_steam}/1245620/pfx/drive_c/users/steamuser/AppData/Roaming/EldenRing/76561198000000000"),
                Some(&format!("{deck_steam}/1245620/pfx")),
                "steam",
            ),
            game(
                "Hades II",
                "/run/media/deck/SD/Games/Hades II/prefix/drive_c/users/steamuser/AppData/Roaming/Hades II",
                Some("/run/media/deck/SD/Games/Hades II/prefix"),
                "prefixr",
            ),
            hollow,
        ],
    })
}

pub fn office() -> Value {
    let mut stardew = game(
        "Stardew Valley",
        "/home/tristan/Stellwerk/Spiele/StardewValley/Saves",
        None,
        "ludusavi",
    );
    stardew["effectiveMode"] = json!("backup");
    stardew["foreignSync"] = json!({
        "tool": "stellwerk", "folder": "/home/tristan/Stellwerk/Spiele", "excluded": false,
    });
    json!({
        "stellwerkAgent": true,
        "prefixr": false,
        "games": [
            stardew,
            game(
                "Baldur's Gate 3",
                &format!("{STEAM}/1086940/pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/PlayerProfiles"),
                Some(&format!("{STEAM}/1086940/pfx")),
                "steam",
            ),
        ],
    })
}
