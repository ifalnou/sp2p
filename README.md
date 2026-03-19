# sp2p

`sp2p` (Simple P2P) is a lightweight, background Windows service enabling zero-configuration peer-to-peer file transfer via your filesystem. It treats "folders as an API": simply drop a file into a specific folder, and `sp2p` automatically discovers peers and transfers it to the same folder on the receiving machines.

No sign-ups, no central servers, and no complicated setups.

## Features

- **"Files as API"**: Intuitive mechanics. Copy files to a `send/<category>` folder, and they automatically appear in the peer's `inbox/<category>` folder.
- **Zero-Config Discovery**: Uses local network UDP broadcast to naturally discover peers.
- **End-to-End Encryption (E2EE)**: Zero-config network encryption (Noise Protocol for TCP transfers, nonced ChaCha20Poly1305 for UDP discovery). Secured by a shared `password` via Argon2 key derivation. Untrusted connections are categorically isolated.
- **Nested Folder Mapping**: Transfers preserve folder hierarchy, perfectly mirroring your nested directories on the target peer.
- **UPnP / Port Forwarding**: Automatically maps the necessary ports on your router to allow sending and receiving files across the internet.
- **WAN & Off-Grid Settings**: Explicitly list IP addresses in `config.toml` to connect directly outside your LAN, or disable LAN discovery altogether via command-line arguments.
- **Background Execution**: Sits quietly in the Windows System Tray with near-zero CPU footprint. No persistent terminal required.
- **Dashboard & Notifications**: Includes a native interface accessible from the tray to view real-time progress, transfer history, and toggle Windows auto-start. Integrated OS toast notifications gracefully alert you to new file arrivals.
- **Resumable & Verified Transfers**: Implements offset tracking to seamlessly resume interrupted large downloads. Ensures bit-perfect data integrity using ultra-fast BLAKE3 hashing.
- **Conflict Resolution**: Files sent to an inbox category claimed by multiple peers will transfer to all matched network endpoints.

## Installation

Download the `sp2p` binary from the Release page.
It requires Windows natively. Place the binary into its own dedicated folder. Example: `C:\tools\sp2p\sp2p.exe`.
Double click it to launch. It will generate the `inbox`, `send`, and `config.toml` structures automatically.

## How It Works Elements

Once started:
1. Two main directories (`inbox` and `send`) appear beside the executable.
2. In your `inbox/` folder, create subdirectories for things you want to receive (e.g. `inbox/photos/`). The folder name acts as your identifier or "channel".
3. When another peer running `sp2p` copies files into `send/photos/`, the service detects the drop, handles file locks, instantly locates your peer via discovery, and transfers the data stream.
4. Finished payloads arrive directly within your `inbox/photos/` without user intervention.

## Command-Line Arguments

Run `sp2p.exe --help` to see all available overrides:

```bash
sp2p 0.1.0
Simple P2P File Transfer via Folders

USAGE:
    sp2p.exe [OPTIONS]

OPTIONS:
    --dir <DIR>         Sets a custom working directory (default is executable's folder)
    --name <NAME>       Set an explicit peer name (defaults to 'Peer-<random>')
    --port <PORT>       TCP transfer port (default: 9081)
    --network <NET>     Network isolation ID for discovering peers (default: 'default')
    --debug             Allocate a console window to display detailed logs
    --no-upnp           Skip UPnP port forwarding via IGD
    --no-lan            Disable UDP network broadcasting and rely explicitly on configured IPs
    --no-tray           Disable Windows System Tray icon
```

## Configuration

A `config.toml` will be automatically generated upon your first launch. It allows you to specify explicit IPs for reaching peers globally or across complicated subnets where UDP broadcasts fail, and establish the E2EE password.

```toml
# Network password used for Pre-Shared Key Derivation (E2EE)
# All peers sharing files must have this matching password
password = "my-secret-password"

# Explicit manual targets (useful for WAN endpoints or remote machines)
peers = [
    "11.22.33.44"
]
```

## Building From Source

```bash
cargo build --release
```

## Roadmap

Check out the [ROADMAP.md](ROADMAP.md) for details on current progress and future objectives like advanced NAT Traversal and WAN routing.
