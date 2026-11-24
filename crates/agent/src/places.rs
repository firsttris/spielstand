//! Where games' saves were found lately, for what runs often and needs only the places.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Longer than the 30 minutes between two catch-ups, which look at the disk and so refresh this.
const LATELY: Duration = Duration::from_secs(35 * 60);

#[derive(Default)]
pub struct Places(Mutex<HashMap<String, (Instant, Vec<PathBuf>)>>);

impl Places {
    /// A game without saves is not remembered: looking again reads nothing, and its first save
    /// is found at once.
    pub fn remember(&self, game: &str, files: &[PathBuf], now: Instant) {
        let mut places = self.0.lock().unwrap();
        if files.is_empty() {
            places.remove(game);
        } else {
            places.insert(game.to_string(), (now, files.to_vec()));
        }
    }

    pub fn recall(&self, game: &str, now: Instant) -> Option<Vec<PathBuf>> {
        let places = self.0.lock().unwrap();
        let (at, files) = places.get(game)?;
        (now.duration_since(*at) < LATELY).then(|| files.clone())
    }

    pub fn forget(&self) {
        self.0.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recalls_what_was_found_lately() {
        let (places, start) = (Places::default(), Instant::now());
        let files = vec![PathBuf::from("/saves/1.sav")];
        places.remember("Hades", &files, start);

        let before_next_catch_up = start + Duration::from_secs(30 * 60);
        assert_eq!(places.recall("Hades", before_next_catch_up), Some(files));
        assert_eq!(places.recall("Hades", start + LATELY), None);
        assert_eq!(places.recall("Celeste", start), None);
    }

    #[test]
    fn forgets_on_request_and_when_the_saves_are_gone() {
        let (places, start) = (Places::default(), Instant::now());
        let files = vec![PathBuf::from("/saves/1.sav")];
        places.remember("Hades", &files, start);
        places.remember("Hades", &[], start);
        assert_eq!(places.recall("Hades", start), None);

        places.remember("Hades", &files, start);
        places.forget();
        assert_eq!(places.recall("Hades", start), None);
    }
}
