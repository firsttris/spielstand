//! Validate uploads without extracting files or buffering their contents. Raw tar iteration
//! keeps extension headers from allocating unbounded buffers inside the tar reader.

use super::{PackError, entry_name};
use sha2::{Digest, Sha256};
use spielstand_protocol::FileEntry;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

struct Budget<R> {
    reader: R,
    left: u64,
    read: u64,
}

impl<R: Read> Read for Budget<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            let mut probe = [0];
            return match self.reader.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other("decompressed package exceeds its limit")),
            };
        }
        let len = (buf.len() as u64).min(self.left) as usize;
        let n = self.reader.read(&mut buf[..len])?;
        self.left -= n as u64;
        self.read += n as u64;
        Ok(n)
    }
}

fn invalid(message: impl Into<String>) -> PackError {
    PackError::InvalidArchive(message.into())
}

fn safe_name(name: &str) -> bool {
    name.starts_with("files/")
        && !name.contains("..")
        && !name.contains('\\')
        && name.split('/').all(|s| !s.is_empty() && s != ".")
}

/// `limit` counts all decompressed tar bytes, including headers, extensions and padding.
/// Decoder history is capped at 64 MiB independently of the advertised file sizes.
pub fn validate(blob: &Path, expected: &[FileEntry], limit: u64) -> Result<(), PackError> {
    let mut decoder = zstd::Decoder::new(File::open(blob)?)?;
    decoder.window_log_max(26)?;
    let input = Budget {
        reader: decoder,
        left: limit,
        read: 0,
    };
    validate_tar(input, expected)
}

fn validate_tar<R: Read>(input: Budget<R>, expected: &[FileEntry]) -> Result<(), PackError> {
    let mut remaining = BTreeMap::new();
    for file in expected {
        let name = entry_name(&file.path);
        if !safe_name(&name) {
            return Err(PackError::UnsafeEntry(name));
        }
        if remaining.insert(name, file).is_some() {
            return Err(invalid("duplicate manifest entry"));
        }
    }
    let max_name = remaining.keys().map(|n| n.len()).max().unwrap_or(0);
    let mut archive = tar::Archive::new(input);
    let mut long_name = None;
    let mut end = 0u64;
    for entry in archive.entries()?.raw(true) {
        let mut entry = entry?;
        let size = entry.size();
        end = entry
            .raw_file_position()
            .checked_add(size)
            .and_then(|n| n.checked_add((512 - size % 512) % 512))
            .ok_or_else(|| invalid("tar size overflow"))?;
        if entry.header().entry_type().is_gnu_longname() && entry.header().as_gnu().is_some() {
            if long_name.is_some() || size > max_name as u64 + 1 {
                return Err(invalid("invalid long pathname"));
            }
            let mut bytes = Vec::with_capacity(size as usize);
            entry.read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size || bytes.pop() != Some(0) || bytes.contains(&0) {
                return Err(invalid("invalid long pathname"));
            }
            long_name = Some(String::from_utf8(bytes).map_err(|_| invalid("non-UTF-8 pathname"))?);
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(invalid("non-regular archive entry"));
        }
        let name = match long_name.take() {
            Some(name) => name,
            None => String::from_utf8(entry.path_bytes().into_owned())
                .map_err(|_| invalid("non-UTF-8 pathname"))?,
        };
        if !safe_name(&name) {
            return Err(PackError::UnsafeEntry(name));
        }
        let meta = remaining
            .remove(&name)
            .ok_or_else(|| invalid(format!("unexpected or duplicate entry {name}")))?;
        if size != meta.size {
            return Err(invalid(format!("entry {name} has the wrong size")));
        }
        check_bytes(&mut entry, meta)?;
    }
    if long_name.is_some() {
        return Err(invalid("long pathname without a file"));
    }
    finish(archive.into_inner(), end)?;
    if let Some(meta) = remaining.values().next() {
        return Err(PackError::Missing(meta.path.clone()));
    }
    Ok(())
}

fn finish<R: Read>(mut input: Budget<R>, end: u64) -> Result<(), PackError> {
    // Drain through the decoder to check frame completion/checksums and charge padding to
    // the same budget. Nonzero trailing data would hide members after tar's end marker.
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if buf[..n].iter().any(|b| *b != 0) {
            return Err(invalid("data after tar end marker"));
        }
    }
    if input.read
        < end
            .checked_add(1024)
            .ok_or_else(|| invalid("tar size overflow"))?
    {
        return Err(invalid("missing tar end markers"));
    }
    Ok(())
}

