# SUPERTERM — Roadmap

Status per November 2026. Arah awal berbasis Tauri / WebView / xterm.js **ditinggalkan** dan diganti aplikasi 100% native Rust (`eframe` / `egui`).

---

## Selesai

### Fondasi
- [x] Aplikasi native Rust (`eframe` / `egui`, backend OpenGL), tanpa web tech
- [x] Toolchain `x86_64-pc-windows-gnu`, build release ~5 MB dengan ikon tersemat
- [x] GitHub Actions: build Windows dan macOS (Apple Silicon & Intel)
- [x] Shutdown bersih (`on_exit`) agar tidak menggantung saat ditutup di macOS

### SSH & Terminal
- [x] SSH v2 async (`russh`), autentikasi password & private key
- [x] Emulator VT100 + renderer sendiri, warna, scrollback, select / copy / paste
- [x] Multi-tab per host (Connect in new tab), tab otomatis tertutup saat sesi SSH selesai
- [x] Menu host: Connect, Connect in new tab, Disconnect (satu tab), Disconnect All Tabs, Edit, Duplicate, Rename, Export Connection, Delete

### Tunneling
- [x] Local (`-L`), Remote (`-R`), Dynamic SOCKS5 (`-D`)
- [x] Auto-ON saat host terhubung, berhenti saat tab terakhir host ditutup
- [x] Panel Port Forwarding: Start/Stop per tunnel, penghitung byte

### Host management
- [x] Sidebar berfolder; rename folder, hapus folder kosong
- [x] Editor host bertab (Connection / Advanced); dropdown folder + input folder baru

### Local Shell
- [x] PowerShell / CMD (Windows), Terminal (macOS/Linux) di PTY sungguhan
- [x] Balasan otomatis untuk query DSR / DA dari ConPTY
- [x] Run as Administrator / Root

### Tools
- [x] Ping, Traceroute, Port Scanner, DNS Lookup, DNS Whois, Network Interfaces
- [x] SSH Key Generator, Password Vault
- [x] SQL Client: MySQL & PostgreSQL, koneksi tersimpan, pilih tunnel, TLS (SChannel / Security.framework), hasil bertab dengan caption nama tabel, Edit / Re-run query, multi-sesi
- [x] Note Vault: catatan multi-tab, tersimpan di vault, ikut ekspor `.stvault`
- [x] Menu Cut / Copy / Paste / Select All di semua input teks

### Vault & Sync
- [x] Vault lokal (JSON) dengan default yang kompatibel ke belakang
- [x] Ekspor/impor `.stvault` (Argon2id + AES-256-GCM)

---

## Berikutnya

### Keamanan (prioritas)
- [ ] Enkripsi `superterm.vault` di disk (sekarang JSON biasa, termasuk password)
- [ ] Verifikasi host key SSH (`known_hosts`); saat ini semua host key diterima
- [ ] Simpan secret di Windows Credential Manager / macOS Keychain

### Menerapkan pengaturan yang sudah tersimpan
- [ ] Jump host (ProxyJump)
- [ ] Keepalive, compression, agent forwarding, auto-reconnect
- [ ] Startup command & SFTP start directory
- [ ] Autentikasi SSH agent dan keyboard-interactive

### Fitur
- [ ] SFTP browser
- [ ] Sinkronisasi cloud otomatis (saat ini manual via file `.stvault`)
- [ ] Tunnel otomatis bind ulang setelah SSH reconnect
- [ ] Opsi isi-otomatis target Ping/Traceroute/Port Scanner bisa dimatikan

### Distribusi
- [ ] Installer Windows (MSI / NSIS)
- [ ] macOS: code signing & notarization, `.dmg`
- [ ] Verifikasi build macOS untuk TLS SQL Client (Security.framework)
- [ ] Paket Linux (AppImage / .deb)
