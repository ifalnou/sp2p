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

## 4. Completed Milestones

### Phase 1: The Skeleton
- [x] Initialize Windows-targeted Rust project.
- [x] Implement `clap` for CLI argument parsing.
- [x] Add conditional console allocation logic for Windows (hide naturally, show on `--debug`).
- [x] Setup default folder creation on startup (`/inbox`, `/send`).

### Phase 2: Core Routing & Discovery
- [x] Dynamic inbox detection: Watch the `/inbox` folder for directory creation/deletion.
- [x] Implement UDP broadcast loop on port `9082`.
- [x] Maintain an in-memory thread-safe routing table `Inbox Name -> List of IPs`.
- [x] Parse `config.toml` for static IP fallback list & explicit peers mapping.

### Phase 3: File Transfer & System Tray
- [x] Implement Windows-native scalable file system watcher (`notify`).
- [x] Build the file streaming TCP server/client (Port `9081`).
- [x] Wire up Windows System Tray (Icon, Menu, Quit).
- [x] Handle nested folder transfers faithfully recreating the tree on the receiving end.

### Phase 4: Port Forwarding & Advanced Networking
- [x] Integrate UPnP (via `igd` crate) to auto-forward TCP `9081` and UDP `9082` on routers.
- [x] Handle routing conflicts (send to all matching IPs broadcasting a matching inbox).
- [x] Prevent file locking issues during transfer via proper debouncing.
- [x] Allow running disconnected from LAN using explicit unicast (`--no-lan`).

### Phase 5: Security & Privacy
- [x] **End-to-End Encryption (E2EE):** TCP streams are symmetrically encrypted via the Noise Protocol framework (`snow`).
- [x] **Stateless Payload Encryption:** UDP discovery blasts are entirely wrapped and nonced using `ChaCha20Poly1305`.
- [x] **Network Group Passwords:** Seamless Pre-Shared Keys generated from user passwords via `Argon2`. Unauthenticated noise is aggressively dropped.

### Phase 6: User Experience (UX) - *Completed*
- [x] **OS Notifications:** Integrate desktop toast notifications (`notify-rust`) intelligently grouped to alert the user when files successfully arrive in their inbox.
- [x] **Lightweight Native UI / Progress:** Created a native `egui` window accessible from the System Tray to display real-time progress bars for active transfers, history, and settings (Auto-start Windows registry hooking).

### Phase 7: Reliability & Large Files - *Completed*
- [x] **Chunking & Resume Support:** Instead of restarting dropped transfers from 0%, allow peers to negotiate file offsets and resume appending to partially downloaded data (crucial for large transfers).
- [x] **File Hash Integrity:** Calculate a fast hash (`BLAKE3`) during transfer and verify it at the end to guarantee data isn't corrupted over the wire.
*Note: Intentional omission of bandwidth limiting. The design philosophy favors keeping it simple and always utilizing the maximum available bandwidth.*

## 5. Future Improvements & Next Steps

Based on initial testing and usage, the following features have been identified as high-value for future iterations:

### 5.1 Advanced Networking (Pending Evaluation)
- **NAT Traversal (Hole Punching) & Relay Fallback:** Deferred for now. UPnP covers basic WAN scenarios. If extended usage and testing indicate strict NAT/UPnP failure is a common bottleneck, STUN/TURN based UDP hole punching or fallback relays will be reconsidered.
