//! An upload body is `u32 length (big endian) | JSON VersionMeta | package bytes`, so the
//! manifest of a version with thousands of files never has to fit into a header.

use std::path::Path;

use axum::body::Body;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::blobs::Received;

pub const MAX_META: usize = 32 * 1024 * 1024;

pub fn encode_prefix(meta_json: &[u8]) -> Vec<u8> {
    let mut out = (meta_json.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(meta_json);
    out
}

/// Takes bytes for the length and the manifest; returns what belongs to the package.
fn take_head<'a>(
    data: &'a [u8],
    len_bytes: &mut Vec<u8>,
    meta: &mut Vec<u8>,
    meta_len: &mut Option<usize>,
) -> anyhow::Result<&'a [u8]> {
    let mut data = data;
    if meta_len.is_none() {
        let take = (4 - len_bytes.len()).min(data.len());
        len_bytes.extend_from_slice(&data[..take]);
        data = &data[take..];
        if len_bytes.len() == 4 {
            let len = u32::from_be_bytes(len_bytes[..].try_into()?) as usize;
            anyhow::ensure!(len <= MAX_META, "manifest too large");
            *meta_len = Some(len);
        }
    }
    if let Some(len) = *meta_len {
        let take = (len - meta.len()).min(data.len());
        meta.extend_from_slice(&data[..take]);
        data = &data[take..];
    }
    Ok(data)
}

pub async fn receive(tmp_dir: &Path, body: Body, limit: u64) -> anyhow::Result<(Vec<u8>, Received)> {
    tokio::fs::create_dir_all(tmp_dir).await?;
    let tmp = tempfile::NamedTempFile::new_in(tmp_dir)?.into_temp_path();
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha256::new();
    let (mut len_bytes, mut meta, mut meta_len) = (Vec::new(), Vec::new(), None);
    let mut size = 0u64;
    let mut stream = body.into_data_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let rest = take_head(&chunk, &mut len_bytes, &mut meta, &mut meta_len)?;
        if rest.is_empty() {
            continue;
        }
        size += rest.len() as u64;
        anyhow::ensure!(size <= limit, "package is larger than {limit} bytes");
        hasher.update(rest);
        file.write_all(rest).await?;
    }
    let len = meta_len.ok_or_else(|| anyhow::anyhow!("upload without manifest"))?;
    anyhow::ensure!(meta.len() == len, "upload ended inside the manifest");
    file.sync_all().await?;
    Ok((
        meta,
        Received {
            tmp,
            sha256: hex::encode(hasher.finalize()),
            size,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn splits_meta_and_package_across_chunks() {
        let meta = br#"{"hello":"world"}"#;
        let mut all = encode_prefix(meta);
        all.extend_from_slice(b"PACKAGEBYTES");
        for size in [1, 3, 7, 64] {
            let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
                all.chunks(size).map(|c| Ok(c.to_vec())).collect();
            let body = Body::from_stream(futures::stream::iter(chunks));
            let dir = tempfile::tempdir().unwrap();
            let (got, received) = receive(dir.path(), body, 1000).await.unwrap();
            assert_eq!(got, meta);
            assert_eq!(received.size, 12);
            assert_eq!(std::fs::read(&received.tmp).unwrap(), b"PACKAGEBYTES");
        }
    }
}
