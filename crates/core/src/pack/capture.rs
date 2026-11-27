use std::{
    fs::{File, Metadata},
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};

use sha2::{Digest, Sha256};
use spielstand_protocol::FileEntry;

use super::{PackError, Packed, Source, content_hash, entry_name, sha256_file};

/// Identity and kernel-maintained change markers. This detects ordinary writes, not an
/// application transaction or every write through a memory map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Fingerprint {
    fn of(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

fn source_io<T>(source: &Source, result: io::Result<T>) -> Result<T, PackError> {
    result.map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => PackError::SourceChanged(source.portable.clone()),
        _ => PackError::Io(e),
    })
}

pub fn fingerprints(sources: &[Source]) -> Result<Vec<Fingerprint>, PackError> {
    sources
        .iter()
        .map(|s| source_io(s, std::fs::metadata(&s.path)).map(|m| Fingerprint::of(&m)))
        .collect()
}

/// Recheck the whole set, including files already read before another file was captured.
pub fn check_sources(sources: &[Source], before: &[Fingerprint]) -> Result<(), PackError> {
    let after = fingerprints(sources)?;
    if let Some(i) = before.iter().zip(&after).position(|(a, b)| a != b) {
        return Err(PackError::SourceChanged(sources[i].portable.clone()));
    }
    if before.len() != after.len() {
        return Err(PackError::SourceChanged("file set".into()));
    }
    Ok(())
}

struct HashingReader<R> {
    reader: R,
    hash: Sha256,
    size: u64,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(buf)?;
        self.hash.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }
}

fn append(
    builder: &mut tar::Builder<impl Write>,
    source: &Source,
    before: &Fingerprint,
) -> Result<FileEntry, PackError> {
    let file = source_io(source, File::open(&source.path))?;
    let meta = file.metadata()?;
    if Fingerprint::of(&meta) != *before {
        return Err(PackError::SourceChanged(source.portable.clone()));
    }
    let mut header = tar::Header::new_gnu();
    header.set_metadata(&meta);
    header.set_entry_type(tar::EntryType::Regular);
    let mut input = HashingReader {
        reader: file.take(before.size),
        hash: Sha256::new(),
        size: 0,
    };
    let result = builder.append_data(&mut header, entry_name(&source.portable), &mut input);
    // Check the open descriptor as well as the pathname, including when tar reports a
    // short read. A disappeared/replaced source is retryable; output I/O failures are not.
    let unchanged = Fingerprint::of(&input.reader.get_ref().metadata()?) == *before;
    check_sources(std::slice::from_ref(source), std::slice::from_ref(before))?;
    if !unchanged {
        return Err(PackError::SourceChanged(source.portable.clone()));
    }
    result?;
    if input.size != before.size {
        return Err(PackError::SourceChanged(source.portable.clone()));
    }
    Ok(FileEntry {
        path: source.portable.clone(),
        size: input.size,
        sha256: hex::encode(input.hash.finalize()),
    })
}

pub(super) fn pack(
    sources: &[Source],
    out: &Path,
    mut after_file: impl FnMut(usize),
) -> Result<Packed, PackError> {
    let before = fingerprints(sources)?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(parent)?;
    let encoder = zstd::Encoder::new(tmp.as_file(), 9)?;
    let mut builder = tar::Builder::new(encoder);
    let mut files = Vec::with_capacity(sources.len());
    for (i, (source, stamp)) in sources.iter().zip(&before).enumerate() {
        files.push(append(&mut builder, source, stamp)?);
        after_file(i);
    }
    let mut encoder = builder.into_inner()?;
    encoder.flush()?;
    encoder.finish()?.sync_all()?;
    check_sources(sources, &before)?;
    let packed = Packed {
        content_hash: content_hash(&files),
        blob_sha256: sha256_file(tmp.path())?,
        blob_size: tmp.as_file().metadata()?.len(),
        files,
    };
    tmp.persist(out).map_err(|e| PackError::Io(e.error))?;
    Ok(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_during_capture_are_rejected_and_output_is_untouched() {
        for change in ["rewrite", "grow", "truncate", "replace", "delete"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("save");
            std::fs::write(&path, "original").unwrap();
            let sources = [Source {
                path: path.clone(),
                portable: "<home>/save".into(),
            }];
            let out = dir.path().join("out");
            std::fs::write(&out, "existing").unwrap();
            let result = pack(&sources, &out, |_| match change {
                "rewrite" => std::fs::write(&path, "modified").unwrap(),
                "grow" => std::fs::write(&path, "longer than original").unwrap(),
                "truncate" => std::fs::write(&path, "").unwrap(),
                "replace" => {
                    let other = dir.path().join("other");
                    std::fs::write(&other, "original").unwrap();
                    std::fs::rename(other, &path).unwrap();
                }
                "delete" => std::fs::remove_file(&path).unwrap(),
                _ => unreachable!(),
            });
            assert!(matches!(result, Err(PackError::SourceChanged(_))), "{change}");
            assert_eq!(std::fs::read(&out).unwrap(), b"existing");
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                if change == "delete" { 1 } else { 2 }
            );
        }
    }

    #[test]
    fn all_sources_are_rechecked_after_later_files() {
        let dir = tempfile::tempdir().unwrap();
        let sources: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|name| {
                let path = dir.path().join(name);
                std::fs::write(&path, name).unwrap();
                Source {
                    path,
                    portable: name.into(),
                }
            })
            .collect();
        let out = dir.path().join("out");
        let result = pack(&sources, &out, |i| {
            if i == 1 {
                std::fs::write(&sources[0].path, "changed").unwrap();
            }
        });
        assert!(matches!(result, Err(PackError::SourceChanged(_))));
        assert!(!out.exists());
    }

    #[test]
    fn empty_files_and_streamed_files_round_trip_with_exact_sizes() {
        let dir = tempfile::tempdir().unwrap();
        for bytes in [vec![], vec![42; 200_000]] {
            let path = dir.path().join("save");
            std::fs::write(&path, &bytes).unwrap();
            let out = dir.path().join("out");
            let packed = pack(
                &[Source {
                    path,
                    portable: "<home>/save".into(),
                }],
                &out,
                |_| {},
            )
            .unwrap();
            assert_eq!(packed.files[0].size, bytes.len() as u64);
            assert_eq!(packed.files[0].sha256, super::super::sha256_hex(&bytes));
            assert_eq!(
                super::super::unpack(&out, &packed.files).unwrap()["<home>/save"],
                bytes
            );
        }
    }
}
