use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

#[derive(Default, Clone)]
pub struct Router {
    // Maps an inbox name to a set of IP:Port addresses that advertise it.
    table: Arc<RwLock<HashMap<String, HashSet<SocketAddr>>>>,
    // Maps IP:Port address to a set of inboxes they own
    peers: Arc<RwLock<HashMap<SocketAddr, HashSet<String>>>>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_peer_inboxes(&self, addr: SocketAddr, inboxes: Vec<String>) {
        let mut table = self.table.write().unwrap();
        let mut peers = self.peers.write().unwrap();

        let new_set: HashSet<String> = inboxes.into_iter().collect();
        let mut old_set = peers.remove(&addr).unwrap_or_default();

        // Add new ones
        for inbox in &new_set {
            if !old_set.contains(inbox) {
                table.entry(inbox.clone()).or_default().insert(addr);
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
