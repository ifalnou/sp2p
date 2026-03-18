use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct Router {
    // Maps an inbox name to a set of IP:Port addresses that advertise it.
    table: Arc<RwLock<HashMap<String, HashSet<SocketAddr>>>>,
    // Maps IP:Port address to a set of inboxes they own
    peers: Arc<RwLock<HashMap<SocketAddr, HashSet<String>>>>,
    // Maps IP:Port address to peer's human readable name
    peer_names: Arc<RwLock<HashMap<SocketAddr, String>>>,
    // Channel to notify when an inbox is discovered or comes online
    new_inbox_tx: Arc<broadcast::Sender<String>>,
}

impl Default for Router {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            table: Arc::new(RwLock::new(HashMap::new())),
            peers: Arc::new(RwLock::new(HashMap::new())),
            peer_names: Arc::new(RwLock::new(HashMap::new())),
            new_inbox_tx: Arc::new(tx),
        }
    }
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe_new_inboxes(&self) -> broadcast::Receiver<String> {
        self.new_inbox_tx.subscribe()
    }

    pub fn update_peer_inboxes(&self, addr: SocketAddr, peer_name: String, inboxes: Vec<String>) {
        let is_new_peer = {
            let mut names = self.peer_names.write().unwrap();
            match names.entry(addr) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(peer_name.clone());
                    true
                }
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    if e.get() != &peer_name {
                        e.insert(peer_name.clone());
                    }
                    false
                }
            }
        };

        if is_new_peer {
            tracing::info!("Discovered new peer: {} at {}", peer_name, addr);
        }

        let mut table = self.table.write().unwrap();
        let mut peers = self.peers.write().unwrap();

        let new_set: HashSet<String> = inboxes.into_iter().collect();
        let old_set = peers.remove(&addr).unwrap_or_default();

        let mut discovered = Vec::new();

        // Add new ones
        for inbox in &new_set {
            if !old_set.contains(inbox) {
                table.entry(inbox.clone()).or_default().insert(addr);
                discovered.push(inbox.clone());
                tracing::info!("Discovered new inbox '{}' from peer {} ({})", inbox, peer_name, addr);
            }
        }

        // Remove old ones
        for inbox in &old_set {
            if !new_set.contains(inbox) {
                if let Some(set) = table.get_mut(inbox) {
                    set.remove(&addr);
                    if set.is_empty() {
                        table.remove(inbox);
                    }
                }
            }
        }

        peers.insert(addr, new_set);

        // Notify after dropping locks
        drop(table);
        drop(peers);

        for inbox in discovered {
            let _ = self.new_inbox_tx.send(inbox);
        }
    }

    pub fn get_peers_for_inbox(&self, inbox: &str) -> Vec<SocketAddr> {
        let table = self.table.read().unwrap();
        if let Some(addrs) = table.get(inbox) {
            addrs.iter().copied().collect()
        } else {
            Vec::new()
        }
    }
}