fn check_bytes(input: &mut impl Read, meta: &FileEntry) -> Result<(), PackError> {
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        size += n as u64;
    }
    if size != meta.size {
        return Err(invalid("truncated archive entry"));
    }
    let actual = hex::encode(hash.finalize());
    if actual != meta.sha256 {
        return Err(PackError::HashMismatch {
            path: meta.path.clone(),
            expected: meta.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{self, Source};

    fn manifest() -> Vec<FileEntry> {
        vec![FileEntry {
            path: "<home>/save".into(),
            size: 5,
            sha256: pack::sha256_hex(b"hello"),
        }]
    }

    fn tar_bytes(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
        let mut out = vec![];
        for (name, bytes, kind) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_entry_type(*kind);
            header.as_mut_bytes()[..100].fill(0);
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(bytes);
            out.resize(out.len().next_multiple_of(512), 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    fn check_tar(bytes: &[u8], expected: &[FileEntry], limit: u64) -> Result<(), PackError> {
        let dir = tempfile::tempdir().unwrap();
        let blob = dir.path().join("blob");
        std::fs::write(&blob, zstd::encode_all(bytes, 1).unwrap()).unwrap();
        validate(&blob, expected, limit)
    }

    #[test]
    fn manifest_must_match_each_archived_file_exactly_once() {
        let name = "files/<home>/save";
        let good = (name, b"hello".as_slice(), tar::EntryType::Regular);
        assert!(check_tar(&tar_bytes(&[good]), &manifest(), 4096).is_ok());
        for entries in [
            vec![(name, b"other".as_slice(), tar::EntryType::Regular)],
            vec![(name, b"shorter?".as_slice(), tar::EntryType::Regular)],
            vec![],
            vec![good, good],
            vec![
                good,
                ("files/<home>/extra", b"hello".as_slice(), tar::EntryType::Regular),
            ],
            vec![("files/../save", b"hello".as_slice(), tar::EntryType::Regular)],
            vec![("/absolute", b"hello".as_slice(), tar::EntryType::Regular)],
            vec![(name, b"hello".as_slice(), tar::EntryType::Symlink)],
            vec![(name, b"hello".as_slice(), tar::EntryType::Directory)],
            vec![(name, b"hello".as_slice(), tar::EntryType::Link)],
            vec![(name, b"hello".as_slice(), tar::EntryType::GNUSparse)],
            vec![(name, b"hello".as_slice(), tar::EntryType::XHeader)],
        ] {
            assert!(
                check_tar(&tar_bytes(&entries), &manifest(), 8192).is_err(),
                "{entries:?}"
            );
        }
        let mut duplicate = manifest();
        duplicate.extend(manifest());
        assert!(check_tar(&tar_bytes(&[good]), &duplicate, 4096).is_err());
    }

    #[test]
    fn malformed_truncated_and_hidden_members_are_rejected() {
        let raw = tar_bytes(&[("files/<home>/save", b"hello", tar::EntryType::Regular)]);
        for length in [0, 100, 512, 514, 1024, 1536] {
            assert!(
                check_tar(&raw[..length], &manifest(), 4096).is_err(),
                "length {length}"
            );
        }
        let mut corrupt = raw.clone();
        corrupt[0] ^= 1;
        assert!(check_tar(&corrupt, &manifest(), 4096).is_err());
        let mut hidden = raw;
        hidden.extend(tar_bytes(&[("files/hidden", b"hidden", tar::EntryType::Regular)]));
        assert!(check_tar(&hidden, &manifest(), 8192).is_err());
        let dir = tempfile::tempdir().unwrap();
        let blob = dir.path().join("blob");
        let mut compressed = zstd::encode_all(tar_bytes(&[]).as_slice(), 1).unwrap();
        compressed.pop();
        std::fs::write(&blob, compressed).unwrap();
        assert!(validate(&blob, &[], 4096).is_err());
    }

    #[test]
    fn decompressed_bytes_and_decoder_history_are_bounded() {
        let raw = tar_bytes(&[("files/<home>/save", b"hello", tar::EntryType::Regular)]);
        assert!(check_tar(&raw, &manifest(), raw.len() as u64).is_ok());
        assert!(check_tar(&raw, &manifest(), raw.len() as u64 - 1).is_err());
        let dir = tempfile::tempdir().unwrap();
        let blob = dir.path().join("blob");
        let mut encoder = zstd::Encoder::new(Vec::new(), 1).unwrap();
        use std::io::Write;
        encoder.write_all(&raw).unwrap();
        let mut compressed = encoder.finish().unwrap();
        assert_eq!(compressed[4] & 0x20, 0, "stream frame has a window descriptor");
        compressed[5] = (27 - 10) << 3; // Advertise a 128 MiB decoder window.
        assert_eq!(zstd::decode_all(compressed.as_slice()).unwrap(), raw);
        std::fs::write(&blob, compressed).unwrap();
        assert!(validate(&blob, &manifest(), 4096).is_err());
    }

    #[test]
    fn long_names_empty_and_large_files_from_the_packer_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let blob = dir.path().join("blob");
        for bytes in [vec![], vec![13; 200_000]] {
            std::fs::write(&path, &bytes).unwrap();
            let packed = pack::pack(
                &[Source {
                    path: path.clone(),
                    portable: format!("<home>/{}save", "directory/".repeat(40)),
                }],
                &blob,
            )
            .unwrap();
            validate(&blob, &packed.files, 1_000_000).unwrap();
            assert_eq!(
                pack::unpack(&blob, &packed.files)
                    .unwrap()
                    .into_values()
                    .next()
                    .unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn extension_headers_cannot_allocate_their_advertised_size() {
        let raw = tar_bytes(&[("././@LongLink", b"name\0", tar::EntryType::GNULongName)]);
        assert!(check_tar(&raw, &manifest(), 4096).is_err());
        let mut huge = raw;
        let header = tar::Header::from_byte_slice(&huge[..512]);
        let mut header = header.clone();
        header.set_size(u64::MAX / 2);
        header.set_cksum();
        huge[..512].copy_from_slice(header.as_bytes());
        assert!(check_tar(&huge, &manifest(), 4096).is_err());
    }
}
