//! Flows that run side by side on one game: the file watcher, the server's pushes and the
//! catch-up are separate tasks in the service and must not decide on each other's half-done work.

mod common;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use common::{GAME, pc, pc_with_scanner, server};
use spielstand_agent::{
    Mode, Outcome,
    engine::{FakeScanner, Scanner},
    settings::Sources,
};
use spielstand_core::portable::Anchors;

/// The fake disk, but the `n`-th look at the save files after [`Paused::arm`] waits until the
/// test lets it go: a flow stopped between asking the server and reading the disk.
struct Paused {
    inner: FakeScanner,
    calls_left: AtomicUsize,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
}

impl Paused {
    /// Returns (the flow is paused now, let it go on).
    fn arm(&self, n: usize) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        *self.entered.lock().unwrap() = Some(entered_tx);
        *self.release.lock().unwrap() = Some(release_rx);
        self.calls_left.store(n, Ordering::SeqCst);
        (entered_rx, release_tx)
    }

    fn maybe_pause(&self) {
        let left = self.calls_left.load(Ordering::SeqCst);
        if left == 0 {
            return;
        }
        self.calls_left.store(left - 1, Ordering::SeqCst);
        if left != 1 {
            return;
        }
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        if let Some(release) = self.release.lock().unwrap().take() {
            let _ = release.recv_timeout(Duration::from_secs(10));
        }
    }
}

impl Scanner for Paused {
    fn anchors(&self) -> Anchors {
        self.inner.anchors()
    }
    fn save_files(&self, game: &str) -> Vec<PathBuf> {
        self.maybe_pause();
        self.inner.save_files(game)
    }
    fn games_with_saves(&self) -> Vec<String> {
        self.inner.games_with_saves()
    }
    fn steam_cloud(&self, game: &str) -> bool {
        self.inner.steam_cloud(game)
    }
    fn game_for_steam_app(&self, app_id: u32) -> Option<String> {
        self.inner.game_for_steam_app(app_id)
    }
    fn install_dirs(&self) -> Vec<(String, PathBuf)> {
        self.inner.install_dirs()
    }
    fn installed_games(&self) -> BTreeSet<String> {
        self.inner.installed_games()
    }
    fn configure(
        &self,
        wine_prefixes: Vec<PathBuf>,
        game_folders: Vec<PathBuf>,
        own: BTreeMap<String, PathBuf>,
    ) {
        self.inner.configure(wine_prefixes, game_folders, own)
    }
    fn own_prefix(&self, game: &str) -> Option<PathBuf> {
        self.inner.own_prefix(game)
    }
    fn find_title(&self, name: &str) -> Option<String> {
        self.inner.find_title(name)
    }
    fn in_manifest(&self, game: &str) -> bool {
        self.inner.in_manifest(game)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_change_recorded_while_a_push_is_handled_is_a_conflict_not_overwritten() {
    let s = server().await;
    let paused: Arc<Mutex<Option<Arc<Paused>>>> = Arc::default();
    let slot = paused.clone();
    let desktop = pc_with_scanner(&s, "desktop", move |home| {
        let mut games = BTreeMap::new();
        games.insert(GAME.to_string(), vec![home.join(".local/share/EldenRing")]);
        let scanner = Arc::new(Paused {
            inner: FakeScanner {
                anchors: Anchors {
                    home: home.to_path_buf(),
                    roots: vec![],
                },
                games,
                ..Default::default()
            },
            calls_left: AtomicUsize::new(0),
            entered: Mutex::default(),
            release: Mutex::default(),
        });
        *slot.lock().unwrap() = Some(scanner.clone());
        (scanner as Arc<dyn Scanner>, Sources::default())
    })
    .await;
    let paused = paused.lock().unwrap().clone().unwrap();
    let deck = pc(&s, "deck", false).await;

    desktop.write_save("shared");
    desktop.agent.after_session(GAME, 1).await.unwrap();
    deck.agent.prepare_launch(GAME).await;
    // The deck comes back from a trip and is first: its state is the new head.
    deck.write_save("deck: played offline");
    assert_eq!(
        deck.agent.after_session(GAME, 60).await.unwrap(),
        Outcome::Uploaded
    );
    desktop.write_save("desktop: played meanwhile");

    // The push about the deck's head: the desktop asks the server, then stops before its disk.
    // (Its first look at the save files is for the game's mode, the second is the disk.)
    let (entered, release) = paused.arm(2);
    let push = {
        let agent = desktop.agent.clone();
        tokio::spawn(async move { agent.sync_game(GAME, Mode::Background).await })
    };
    tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(10)))
        .await
        .unwrap()
        .expect("the push reached the disk");
    // Meanwhile the file watcher records the desktop's change.
    let watcher = {
        let agent = desktop.agent.clone();
        tokio::spawn(async move { agent.observe(GAME).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    release.send(()).unwrap();
    push.await.unwrap().unwrap();
    watcher.await.unwrap().unwrap();

    assert_eq!(desktop.read_save().as_deref(), Some("desktop: played meanwhile"));
    assert!(matches!(
        desktop.agent.sync_game(GAME, Mode::Background).await.unwrap(),
        Outcome::Conflict { .. }
    ));
    assert_eq!(desktop.read_save().as_deref(), Some("desktop: played meanwhile"));
}
