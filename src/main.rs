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

    /// TCP port for file transfers (defaults to 9081)
    #[arg(short, long, default_value_t = 9081)]
    port: u16,

    /// Override the root directory for inbox/send folders (defaults to executable dir)
    #[arg(long)]
    dir: Option<std::path::PathBuf>,

    /// Disable UPnP port forwarding
    #[arg(long)]
    no_upnp: bool,

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

    let env_filter = EnvFilter::new(if args.debug { "debug" } else { "info" });
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
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

    // Setup static peers from config
    for peer in config.peers {
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
    watch_send_directory(dirs.send.clone(), router.clone());

    // 5. Start TCP File Acceptance Server
    spawn_server(args.port, dirs.inbox.clone());

    // 6. Start Discovery (UDP Broadcasts)
    spawn_broadcaster(local_inboxes, args.port);
    spawn_listener(router.clone());

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

