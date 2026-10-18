//! Why the server refuses a device's request about custom games, word for word, so the agent
//! can say it in its own words.

pub const EVERY_DEVICE: &str = "this custom game is in effect on every device; change it in the web UI";
pub const OTHER_DEVICE: &str = "another device proposed a custom game of this name";
pub const KNOWN_GAME: &str =
    "a device proposes only games that are not known yet; places for a known game are added in the web UI";
