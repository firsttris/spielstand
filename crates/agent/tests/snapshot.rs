mod common;

use common::{GAME, pc, pc_with_scanner, server};
use spielstand_agent::{Outcome, disk::UnstableSnapshot, engine::Scanner, settings::Sources};
use spielstand_core::portable::Anchors;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Changes the save on every discovery call, deterministically crossing each capture's
/// before/after checks without depending on filesystem event timing.
struct WritingScanner {
    home: PathBuf,
    writes: AtomicUsize,
}

impl WritingScanner {
    fn save(&self) -> PathBuf {
        self.home.join(".local/share/EldenRing/ER0000.sl2")
    }
}

impl Scanner for WritingScanner {
    fn anchors(&self) -> Anchors {
        Anchors {
            home: self.home.clone(),
            roots: vec![],
        }
    }
    fn save_files(&self, _: &str) -> Vec<PathBuf> {
        let path = self.save();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let n = self.writes.fetch_add(1, Ordering::SeqCst);
        std::fs::write(&path, format!("local write {n}")).unwrap();
        vec![path]
    }
    fn games_with_saves(&self) -> Vec<String> {
        vec![GAME.into()]
    }
    fn steam_cloud(&self, _: &str) -> bool {
        false
    }
    fn game_for_steam_app(&self, _: u32) -> Option<String> {
        None
    }
    fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        vec![]
    }
    fn configure(&self, _: Vec<PathBuf>, _: Vec<PathBuf>, _: BTreeMap<String, PathBuf>) {}
    fn own_prefix(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn find_title(&self, name: &str) -> Option<String> {
        (name == GAME).then(|| name.into())
    }
    fn in_manifest(&self, name: &str) -> bool {
        name == GAME
    }
}

#[tokio::test]
async fn unstable_safety_snapshot_blocks_restore_and_launch_keeps_local_saves() {
    let s = server().await;
    let source = pc(&s, "source", false).await;
    source.write_save("remote state");
    source.agent.after_session(GAME, 1).await.unwrap();
    let remote = spielstand_server::versions::head_of(&s.state.db, GAME)
        .await
        .unwrap()
        .unwrap();
    let target = pc_with_scanner(&s, "target", |home| {
        (
            Arc::new(WritingScanner {
                home: home.into(),
                writes: AtomicUsize::new(0),
            }),
            Sources::default(),
        )
    })
    .await;
    let error = target.agent.restore_version(GAME, &remote).await.unwrap_err();
    assert!(error.downcast_ref::<UnstableSnapshot>().is_some(), "{error:#}");
    assert!(target.read_save().unwrap().starts_with("local write"));
    assert!(target.agent.store.base(GAME).await.unwrap().is_none());
    assert!(target.agent.store.pending().await.unwrap().is_empty());
    assert_eq!(target.agent.prepare_launch(GAME).await, Outcome::Offline);
    assert!(target.read_save().unwrap().starts_with("local write"));
    assert!(target.agent.store.base(GAME).await.unwrap().is_none());
}
