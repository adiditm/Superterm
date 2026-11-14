# SUPERTERM — Implementation Roadmap & Execution Plan

Rencana eksekusi pembangunan aplikasi **Superterm** dari fase pondasi hingga siap rilis di Windows, diikuti macOS dan Linux.

---

## Tahapan Milestone (Roadmap)

### Milestone 1: Arsitektur & Scaffold Proyek (Sedang Berjalan)
- [x] Inisialisasi direktori proyek `Z:\ProjectsX\Experiments\Superterm`
- [x] Desain Arsitektur Sistem Lengkap (`docs/ARCHITECTURE.md`)
- [x] Spesifikasi Data Schema & Enkripsi Vault (`docs/DATA_SCHEMA.md`)
- [x] Panduan Teknis SSH Tunneling & SOCKS5 (`docs/TUNNELING_GUIDE.md`)
- [x] Scaffold Manifest Rust (`Cargo.toml`) dan Frontend (`package.json`, Vite, Tailwind, xterm.js)
- [x] Mockup UI Interaktif Standalone (untuk memverifikasi layout, sidebar, terminal xterm, dan tunnel manager sebelum build binary)

### Milestone 2: Persiapan Toolchain & Environment Windows
- [ ] Pemasangan Rust Toolchain (`rustup`, `rustc`, `cargo` x86_64-pc-windows-msvc)
- [ ] Pemasangan Microsoft C++ Build Tools (MSVC Linker & Windows SDK)
- [ ] Konfigurasi WebView2 Runtime & Tauri CLI (`@tauri-apps/cli`)

### Milestone 3: Core SSH Engine & Terminal
- [ ] Implementasi PTY async wrapper di Rust (`russh` + `tokio::sync::mpsc`)
- [ ] Integrasi `xterm.js` dengan WebGL Addon & Fit Addon
- [ ] Komunikasi bidirectional Tauri IPC Event: Keystrokes -> Rust SSH stdin; SSH stdout -> xterm display
- [ ] Penanganan Terminal Resize (`cols`, `rows`) real-time
- [ ] Otentikasi: Password, Private Key (PEM, OpenSSH, Ed25519), dan Passphrase

### Milestone 4: SSH Tunneling Engine (Local, Remote, SOCKS5)
- [ ] Implementasi Static Local Forwarding (`-L`) dengan `TcpListener` lokal
- [ ] Implementasi Static Remote Forwarding (`-R`) dengan `tcpip-forward` request
- [ ] Implementasi Dynamic SOCKS5 Proxy Server (`-D`) kompatibel RFC 1928
- [ ] UI Port Forwarding Dashboard (Live Toggle, Port Inspector, Byte Counter)

### Milestone 5: Cross-Platform Encrypted Vault
- [ ] Modul Rust `crypto.rs` (Argon2id Key Derivation + AES-256-GCM)
- [ ] Penyimpanan lokal portable `.stvault` (bebas registry Windows, 100% kompatibel Mac & Linux)
- [ ] CRUD Profile Host, Folder Grouping, Tagging, dan Pencarian Cepat di Left Sidebar
- [ ] Setting kustomisasi font, ukuran, line-height, dan palet warna tema

### Milestone 6: Google OAuth & E2EE Cloud Sync
- [ ] Rust OAuth 2.0 PKCE Flow dengan local HTTP loopback listener (`http://127.0.0.1:<port>/callback`)
- [ ] Integrasi Google Drive `drive.appdata` REST API (Zero-server architecture)
- [ ] Mekanisme sinkronisasi berkas vault terenkripsi (E2EE) antar laptop
- [ ] Deteksi konflik revisi dan merger profil otomatis

### Milestone 7: Build, Packaging & Rilis Windows
- [ ] Compile release optimized executable Windows (`superterm.exe`)
- [ ] Pembuatan Installer MSI / NSIS Installer
- [ ] Testing pada monitor Full HD (1080p), 2K, 4K, dan Ultrawide
- [ ] Persiapan build cross-platform untuk macOS (.dmg, Universal Binary) dan Linux (.AppImage, .deb).
