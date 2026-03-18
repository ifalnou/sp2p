#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod discovery;
mod transfer;

use clap::Parser;
use std::sync::Arc;
use tokio::sync::mpsc;
use tray_item::{IconSource, TrayItem};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::{FormatEvent, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing::Event;

struct CustomFormatter {
    name: String,
}

impl<S, N> FormatEvent<S, N> for CustomFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        // Format: [name] LEVEL message...
        // This drops the timestamp and the target namespace
        write!(writer, "[{}] {} ", self.name, meta.level())?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Console::AllocConsole;

use crate::core::dirs::AppDirs;
use crate::core::router::Router;
use crate::core::watcher::{LocalInboxes, watch_inbox_directory, watch_send_directory};
use crate::discovery::broadcast::{spawn_broadcaster, spawn_listener};
use crate::discovery::upnp::forward_port;
use crate::core::config::Config;
use crate::transfer::server::spawn_server;

#[derive(Parser, Debug)]
#[command(name = "sp2p")]
#[command(about = "Simple P2P file transfer", long_about = None)]
struct Args {
    /// Enable debug logging and show the console window
    #[arg(short, long)]
    debug: bool,

    /// Instance name for logging (defaults to port if not set)
    #[arg(short, long)]
    name: Option<String>,

    /// Network group name to isolate peers (defaults to "default")
    #[arg(long, default_value = "default")]
    network: String,

    /// TCP port for file transfers (defaults to 9081)
    #[arg(short, long, default_value_t = 9081)]
    port: u16,

    /// Override the root directory for inbox/send folders (defaults to executable dir)
    #[arg(long)]
    dir: Option<std::path::PathBuf>,

    /// Disable UPnP port forwarding
    #[arg(long)]
    no_upnp: bool,

    /// Disable LAN broadcast discovery
    #[arg(long)]
    no_lan: bool,

    /// Disable System Tray UI
    #[arg(long)]
    no_tray: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.debug {
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = AllocConsole();
        }
    }

    let instance_name = args.name.unwrap_or_else(|| args.port.to_string());

    let env_filter = EnvFilter::new(if args.debug { "debug" } else { "info" });
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .event_format(CustomFormatter { name: instance_name.clone() })
        .init();

    info!("Starting sp2p...");

    // 1. Setup Directories
    let dirs = match AppDirs::init(args.dir) {
        Ok(d) => d,
        Err(e) => {
            error!("Failed to setup directories: {}", e);
            return;
        }
    };

    // 2. Load Config
    let config_path = dirs.root.join("config.toml");
    let config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {}", e);
            return;
        }
    };
    info!("Loaded config: {:?}", config);

    // 3. Initialize Shared State
    let router = Router::new();
    let local_inboxes = Arc::new(LocalInboxes::new());

    // Pre-Shared Group Password / Key derivation
    let password = config.password.clone().unwrap_or_else(|| "sp2p-default-net".to_string());
    let crypto_key = Arc::new(crate::core::crypto::derive_key(&password));
    info!("Network encryption initialized.");

    // Setup static peers from config
    for peer in &config.peers {
        if let Ok(ip) = peer.parse::<std::net::IpAddr>() {
            // Static peers can be pre-seeded, or we might need to actively poll them.
            // For now, they are just parsed. They will be integrated in TCP logic.
            debug!("Loaded static peer: {}", ip);
        }
    }

    // 4. Start Core Modules

    // Initial scan of our current inbox folders
    local_inboxes.scan_initial(&dirs.inbox);

    // Watch for new/deleted inbox folders
    watch_inbox_directory(dirs.inbox.clone(), local_inboxes.clone());

    // Watch the send folder to dispatch files
    watch_send_directory(dirs.send.clone(), router.clone(), crypto_key.clone());

    // 5. Start TCP File Acceptance Server
    spawn_server(args.port, dirs.inbox.clone(), crypto_key.clone());

    // Generate a unique instance ID for broadcast loopback avoidance
    let instance_id = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

    // 6. Start Discovery (UDP Broadcasts)
    spawn_broadcaster(
        local_inboxes.clone(),
        args.port,
        instance_id.clone(),
        instance_name.clone(),
        args.network.clone(),
        args.no_lan,
        config.peers.clone(),
        crypto_key.clone()
    );
    spawn_listener(router.clone(), instance_id, args.network, crypto_key.clone());

    // 7. Request UPnP Port Forwarding async (unless disabled)
    if !args.no_upnp {
        forward_port(args.port);
    }

    // 8. Setup System Tray
    let (tx, mut rx) = mpsc::unbounded_channel();

    // We only need to hold the tray to prevent it from dropping
    let _tray = if !args.no_tray {
        match TrayItem::new("sp2p", IconSource::Resource("app-icon")) {
            Ok(mut tray) => {
                let quit_tx = tx.clone();
                tray.add_menu_item("Quit", move || {
                    let _ = quit_tx.send(());
                }).unwrap_or_else(|e| tracing::warn!("Failed to add quit menu: {}", e));
                Some(tray)
            },
            Err(e) => {
                tracing::warn!("Failed to create system tray: {}", e);
                None
            }
        }
    } else {
        None
    };

    info!("sp2p initialized successfully on port {}. Standing by.", args.port);

    // Wait for graceful shutdown (Ctrl+C OR Tray Icon Quit)
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down.");
        }
        _ = rx.recv() => {
            info!("Quit requested via System Tray, shutting down.");
        }
    }
}

