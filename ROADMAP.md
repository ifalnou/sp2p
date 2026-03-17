# sp2p: Simple P2P - Development Roadmap

## 1. Project Overview
**sp2p** is a lightweight, background service enabling peer-to-peer file transfer via filesystem operations. It adheres to the "files as API" philosophy—users send files by copying them to a specific folder, and receive files in another.

## 2. Core Constraints & Requirements
- **Language**: Rust (Safe, highly performant).
- **Target Platform**: **Windows ONLY**.
- **Resource Usage**:
  - Optimized for binary size.
  - Minimal RAM footprint.
  - Near-zero CPU usage when idle (event-driven, not polling).
- **OS Integration**:
  - Windows System Tray support (Icon + Right-click > Quit).
  - Hidden console window by default (`#![windows_subsystem = "windows"]`).
  - CLI argument `--debug` to attach/allocate a visible console for logging.

## 3. Functional Specification

### 3.1 Folder Structure
The application manages a root directory (e.g., alongside the executable):
```
/app_folder
├── /config.toml       # User configuration (static peer IPs, settings)
├── /inbox             # User creates subfolders here to define their "inboxes"
│   ├── /photos        # Advertises the "photos" inbox to the network
│   └── /work_docs     # Advertises the "work_docs" inbox to the network
├── /send              # Outgoing queue
│   ├── /photos        # Files dropped here are sent to peers broadcasting a "photos" inbox
│   └── /work_docs     # Files dropped here map to peers with the "work_docs" inbox
└── /sent              # (Optional) Files are moved here after success
```

### 3.2 Networking & Discovery
- **Concept**: Peers are fundamentally represented merely by their IPs. There are no human-readable peer identifiers (like "alice" or "bob"). Instead, identities are represented by *Inbox names*. Which are dynamically deduced from subfolders inside the user's `inbox/` directory.
- **Port:** Default TCP port `9081` for handling file transfers.
- **LAN Discovery Technology**: **UDP Broadcast**.
  - *Why not mDNS/Bonjour?* Standard Windows does not ship with a native mDNS responder guaranteed to be running without extra software (like iTunes/Bonjour), and complex mDNS crates add large dependencies. A lightweight **IPv4 UDP Broadcast** (e.g. on port `9082`) is natively supported, zero-dependency, and extremely reliable for simple local network discovery.
  - Each peer periodically emits a UDP blast containing: `[Control Port (9081), Available Inboxes: ["photos", "work_docs"]]`.
- **Manual / WAN Peers**: `config.toml` allows defining explicit IPs (e.g., `peers = ["192.168.2.5", "203.0.113.10"]`) outside the broadcast domain.
- **UPnP**: Implemented to automatically map port `9081` on local-network routers via SSDP to allow incoming file transfers from non-local networks.

### 3.3 File Transfer Logic
1.  **Watcher**: Watch the `app_folder/send/` directory and its subdirectories using Windows-native events (`ReadDirectoryChangesW` via the `notify` crate).
2.  **Debounce**: Ensure the file handle is released by the operating system (meaning the user has finished writing/copying) before starting the transfer.
3.  **Routing**: When a file is dropped in `send/photos`, check the internal routing table for any IP that last advertised possessing a `photos` inbox. Initiate a TCP connection to `IP:9081` to stream the payload.
4.  **Cleanup**: Upon successful network acknowledgment, remove the file from `send/` (or move to `sent/`).

## 4. Proposed Roadmap Phases

### Phase 1: The Skeleton
- [ ] Initialize Windows-targeted Rust project.
- [ ] Implement `clap` for argument parsing (`--debug`).
- [ ] Add conditional console allocation logic for Windows (hide naturally, show on `--debug`).
- [ ] Setup default folder creation on startup (`/inbox`, `/send`).

### Phase 2: Core Routing & Discovery
- [ ] Dynamic inbox detection: Watch the `/inbox` folder for directory creation/deletion.
- [ ] Implement UDP broadcast loop on port `9082`.
- [ ] Maintain an in-memory thread-safe routing table `Inbox Name -> List of IPs`.
- [ ] Poll or parse `config.toml` for static IP fallback list.

### Phase 3: File Transfer & System Tray
- [ ] Implement Windows-native scalable file system watcher (`notify`).
- [ ] Build the file streaming TCP server/client (Port `9081`).
- [ ] Wire up Windows System Tray (Icon, Menu, Quit).

### Phase 4: Port Forwarding & Edge Cases
- [ ] Integrate UPnP (e.g., the `igd` crate) to auto-forward port `9081` on routers.
- [ ] Handle routing conflicts (e.g. if two IPs claim the same inbox name – send to both).
- [ ] Prevent file locking issues during transfer.
