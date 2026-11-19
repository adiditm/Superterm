# Superterm

**English** | [Bahasa Indonesia](README.id.md)

> **Native SSH client, tunneling manager, and network toolbox for Windows, macOS, & Linux.**
> 100% Pure Rust - zero HTML, CSS, JavaScript, Chromium, or WebView.

## Download

Standalone binaries ready to run are available on [GitHub Releases](https://github.com/adiditm/Superterm/releases/latest):

| Platform | Architecture | Package | Description |
|---|---|---|---|
| **macOS** | Apple Silicon | [Superterm-macos-arm64.zip](https://github.com/adiditm/Superterm/releases/latest) | macOS 11.0+ |
| **macOS** | Intel | [Superterm-macos-x86_64.zip](https://github.com/adiditm/Superterm/releases/latest) | macOS 15.0+ |
| **Linux** | ARM64 | [Superterm-linux-arm64.tar.gz](https://github.com/adiditm/Superterm/releases/latest) | glibc 2.17+ |
| **Linux** | Intel/AMD64 | [Superterm-linux-x86_64.tar.gz](https://github.com/adiditm/Superterm/releases/latest) | glibc 2.17+ |
| **Windows** | x86_64 (64-bit) | [Superterm-windows-x86_64.zip](https://github.com/adiditm/Superterm/releases/latest) | Windows 10/11 |
| **Windows** | ARM64 | [Superterm-windows-arm64.zip](https://github.com/adiditm/Superterm/releases/latest) | Windows 10/11 ARM64 |

*No installation required. Just extract and run.*

---

## Highlights

- **Native, single file**: Build produces a standalone executable (~5 MB) with no external runtimes or framework dependencies.
- **Direct GPU rendering**: UI rendered via OpenGL (`eframe` / `egui`, with `glow` backend).
- **Zero web attack surface**: No DOM, no XSS vectors, no embedded browser engine.

---

## Features

### 1. Hosts & SSH
- Interactive SSH v2 terminal (`russh` + `tokio`) with custom VT100 emulator, color support, scrollback, real-time PTY resizing, and select / copy / paste.
- Authentication via **password** and **private key** (Ed25519, RSA, ECDSA; passphrase supported).
- Multi-tab support: open multiple tabs for a single host (**Connect in new tab**).
- Foldered sidebar: categorize connections in folders. Rename and delete via right-click (deletion only allowed on empty folders). Host editor offers existing folders via dropdown or new custom folder creation.
- Host menu (right-click or `...` button): Connect, Connect in new tab, **Disconnect** (single tab), **Disconnect All Tabs**, Edit, Duplicate, Rename, Export Connection (JSON), Delete.
- Tabbed host editor: *Connection* and *Advanced* configurations.

### 2. SSH Tunneling
- **Local** (`-L`), **Remote** (`-R`), and **Dynamic SOCKS5** (`-D`, RFC 1928).
- **Auto-ON** tunnels automatically start upon host connection and terminate when the last session tab for that host is closed.
- *Port Forwarding & Proxy* panel: Start/Stop individual tunnels on-the-fly without dropping the SSH session, with live byte counters.

### 3. Local Shell
- **PowerShell** and **CMD** on Windows, **Terminal** (login shell) on macOS/Linux, running on real PTYs (ConPTY on Windows).
- **Run as Administrator / Root** option (Superterm relaunches with elevated privileges). Elevated tabs are distinctively badged `(Administrator)` / `(Root)`.

### 4. Tools
| Tool | Description |
|---|---|
| Ping, Traceroute, Port Scanner | Network diagnostics & troubleshooting |
| DNS Lookup, DNS Whois | Domain intelligence including ccTLD |
| Network Interfaces | Local adapter & IP address listing |
| SSH Key Generator | Generate Ed25519 / ECDSA-P256 keys directly to `~/.ssh` |
| Password Vault | View and copy saved host credentials |
| **SQL Client** | MySQL & PostgreSQL, multi-session, tabbed results |
| **Note Vault** | Multi-tab notes persisted inside vault |

**SQL Client**
- Saved connections in vault; host/port can auto-fill from an active local tunnel via **Use tunnel...**
- **TLS** via operating system security layer (SChannel on Windows, Security.framework on macOS, native-tls/OpenSSL on Linux), mode `prefer`: SSL is used if offered by server (essential for servers enforcing `hostssl` in `pg_hba.conf`).
- Each execution opens a **new result tab**. Tab caption reflects the first queried table; right-click to Copy / Edit / Re-run. SELECT queries display as interactive data grids (up to 1,000 rows); INSERT/UPDATE/DELETE report affected row counts.
- Multiple database connections can run concurrently as independent tabs (**New SQL session**).

### 5. Text Input
All text fields in the application provide right-click context menus with **Cut / Copy / Paste / Select All**. Password inputs safely restrict actions to Paste and Select All unless the reveal password toggle is enabled.

### 6. Vault & Sync
- All application data (hosts, tunnels, notes, SQL connections, preferences) is stored in a single local vault file.
- **Sync** = Export and import encrypted `.stvault` files (**Argon2id + AES-256-GCM**) to any directory of your choice (e.g. Google Drive, OneDrive, or local backup drives).

### 7. Appearance & Customization
- Night / Day / Auto modes (respects system preferences).
- Terminal themes: Dracula, Tokyo Night, Nord, Monokai Pro, Cyberpunk, Daylight Clean, Solarized Light.
- Configurable terminal font size (11-24 px) and global UI zoom scaling.

---

## Data Storage Locations

| OS | Path |
|---|---|
| Windows | `%APPDATA%\Superterm\superterm.vault` |
| macOS | `~/Library/Application Support/Superterm/superterm.vault` |
| Linux | `~/.config/Superterm/superterm.vault` |

---

## Building from Source

```bash
cargo build --release
```

Binary outputs: `target/release/superterm.exe` (Windows) or `target/release/superterm` (macOS/Linux).

---

## Source Tree Overview

```
Superterm/
|-- Cargo.toml
|-- build.rs / app.rc / app.ico      # Windows metadata & application icons
|-- docs/
|   |-- ARCHITECTURE.md
|   |-- DATA_SCHEMA.md
|   |-- ROADMAP.md
|   `-- TUNNELING_GUIDE.md
`-- src/
    |-- main.rs                      # Application entry point (eframe::run_native)
    |-- core/
    |   |-- ssh.rs                   # Async SSH sessions & PTY streaming
    |   |-- tunnel.rs                # Local / Remote / SOCKS5 proxy engine
    |   |-- localshell.rs            # PowerShell / CMD / Terminal in native PTY
    |   |-- sqlclient.rs             # MySQL & PostgreSQL driver integration (sqlx)
    |   |-- tools.rs                 # Ping, DNS, Whois, port scan, keygen
    |   `-- vault.rs                 # Local vault storage & encrypted .stvault import/export
    `-- ui/
        |-- app.rs                   # Main UI shell: sidebar, tabs, modals, menus
        |-- terminal.rs              # VT100 terminal emulator & UI renderer
        `-- tools_view.rs            # Diagnostics UI, SQL Client, & Note Vault
```

Refer to [`docs/`](docs/) for detailed architecture, data schema, and tunneling documentation.
