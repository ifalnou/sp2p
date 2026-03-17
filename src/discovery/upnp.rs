use igd::{search_gateway, PortMappingProtocol};
use local_ip_address::local_ip;
use std::net::{IpAddr, SocketAddrV4};
use tracing::{info, warn};

pub fn forward_port(tcp_port: u16) {
    // UPnP operations can block on network timeouts, so we put it on a standard thread
    std::thread::spawn(move || {
        let my_local_ip = match local_ip() {
            Ok(IpAddr::V4(ipv4)) => {
                info!("UPnP: Discovered local IP: {}", ipv4);
                ipv4
            },
            Ok(IpAddr::V6(_)) => {
                warn!("UPnP: Cannot map IPv6 local address");
                return;
            }
            Err(e) => {
                warn!("UPnP: Could not determine local IP address: {}", e);
                return;
            }
        };

        info!("UPnP: Searching for IGD gateway on the network...");

        let mut options = igd::SearchOptions::default();
        options.bind_addr = std::net::SocketAddr::V4(SocketAddrV4::new(my_local_ip, 0));
        options.timeout = Some(std::time::Duration::from_secs(15));

        match search_gateway(options) {
            Ok(gateway) => {
                let local_addr = SocketAddrV4::new(my_local_ip, tcp_port);

                // We ask the router to map the external port matching our internal tcp_port to us
                match gateway.add_port(
                    PortMappingProtocol::TCP,
                    tcp_port,       // External port
                    local_addr,     // Internal IP:Port
                    60 * 60 * 24,   // 24 hours lease duration
                    "sp2p app",     // Description on the router's UI
                ) {
                    Ok(_) => info!(
                        "UPnP: Successfully mapped external port {} to {}",
                        tcp_port, local_addr
                    ),
                    Err(e) => warn!("UPnP: Failed to map port: {}", e),
                }
            }
            Err(e) => {
                warn!("UPnP: Gateway not found or didn't respond ({}). This is normal if you're not behind a UPnP-enabled router.", e);
            }
        }
    });
}
