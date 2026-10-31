//! A version on the wire and on disk: one `.tar.zst`, entries named by portable path.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use spielstand_protocol::FileEntry;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("entry {0} is not a safe relative name")]
    UnsafeEntry(String),
    #[error("entry {path} has hash {actual}, the manifest says {expected}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("the package misses {0}")]
    Missing(String),
}

/// A file to pack: where it lives now and its portable name.
pub struct Source {
    pub portable: String,
    pub path: PathBuf,
}

/// The result of packing: the entries and the hashes that identify the version.
pub struct Packed {
    pub files: Vec<FileEntry>,
    pub content_hash: String,
    pub blob_sha256: String,
    pub blob_size: u64,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    // sha2 0.11 no longer implements io::Write
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Equal sets of files give equal hashes, whatever order they were found in.
pub fn content_hash(files: &[FileEntry]) -> String {
    let mut sorted: Vec<_> = files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    for (path, sha) in sorted {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(sha.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Hash the files without packing them, to compare the disk with a known version.
pub fn describe(sources: &[Source]) -> io::Result<Vec<FileEntry>> {
    sources
        .iter()
        .map(|s| {
            Ok(FileEntry {
                path: s.portable.clone(),
                size: std::fs::metadata(&s.path)?.len(),
                sha256: sha256_file(&s.path)?,
            })
        })
        .collect()
}

/// Entry names inside the archive: the portable path with its anchor kept, made relative.
fn entry_name(portable: &str) -> String {
    format!("files/{}", portable.trim_start_matches('/'))
}

pub fn pack(sources: &[Source], out: &Path) -> Result<Packed, PackError> {
    let files = describe(sources)?;
    let blob = File::create(out)?;
    let encoder = zstd::Encoder::new(blob, 9)?;
    let mut builder = tar::Builder::new(encoder);
    for source in sources {
        let mut file = File::open(&source.path)?;
        builder.append_file(entry_name(&source.portable), &mut file)?;
    }
    let mut encoder = builder.into_inner()?;
    encoder.flush()?;
    encoder.finish()?.sync_all()?;
    Ok(Packed {
        content_hash: content_hash(&files),
        blob_sha256: sha256_file(out)?,
        blob_size: std::fs::metadata(out)?.len(),
        files,
    })
}

/// Read every file of a package into memory, checked against the manifest's hashes.
pub fn unpack(blob: &Path, expected: &[FileEntry]) -> Result<BTreeMap<String, Vec<u8>>, PackError> {
    let decoder = zstd::Decoder::new(File::open(blob)?)?;
    let mut archive = tar::Archive::new(decoder);
    let by_entry: BTreeMap<String, &FileEntry> = expected.iter().map(|f| (entry_name(&f.path), f)).collect();
    let mut out = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().to_string();
        if name.contains("..") {
            return Err(PackError::UnsafeEntry(name));
        }
        let Some(meta) = by_entry.get(&name) else { continue };
        let mut bytes = Vec::with_capacity(meta.size as usize);
        entry.read_to_end(&mut bytes)?;
        let actual = sha256_hex(&bytes);
        if actual != meta.sha256 {
            return Err(PackError::HashMismatch {
                path: meta.path.clone(),
                expected: meta.sha256.clone(),
                actual,
            });
        }
        out.insert(meta.path.clone(), bytes);
    }
    if let Some(missing) = expected.iter().find(|f| !out.contains_key(&f.path)) {
        return Err(PackError::Missing(missing.path.clone()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_bytes_and_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.sav");
        let b = dir.path().join("b.sav");
        std::fs::write(&a, b"alpha").unwrap();
        std::fs::write(&b, vec![7u8; 100_000]).unwrap();
        let sources = vec![
            Source {
                portable: "<home>/Game/a.sav".into(),
                path: a,
            },
            Source {
                portable: "<root:steam>/x/b.sav".into(),
                path: b,
            },
        ];
        let out = dir.path().join("v.tar.zst");
        let packed = pack(&sources, &out).unwrap();
        assert_eq!(packed.files.len(), 2);
        assert_eq!(packed.blob_sha256, sha256_file(&out).unwrap());

        let back = unpack(&out, &packed.files).unwrap();
        assert_eq!(back["<home>/Game/a.sav"], b"alpha");
        assert_eq!(back["<root:steam>/x/b.sav"].len(), 100_000);
    }

    #[test]
    fn content_hash_ignores_order() {
        let f = |p: &str, h: &str| FileEntry {
            path: p.into(),
            size: 1,
            sha256: h.into(),
        };
        assert_eq!(
            content_hash(&[f("a", "1"), f("b", "2")]),
            content_hash(&[f("b", "2"), f("a", "1")])
        );
        assert_ne!(content_hash(&[f("a", "1")]), content_hash(&[f("a", "2")]));
    }

    #[test]
    fn tampered_manifest_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.sav");
        std::fs::write(&a, b"alpha").unwrap();
        let out = dir.path().join("v.tar.zst");
        let mut packed = pack(
            &[Source {
                portable: "<home>/a.sav".into(),
                path: a,
            }],
            &out,
        )
        .unwrap();
        packed.files[0].sha256 = "00".into();
        assert!(matches!(
            unpack(&out, &packed.files),
            Err(PackError::HashMismatch { .. })
        ));
    }
}
