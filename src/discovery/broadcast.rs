use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tracing::{debug, error, info};
use socket2::{Socket, Domain, Type, Protocol};

use crate::core::router::Router;
use crate::core::watcher::LocalInboxes;

const BROADCAST_PORT: u16 = 9082;

#[derive(Serialize, Deserialize, Debug)]
struct AnnouncePayload {
    instance_id: String,
    name: String,
    network: String,
    tcp_port: u16,
    inboxes: Vec<String>,
}

/// Spawns a background task that periodically broadcasts our local inboxes to the LAN
pub fn spawn_broadcaster(local_inboxes: Arc<LocalInboxes>, my_tcp_port: u16, instance_id: String, instance_name: String, my_network: String) {
    tokio::spawn(async move {
        // We use standard socket for sending broadcast
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to bind broadcast sending socket: {}", e);
                return;
            }
        };

        if let Err(e) = socket.set_broadcast(true) {
            error!("Failed to set SO_BROADCAST: {}", e);
            return;
        }

        let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, BROADCAST_PORT));

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            let inboxes: Vec<String> = local_inboxes
                .folders
                .read()
                .unwrap()
                .iter()
                .cloned()
                .collect();

            let payload = AnnouncePayload {
                instance_id: instance_id.clone(),
                name: instance_name.clone(),
                network: my_network.clone(),
                tcp_port: my_tcp_port,
                inboxes
            };

            if let Ok(json) = serde_json::to_string(&payload) {
                if let Err(e) = socket.send_to(json.as_bytes(), broadcast_addr).await {
                    debug!("Broadcast failed: {}", e);
                }
            }
        }
    });
}

/// Spawns a listener that receives LAN broadcasts and updates the Router
pub fn spawn_listener(router: Router, my_instance_id: String, my_network: String) {
    tokio::spawn(async move {
        // Use socket2 to enable SO_REUSEADDR so multiple local processes can bind to the same UDP port
        let socket = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create udp socket: {}", e);
                return;
            }
        };

        if let Err(e) = socket.set_reuse_address(true) {
            error!("Failed to set reuseADDR: {}", e);
        }

        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, BROADCAST_PORT);
        if let Err(e) = socket.bind(&addr.into()) {
            error!("Failed to bind UDP listener on port {}: {}", BROADCAST_PORT, e);
            return;
        }

        let socket: std::net::UdpSocket = socket.into();
        socket.set_nonblocking(true).unwrap();
        let socket = match UdpSocket::from_std(socket) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to convert to tokio socket: {}", e);
                return;
            }
        };

        info!("Listening for LAN broadcasts on UDP port {}", BROADCAST_PORT);

        let mut buf = [0u8; 2048];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    if let Ok(payload) = serde_json::from_slice::<AnnouncePayload>(&buf[..len]) {
                        // Ignore our own broadcasts
                        if payload.instance_id == my_instance_id {
                            continue;
                        }

                        // Ignore broadcasts from other network groups
                        if payload.network != my_network {
                            continue;
                        }

                        // Construct the peer's actual TCP socket address
                        let mut peer_addr = addr;
                        peer_addr.set_port(payload.tcp_port);

                        router.update_peer_inboxes(peer_addr, payload.name, payload.inboxes);
                    }
                }
                Err(e) => {
                    error!("Error receiving from UDP broadcast: {}", e);
                }
            }
        }
    });
}
