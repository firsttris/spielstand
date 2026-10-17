//! Backups made by Ludusavi itself (full, differential, zip) come over with their whole history.

mod common;

use std::path::Path;

use ludusavi::{
    api::{Ludusavi, parameters::BackUp},
    path::StrictPath,
    prelude::Finality,
    resource::{
        config::{BackupFormat, Config, Retention, Root},
        manifest::{Manifest, Store},
    },
};
use spielstand_server::{importer, versions};

fn ludusavi_backup(home: &Path, backups: &Path, format: BackupFormat) -> Ludusavi {
    let manifest: Manifest = serde_yaml::from_str(
        r#"
Demo Game:
  files:
    <home>/.local/share/DemoGame:
      tags: [save]
"#,
    )
    .unwrap();
    let mut config = Config {
        roots: vec![Root::new(home.to_string_lossy().to_string(), Store::OtherHome)],
        ..Config::default()
    };
    config.backup.path = StrictPath::from(backups.to_path_buf());
    config.backup.retention = Retention {
        full: 1,
        differential: 3,
        force_new_full: false,
    };
    config.backup.format.chosen = format;
    Ludusavi::new(config, manifest)
}

fn back_up(lud: &mut Ludusavi) {
    lud.back_up(BackUp {
        games: vec![],
        finality: Finality::Final,
        resolve_cloud_conflict: None,
        wine_prefix: None,
        include_disabled: false,
        skip_downgrade: false,
    })
    .unwrap();
}

async fn imported_contents(s: &common::TestServer, game: &str) -> Vec<Vec<(String, String)>> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM versions WHERE game = ? ORDER BY created_at")
        .bind(game)
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    let mut out = vec![];
    for (id,) in rows {
        let meta = versions::meta(&s.state.db, &id).await.unwrap().unwrap();
        let blob = spielstand_server::blobs::path(&s.state.settings.blobs_dir(), &meta.blob_sha256);
        let files = spielstand_core::pack::unpack(&blob, &meta.files).unwrap();
        let mut list: Vec<(String, String)> = files
            .into_iter()
            .map(|(p, b)| {
                (
                    p.rsplit('/').next().unwrap().to_string(),
                    String::from_utf8(b).unwrap(),
                )
            })
            .collect();
        list.sort();
        out.push(list);
    }
    out
}

async fn import_case(format: BackupFormat) {
    let s = common::start().await;
    let work = tempfile::tempdir().unwrap();
    *ludusavi::prelude::CONFIG_DIR.lock().unwrap() = Some(work.path().join("ludusavi"));
    let home = work.path().join("home");
    let saves = home.join(".local/share/DemoGame");
    std::fs::create_dir_all(&saves).unwrap();
    let backups = work.path().join("backups");
    let mut lud = ludusavi_backup(&home, &backups, format);

    std::fs::write(saves.join("slot1.sav"), "one").unwrap();
    std::fs::write(saves.join("options.ini"), "volume=5").unwrap();
    back_up(&mut lud);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(saves.join("slot1.sav"), "two").unwrap();
    back_up(&mut lud);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(saves.join("slot1.sav"), "three").unwrap();
    back_up(&mut lud);

    let report = importer::import_dir(&s.state, &backups).await.unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.versions, 3);

    let contents = imported_contents(&s, "Demo Game").await;
    let slot = |v: &Vec<(String, String)>| v.iter().find(|(n, _)| n == "slot1.sav").unwrap().1.clone();
    assert_eq!(
        contents.iter().map(slot).collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    for v in &contents {
        assert!(
            v.iter().any(|(n, c)| n == "options.ini" && c == "volume=5"),
            "differential lost an inherited file"
        );
    }

    // Paths came over portable, and imports never take the head.
    let head = versions::head_of(&s.state.db, "Demo Game").await.unwrap();
    assert_eq!(head, None);
    let id: (String,) = sqlx::query_as("SELECT id FROM versions LIMIT 1")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    let meta = versions::meta(&s.state.db, &id.0).await.unwrap().unwrap();
    assert!(
        meta.files
            .iter()
            .all(|f| f.path.starts_with("<home>/") || f.path.contains("/home/")),
        "{:?}",
        meta.files
    );

    // Importing again adds nothing.
    let again = importer::import_dir(&s.state, &backups).await.unwrap();
    assert_eq!(again.versions, 0);
}

#[tokio::test]
async fn simple_backups_with_differentials_import() {
    import_case(BackupFormat::Simple).await;
}

#[tokio::test]
async fn zip_backups_import() {
    import_case(BackupFormat::Zip).await;
}
