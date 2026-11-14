# SUPERTERM — Pure Native Rust Architecture (Zero Web Tech)

> **Superterm** adalah aplikasi Windows Desktop murni berbasis **100% RUST** (tanpa HTML, tanpa CSS, tanpa JavaScript, dan tanpa Chromium/WebView).

---

## 1. Filosofi: Pure Native Rust vs Web Tech

| Parameter | Pure Native Rust (Superterm) | Web Tech / Electron / WebView |
|---|---|---|
| **Bahasa Pemrograman** | **100% Rust Murni** | JavaScript, HTML, CSS + Wrapper |
| **Rendering Engine** | **DirectX 12 / Vulkan via `wgpu` / `glow`** | Chromium Blink / WebKit |
| **Konsumsi RAM** | **~25 MB - 40 MB** (Sangat Ringan) | > 150 MB - 500 MB (Boros Memori) |
| **Startup Time** | **< 0.1 detik (Instant)** | 1.5 - 3 detik |
| **Ukuran Binary** | **Single Executable `.exe` Mandiri** | Butuh runtime browser atau Node.js |
| **Keamanan** | Memory-Safe, bebas CVE XSS/Web | Rawan celah web & dependency bloat |

---

## 2. Diagram Arsitektur Pure Native Rust

```
+-------------------------------------------------------------------------------+
|                       SUPERTERM NATIVE WINDOWS APP                            |
|             (eframe / egui — Immediate Mode Native Rust GUI)                   |
|                                                                               |
|  +--------------------+  +-------------------------------------------------+  |
|  |  LEFT SIDEBAR PANE |  |            TABS & STATUS BAR                    |  |
|  |                    |  |  [AWS Web Cluster 01] [● 24ms Connected]        |  |
|  |  [Search Filter]   |  +-------------------------------------------------+  |
|  |  [+ Add Host]      |  |                                                 |  |
|  |                    |  |             NATIVE TERMINAL VIEWPORT            |  |
|  |  v Production      |  |  ubuntu@aws-web-prod:~$ uname -smr              |  |
|  |    ● AWS-01 (SSH)  |  |  Linux 6.8.0-45-generic x86_64                  |  |
|  |  v Staging         |  |  (GPU-Accelerated Direct Pixel Rasterizer)      |  |
|  |    ● API Gateway   |  |                                                 |  |
|  |  v Homelab         |  +-------------------------------------------------+  |
|  |    ● Proxmox       |  |           PORT FORWARDING DRAWER (RIGHT)        |  |
|  |                    |  |  * [LOCAL]  :3307 -> 10.0.2.14:3306 [ACTIVE]    |  |
|  |  [Google Sync E2EE]|  |  * [SOCKS5] :1080 Dynamic Proxy     [READY]     |  |
|  +--------------------+  +-------------------------------------------------+  |
+---------------------------------------+---------------------------------------+
                                        | Pure Rust In-Memory Channels (mpsc)
                                        v
+-------------------------------------------------------------------------------+
|                      RUST CORE ASYNC TOKIO RUNTIME                            |
|                                                                               |
|  +--------------------------+  +-------------------------------------------+  |
|  |     SSH & PTY ENGINE     |  |           TUNNELING & PROXY ENGINE        |  |
|  |  - russh / ssh2          |  |  - Static Local Forwarding (-L)           |  |
|  |  - Ed25519, RSA, PEM     |  |  - Static Remote Forwarding (-R)          |  |
|  |  - Tokio PTY Async Stream|  |  - Dynamic SOCKS5 Proxy Server (-D)       |  |
|  +--------------------------+  +-------------------------------------------+  |
|                                                                               |
|  +--------------------------+  +-------------------------------------------+  |
|  |    ENCRYPTED VAULT       |  |           GOOGLE E2EE CLOUD SYNC          |  |
|  |  - AES-256-GCM           |  |  - OAuth 2.0 PKCE Loopback Receiver       |  |
|  |  - Argon2id Key Deriv    |  |  - Google Drive AppData Storage           |  |
|  |  - Portable .stvault     |  |  - Zero-Knowledge Client-Side Encryption  |  |
|  +--------------------------+  +-------------------------------------------+  |
+-------------------------------------------------------------------------------+
```

---

## 3. Struktur Modul Kode Sumber Rust

```
Z:\ProjectsX\Experiments\Superterm\
├── Cargo.toml                  # Konfigurasi Rust murni & dependensi (eframe, egui, tokio, russh)
├── src/
│   ├── main.rs                 # Entry point aplikasi native Windows (eframe::run_native)
│   ├── ui/
│   │   ├── mod.rs
│   │   └── app.rs              # UI Superterm 100% Rust (Sidebar, Terminal, Tunnels, Settings)
│   └── core/
│       ├── mod.rs
│       ├── ssh.rs              # Asynchronous SSH client & channel streaming
│       ├── tunnel.rs           # Local, Remote, & Dynamic SOCKS5 Proxy Server (RFC 1928)
│       └── vault.rs            # Portable Encrypted Storage (AES-256-GCM + Argon2id)
```

---

## 4. Cara Mengompilasi Menjadi Native Windows `.exe`

Cukup jalankan satu perintah standar Cargo:

```powershell
cd Z:\ProjectsX\Experiments\Superterm
cargo build --release
```

Binary native Windows `superterm.exe` murni tanpa ketergantungan web akan langsung dihasilkan di:
`Z:\ProjectsX\Experiments\Superterm	arget\release\superterm.exe`
