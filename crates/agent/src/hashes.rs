//! The hashes of save files that have not changed since they were read, so a look at all games
//! does not read all saves again. A file counts as unchanged while its inode, size and times
//! are; the change time is set by the kernel and cannot be put back by a program.

use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use spielstand_core::pack::{self, Source};
use spielstand_protocol::FileEntry;

/// A file changed this shortly before it was read is read again: a write through a memory map
/// shows in the times only once the kernel writes the page back (up to half a minute).
const SETTLED: Duration = Duration::from_secs(60);

/// Read again after this long in any case.
const TRUSTED: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn stamp(path: &Path) -> std::io::Result<Stamp> {
    let m = std::fs::metadata(path)?;
    Ok(Stamp {
        device: m.dev(),
        inode: m.ino(),
        size: m.len(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}

struct Known {
    stamp: Stamp,
    sha256: String,
    read_at: SystemTime,
}

#[derive(Default)]
pub struct Hashes(Mutex<HashMap<PathBuf, Known>>);

fn settled(stamp: &Stamp, read_at: SystemTime) -> bool {
    let changed = UNIX_EPOCH + Duration::from_secs(stamp.changed.0.max(0) as u64);
    read_at.duration_since(changed).is_ok_and(|age| age >= SETTLED)
}

impl Hashes {
    fn recall(&self, path: &Path, stamp: &Stamp, now: SystemTime) -> Option<String> {
        let known = self.0.lock().unwrap();
        let known = known.get(path).filter(|k| k.stamp == *stamp)?;
        let age = now.duration_since(known.read_at).ok()?;
        (age < TRUSTED && settled(&known.stamp, known.read_at)).then(|| known.sha256.clone())
    }

    fn hash(&self, path: &Path, now: SystemTime) -> std::io::Result<(u64, String)> {
        let before = stamp(path)?;
        if let Some(sha256) = self.recall(path, &before, now) {
            return Ok((before.size, sha256));
        }
        let sha256 = pack::sha256_file(path)?;
        let mut known = self.0.lock().unwrap();
        // Changed while it was read: what was read is no state to remember.
        if stamp(path)? == before {
            known.insert(
                path.to_path_buf(),
                Known {
                    stamp: before,
                    sha256: sha256.clone(),
                    read_at: now,
                },
            );
        } else {
            known.remove(path);
        }
        Ok((before.size, sha256))
    }

    /// Like `pack::describe`, reading only what changed.
    pub fn describe(&self, sources: &[Source]) -> std::io::Result<Vec<FileEntry>> {
        self.describe_at(sources, SystemTime::now())
    }

    fn describe_at(&self, sources: &[Source], now: SystemTime) -> std::io::Result<Vec<FileEntry>> {
        sources
            .iter()
            .map(|s| {
                let (size, sha256) = self.hash(&s.path, now)?;
                Ok(FileEntry {
                    path: s.portable.clone(),
                    size,
                    sha256,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &Path) -> Source {
        Source {
            portable: "save".into(),
            path: path.to_path_buf(),
        }
    }

    fn later(seconds: u64) -> SystemTime {
        SystemTime::now() + Duration::from_secs(seconds)
    }

    /// Whether the file is not read: an entry planted under its stamp comes back.
    fn remembered(hashes: &Hashes, path: &Path, at: SystemTime) -> bool {
        if let Some(known) = hashes.0.lock().unwrap().get_mut(path) {
            known.sha256 = "planted".into();
        }
        hashes.describe_at(&[source(path)], at).unwrap()[0].sha256 == "planted"
    }

    #[test]
    fn an_unchanged_file_is_not_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("save");
        std::fs::write(&file, "one").unwrap();
        let hashes = Hashes::default();
        let first = hashes.describe_at(&[source(&file)], later(100)).unwrap();
        assert_eq!(first, pack::describe(&[source(&file)]).unwrap());
        assert!(remembered(&hashes, &file, later(200)));
        assert!(
            !remembered(&hashes, &file, later(100 + 25 * 3600)),
            "read again after a day"
        );
    }

    #[test]
    fn a_changed_file_is_read_even_with_its_size_and_modification_time_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("save");
        std::fs::write(&file, "one").unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        let hashes = Hashes::default();
        hashes.describe_at(&[source(&file)], later(100)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(&file, "two").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(
            hashes.describe_at(&[source(&file)], later(200)).unwrap(),
            pack::describe(&[source(&file)]).unwrap()
        );
    }

    #[test]
    fn a_file_read_right_after_it_changed_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("save");
        std::fs::write(&file, "one").unwrap();
        let hashes = Hashes::default();
        hashes.describe_at(&[source(&file)], SystemTime::now()).unwrap();
        assert!(!remembered(&hashes, &file, later(200)));
    }
}
