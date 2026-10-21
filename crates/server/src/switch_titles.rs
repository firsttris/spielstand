//! Names of Switch titles that no emulator gave: Eden and the rest of the yuzu family cache a name
//! (`game_list/<title id>.appname.txt`) only for games installed to the NAND, not for games in a
//! games folder (Eden `src/yuzu/game/game_list_worker.cpp`, `GetMetadataFromControlNCA`). The names
//! come from blawar's titledb, downloaded at most once a week and kept small in the data folder.

use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Arc, RwLock},
    time::Duration,
};

use futures::StreamExt;
use serde::Deserialize;
use spielstand_core::switch_title_id;
use tokio::io::AsyncWriteExt;

use crate::app::AppState;

/// Upper-case title id → name, of base titles only (saves belong to them, not to updates or DLC).
pub type SwitchTitles = Arc<RwLock<BTreeMap<String, String>>>;

const CACHE: &str = "switch-titles.json";
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
const RETRY: Duration = Duration::from_secs(6 * 3600);

/// The name of a game keyed `Switch <title id>`, if the list knows it.
pub fn name_of(titles: &SwitchTitles, game: &str) -> Option<String> {
    let id = switch_title_id(game)?.to_ascii_uppercase();
    titles.read().unwrap().get(&id).cloned()
}

#[derive(Deserialize)]
struct Entry {
    id: Option<String>,
    name: Option<String>,
}

/// The base titles of a titledb file (`US.en.json`: eShop id → entry with `id` and `name`).
pub fn parse(reader: impl Read) -> anyhow::Result<BTreeMap<String, String>> {
    // Sorted by eShop id, so a title listed twice always gets the same name.
    let entries: BTreeMap<String, Entry> = serde_json::from_reader(reader)?;
    let mut titles = BTreeMap::new();
    for entry in entries.into_values() {
        let (Some(id), Some(name)) = (entry.id, entry.name) else {
            continue;
        };
        let (id, name) = (id.to_ascii_uppercase(), clean(&name));
        if is_base_title(&id) && !name.is_empty() {
            titles.entry(id).or_insert(name);
        }
    }
    Ok(titles)
}

/// Updates end in `800`, DLC count up from `…001`; the application itself ends in `000`.
fn is_base_title(id: &str) -> bool {
    switch_title_id(&format!("Switch {id}")).is_some() && id.ends_with("000")
}

/// The eShop's names carry trademark signs the emulators' names mostly lack.
fn clean(name: &str) -> String {
    name.replace(['™', '®', '©'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Loads the cached list, then refreshes it when it is older than a week.
pub async fn run(state: AppState) {
    if state.settings.switch_titles.is_empty() {
        return;
    }
    let cache = state.settings.data_dir.join(CACHE);
    if let Ok(titles) = read_cache(&cache).await {
        *state.switch_titles.write().unwrap() = titles;
    }
    loop {
        if !is_fresh(&cache).await {
            match refresh(&state).await {
                Ok(n) => tracing::info!("switch titles: {n} names"),
                Err(e) => tracing::warn!("switch titles: {e:#}"),
            }
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn read_cache(path: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    Ok(serde_json::from_slice(&tokio::fs::read(path).await?)?)
}

async fn is_fresh(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < MAX_AGE)
}

/// Fetches the list from `SPIELSTAND_SWITCH_TITLES`, keeps it as the cache and in memory.
pub async fn refresh(state: &AppState) -> anyhow::Result<usize> {
    let tmp_dir = state.settings.tmp_dir();
    tokio::fs::create_dir_all(&tmp_dir).await?;
    let download = tempfile::NamedTempFile::new_in(&tmp_dir)?.into_temp_path();
    fetch(&state.settings.switch_titles, &download).await?;
    let file = std::fs::File::open(&download)?;
    let titles = tokio::task::spawn_blocking(move || parse(std::io::BufReader::new(file))).await??;
    anyhow::ensure!(!titles.is_empty(), "no titles in the list");
    let cache = tempfile::NamedTempFile::new_in(&tmp_dir)?;
    tokio::fs::write(cache.path(), serde_json::to_vec(&titles)?).await?;
    cache.persist(state.settings.data_dir.join(CACHE))?;
    let n = titles.len();
    *state.switch_titles.write().unwrap() = titles;
    Ok(n)
}

/// A URL is downloaded; anything else is a file (offline use, tests).
async fn fetch(source: &str, to: &Path) -> anyhow::Result<()> {
    if !(source.starts_with("https://") || source.starts_with("http://")) {
        tokio::fs::copy(source, to).await?;
        return Ok(());
    }
    let mut body = reqwest::get(source).await?.error_for_status()?.bytes_stream();
    let mut file = tokio::fs::File::create(to).await?;
    while let Some(chunk) = body.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLEDB: &str = r#"{
        "70010000000025": {"id": "01007EF00011E000", "name": "The Legend of Zelda™: Breath of the Wild", "size": 1},
        "70010000000026": {"id": "01007ef00011e800", "name": "Update"},
        "70010000000027": {"id": "01007EF00011F001", "name": "Expansion Pass"},
        "70010000000028": {"id": null, "name": "Bundle"},
        "70010000000029": {"id": "0100000000010000", "name": "Super Mario Odyssey®"},
        "70010000000030": {"id": "0100000000010000", "name": "Super Mario Odyssey (other region)"}
    }"#;

    #[test]
    fn only_base_titles_are_kept_with_clean_names() {
        let titles = parse(TITLEDB.as_bytes()).unwrap();
        assert_eq!(
            titles,
            BTreeMap::from([
                ("0100000000010000".into(), "Super Mario Odyssey".into()),
                (
                    "01007EF00011E000".into(),
                    "The Legend of Zelda: Breath of the Wild".into()
                ),
            ])
        );
    }

    #[test]
    fn games_are_found_by_their_title_id() {
        let titles: SwitchTitles = Arc::new(RwLock::new(parse(TITLEDB.as_bytes()).unwrap()));
        assert_eq!(
            name_of(&titles, "Switch 0100000000010000").as_deref(),
            Some("Super Mario Odyssey")
        );
        assert_eq!(name_of(&titles, "Switch 0100000000099000"), None);
        assert_eq!(name_of(&titles, "Super Mario Odyssey"), None);
    }
}
