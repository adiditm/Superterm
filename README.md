# Superterm 🚀

> **Pure Native Rust SSH Client & Tunneling Manager for Windows.**
> 100% Rust Murni — **Zero Web Tech**, tanpa HTML, tanpa CSS, tanpa JavaScript, dan tanpa Chromium/WebView.

---

## ✨ Keunggulan Pure Native Rust

- **Kinerja Instan & Ringan**: Konsumsi memori hanya ~25-40 MB RAM (dibandingkan web/electron yang >200 MB).
- **Direct GPU Rendering**: UI di-render langsung ke GPU (DirectX 12 / Vulkan / OpenGL) menggunakan `eframe` & `egui`.
- **Single Native Binary**: Menghasilkan satu file `superterm.exe` mandiri tanpa ketergantungan runtime eksternal.
- **Keamanan Maksimal**: Bebas dari risiko keamanan web (XSS, DOM exploits, Chromium zero-days).

---

## 🛠️ Fitur Fungsional

1. **SSH Interactive Terminal**:
   - Asynchronous SSH v2 (`russh` / `ssh2` + `tokio`).
   - Dukungan otentikasi Password, Private Key (Ed25519, RSA, PEM), Passphrase.
   - PTY window resizing real-time.
2. **SSH Tunneling (Port Forwarding & SOCKS5)**:
   - **Static Local Forwarding (`-L`)**: Akses database remote langsung dari localhost.
   - **Static Remote Forwarding (`-R`)**: Ekspos port lokal ke server remote.
   - **Dynamic SOCKS5 Proxy Server (`-D`)**: Terintegrasi penuh (RFC 1928) dengan switch on/off instan.
3. **Cross-Platform Encrypted Vault**:
   - Enkripsi kuat **AES-256-GCM** dengan derivasi kunci **Argon2id**.
   - Berkas `.stvault` portabel yang kompatibel penuh di Windows, macOS, dan Linux.
4. **Kustomisasi Terminal**:
   - Slider ukuran font dinamis (11px – 24px).
   - Palet warna tema siap pakai (*Dracula*, *Tokyo Night*, *Nord*, *Monokai Pro*, *Cyberpunk*).
5. **Left Sidebar Connection Tree**:
   - Pengelompokan folder (*Production*, *Staging*, *Homelab*), filter pencarian instan, dan status indikator koneksi.
6. **Google E2EE Cloud Sync**:
   - Zero-Knowledge Client-Side Encryption disinkronkan ke **Google Drive AppData**.

---

## 📁 Struktur Kode Sumber Pure Rust

```
Z:\ProjectsX\Experiments\Superterm\
├── Cargo.toml                  # Rust Package Manifest & Dependencies
├── docs/                       # Dokumentasi Teknis
│   ├── ARCHITECTURE.md         # Arsitektur Native Rust Lengkap
│   ├── DATA_SCHEMA.md          # Spesifikasi Skema JSON & Vault
│   └── TUNNELING_GUIDE.md      # Panduan Teknis Port Forwarding & SOCKS5
└── src/                        # 100% Rust Source Code
    ├── main.rs                 # Native Windows Entry Point (eframe::run_native)
    ├── ui/
    │   ├── mod.rs
    │   └── app.rs              # UI Native Rust (Sidebar, Terminal View, Tunnels, Settings)
    └── core/
        ├── mod.rs
        ├── ssh.rs              # Asynchronous SSH Client & PTY Streaming
        ├── tunnel.rs           # Local, Remote, & Dynamic SOCKS5 Proxy Server
        └── vault.rs            # Portable AES-256-GCM + Argon2id Vault Storage
```

---

## ⚡ Cara Kompilasi Menjadi Windows `.exe`

Setelah Rustup terpasang di Windows:

```powershell
cd Z:\ProjectsX\Experiments\Superterm
cargo build --release
```

Berkas binary mandiri `superterm.exe` murni Rust akan terbentuk di:
`target/release/superterm.exe`
