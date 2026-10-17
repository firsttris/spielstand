//! Questions and notices as desktop notifications (freedesktop, via D-Bus). Without a
//! notification service every question times out, and the local state is kept.

use std::time::Duration;

use async_trait::async_trait;
use notify_rust::{Notification, Timeout};

use crate::sync::{Choice, Ui};

pub struct Desktop;

fn ask(summary: String, body: String, actions: Vec<(String, String)>, timeout: Duration) -> Option<String> {
    let mut n = Notification::new();
    n.summary(&summary)
        .body(&body)
        .appname("Spielstand")
        .timeout(Timeout::Milliseconds(timeout.as_millis() as u32));
    for (id, label) in &actions {
        n.action(id, label);
    }
    let handle = n.show().ok()?;
    let mut picked = None;
    handle.wait_for_action(|action| picked = Some(action.to_string()));
    picked
}

#[async_trait]
impl Ui for Desktop {
    async fn ask_conflict(&self, game: &str, other_device: &str, timeout: Duration) -> Choice {
        let summary = format!("{game}: auf zwei PCs gespielt");
        let body = format!(
            "Hier und auf {other_device} hat sich der Spielstand geändert. Beide sind gesichert. \
             Ohne Wahl wird der Stand dieses PCs gespielt."
        );
        let actions = vec![
            ("mine".into(), "Stand dieses PCs".into()),
            ("theirs".into(), format!("Stand von {other_device}")),
        ];
        let task = tokio::task::spawn_blocking(move || ask(summary, body, actions, timeout));
        match tokio::time::timeout(timeout + Duration::from_secs(2), task).await {
            Ok(Ok(Some(a))) if a == "mine" => Choice::Mine,
            Ok(Ok(Some(a))) if a == "theirs" => Choice::Theirs,
            _ => Choice::NoAnswer,
        }
    }

    async fn warn_playing_elsewhere(&self, game: &str, device: &str) {
        self.info(
            &format!("{game} läuft gerade auf {device}"),
            "Beende es dort zuerst, sonst entstehen zwei Stände (beide bleiben erhalten).",
        )
        .await;
    }

    async fn info(&self, title: &str, body: &str) {
        let (title, body) = (title.to_string(), body.to_string());
        let _ = tokio::task::spawn_blocking(move || {
            Notification::new()
                .summary(&title)
                .body(&body)
                .appname("Spielstand")
                .show()
                .map(|_| ())
        })
        .await;
    }
}
