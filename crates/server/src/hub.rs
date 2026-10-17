//! Live connections to the agents: who is online, and a way to push messages to them.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use spielstand_protocol::ServerMessage;
use tokio::sync::mpsc;

type Sender = mpsc::UnboundedSender<ServerMessage>;
/// Open connections per device; a device may briefly have two while it reconnects.
type Connections = HashMap<String, Vec<(u64, Sender)>>;

#[derive(Clone, Default)]
pub struct Hub {
    connections: Arc<Mutex<Connections>>,
    next: Arc<AtomicU64>,
}

impl Hub {
    pub fn connect(&self, device: &str) -> (u64, mpsc::UnboundedReceiver<ServerMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.connections
            .lock()
            .unwrap()
            .entry(device.to_string())
            .or_default()
            .push((id, tx));
        (id, rx)
    }

    pub fn disconnect(&self, device: &str, id: u64) {
        let mut map = self.connections.lock().unwrap();
        if let Some(list) = map.get_mut(device) {
            list.retain(|(i, _)| *i != id);
            if list.is_empty() {
                map.remove(device);
            }
        }
    }

    pub fn is_online(&self, device: &str) -> bool {
        self.connections.lock().unwrap().contains_key(device)
    }

    pub fn online_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    /// Sends to every connection of the device; `false` when none took it.
    pub fn send(&self, device: &str, message: ServerMessage) -> bool {
        let map = self.connections.lock().unwrap();
        map.get(device)
            .map(|list| {
                list.iter()
                    .filter(|(_, tx)| tx.send(message.clone()).is_ok())
                    .count()
                    > 0
            })
            .unwrap_or(false)
    }

    pub fn broadcast_except(&self, except: Option<&str>, message: ServerMessage) {
        let map = self.connections.lock().unwrap();
        for (device, list) in map.iter() {
            if Some(device.as_str()) != except {
                for (_, tx) in list {
                    let _ = tx.send(message.clone());
                }
            }
        }
    }
}
