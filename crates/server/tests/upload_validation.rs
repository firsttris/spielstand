mod common;

use common::{TestAgent, TestServer, start, start_with, version};
use spielstand_core::pack;
use spielstand_protocol::VersionMeta;

async fn assert_rejected_without_changes(
    s: &TestServer,
    agent: &TestAgent,
    meta: &VersionMeta,
    blob: &std::path::Path,
) {
    let head = spielstand_server::versions::head_of(&s.state.db, "G")
        .await
        .unwrap();
    let versions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM versions")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    let bases: Vec<(String, String, String)> =
        sqlx::query_as("SELECT device, game, version FROM device_bases ORDER BY device, game")
            .fetch_all(&s.state.db)
            .await
            .unwrap();
    let conflicts: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM conflicts")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    let events: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM events")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    let stored = spielstand_server::blobs::path(&s.state.settings.blobs_dir(), &meta.blob_sha256);
    let already_stored = stored.exists();
    assert_eq!(s.upload(agent, meta, blob).await.status(), 400);
    assert_eq!(
        spielstand_server::versions::head_of(&s.state.db, "G")
            .await
            .unwrap(),
        head
    );
    assert_eq!(
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM versions")
            .fetch_one(&s.state.db)
            .await
            .unwrap(),
        versions
    );
    assert_eq!(
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT device, game, version FROM device_bases ORDER BY device, game"
        )
        .fetch_all(&s.state.db)
        .await
        .unwrap(),
        bases
    );
    assert_eq!(
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM conflicts")
            .fetch_one(&s.state.db)
            .await
            .unwrap(),
        conflicts
    );
    assert_eq!(
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM events")
            .fetch_one(&s.state.db)
            .await
            .unwrap(),
        events
    );
    assert_eq!(stored.exists(), already_stored);
    assert_eq!(std::fs::read_dir(s.state.settings.tmp_dir()).unwrap().count(), 0);
}

#[tokio::test]
async fn matching_outer_hashes_do_not_hide_invalid_archive_contents() {
    let s = start().await;
    let agent = s.pair("pc").await;
    let (initial, blob) = version(s.dir.path(), "G", "initial", None);
    s.upload_ok(&agent, &initial, &blob).await;
    for defect in ["hash", "size", "missing", "duplicate", "unexpected", "truncated"] {
        let (mut meta, blob) = version(s.dir.path(), "G", "next state", Some(&initial.id));
        match defect {
            "hash" => meta.files[0].sha256 = pack::sha256_hex(b"different"),
            "size" => meta.files[0].size += 1,
            "missing" => {
                let mut extra = meta.files[0].clone();
                extra.path += ".missing";
                meta.files.push(extra);
            }
            "duplicate" => meta.files.push(meta.files[0].clone()),
            "unexpected" => meta.files.clear(),
            "truncated" => {
                let mut bytes = std::fs::read(&blob).unwrap();
                bytes.truncate(bytes.len() / 2);
                std::fs::write(&blob, bytes).unwrap();
                meta.blob_sha256 = pack::sha256_file(&blob).unwrap();
                meta.blob_size = std::fs::metadata(&blob).unwrap().len();
            }
            _ => unreachable!(),
        }
        meta.content_hash = pack::content_hash(&meta.files);
        spielstand_server::versions::check_meta(
            &meta,
            &pack::sha256_file(&blob).unwrap(),
            std::fs::metadata(&blob).unwrap().len(),
        )
        .unwrap();
        assert_rejected_without_changes(&s, &agent, &meta, &blob).await;
    }
}

#[tokio::test]
async fn known_uploads_are_validated_before_acknowledgement() {
    let s = start().await;
    let agent = s.pair("pc").await;
    let (mut meta, blob) = version(s.dir.path(), "G", "initial", None);
    s.upload_ok(&agent, &meta, &blob).await;
    meta.files[0].sha256 = pack::sha256_hex(b"different");
    meta.content_hash = pack::content_hash(&meta.files);
    assert_rejected_without_changes(&s, &agent, &meta, &blob).await;
}

#[tokio::test]
async fn decompressed_limit_applies_before_publication() {
    let s = start_with(|settings| settings.max_unpacked = 1024).await;
    let agent = s.pair("pc").await;
    let (meta, blob) = version(s.dir.path(), "G", "initial", None);
    assert_rejected_without_changes(&s, &agent, &meta, &blob).await;
}
