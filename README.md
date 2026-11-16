# Superterm

> **SSH client, tunneling manager, dan toolbox jaringan native untuk Windows & macOS.**
> 100% Rust — tanpa HTML, CSS, JavaScript, Chromium, maupun WebView.

---

## Keunggulan

- **Native, satu file**: hasil build adalah satu executable mandiri (~5 MB) tanpa runtime eksternal.
- **Rendering GPU langsung**: UI digambar lewat OpenGL (`eframe` / `egui`, backend `glow`).
- **Tanpa permukaan serangan web**: tidak ada DOM, tidak ada XSS, tidak ada engine browser.

---

## Fitur

### 1. Hosts & SSH
- Terminal SSH v2 interaktif (`russh` + `tokio`) dengan emulator VT100 sendiri, warna, scrollback, resize PTY real-time, serta select / copy / paste.
- Autentikasi **password** dan **private key** (Ed25519, RSA, ECDSA; dengan passphrase).
- Multi-tab: satu host bisa dibuka di beberapa tab sekaligus (**Connect in new tab**).
- Sidebar berfolder: folder bisa di-rename dan dihapus lewat klik kanan (hapus hanya untuk folder kosong). Field Folder di editor host berupa dropdown folder yang ada, atau ketik nama folder baru.
- Menu host (klik kanan atau tombol `...`): Connect, Connect in new tab, **Disconnect** (satu tab), **Disconnect All Tabs**, Edit, Duplicate, Rename, Export Connection (JSON), Delete.
- Editor host bertab: *Connection* dan *Advanced*.

### 2. SSH Tunneling
- **Local** (`-L`), **Remote** (`-R`), dan **Dynamic SOCKS5** (`-D`, RFC 1928).
- Tunnel **Auto-ON** otomatis aktif saat host terhubung, dan berhenti saat tab terakhir host itu ditutup.
- Panel *Port Forwarding & Proxy*: Start/Stop per tunnel tanpa memutus SSH, dengan penghitung byte live.

### 3. Local Shell
- **PowerShell** dan **CMD** di Windows, **Terminal** (login shell) di macOS/Linux, berjalan di PTY sungguhan (ConPTY di Windows).
- Opsi **Run as Administrator / Root** (Superterm di-restart dengan hak elevated). Tab elevated diberi label `(Administrator)` / `(Root)`.

### 4. Tools
| Tool | Fungsi |
|---|---|
| Ping, Traceroute, Port Scanner | Diagnostik jaringan |
| DNS Lookup, DNS Whois | Termasuk ccTLD |
| Network Interfaces | Daftar adapter lokal |
| SSH Key Generator | Ed25519 / ECDSA-P256 ke `~/.ssh` |
| Password Vault | Melihat & menyalin kredensial host |
| **SQL Client** | MySQL & PostgreSQL, multi-sesi, hasil bertab |
| **Note Vault** | Catatan multi-tab, ikut tersimpan di vault |

**SQL Client**
- Koneksi tersimpan di vault; host/port bisa diisi otomatis dari tunnel lokal lewat **Use tunnel...**
- **TLS** lewat sistem operasi (SChannel di Windows, Security.framework di macOS), mode `prefer`: SSL dipakai bila server menawarkannya. Ini wajib untuk server dengan `pg_hba.conf` berjenis `hostssl`.
- Setiap Execute membuka **tab hasil baru**. Caption tab adalah tabel pertama di query; klik kanan untuk Copy / Edit / Re-run. SELECT tampil sebagai grid (maks 1.000 baris), INSERT/UPDATE/DELETE menampilkan jumlah baris terdampak.
- Beberapa database bisa dibuka bersamaan sebagai tab terpisah (**New SQL session**).

### 5. Input teks
Semua field teks di aplikasi punya menu klik kanan **Cut / Copy / Paste / Select All**. Field password hanya menyediakan Paste dan Select All, kecuali password sedang ditampilkan.

### 6. Vault & Sync
- Semua data (host, tunnel, catatan, koneksi SQL, pengaturan) disimpan di satu file vault lokal.
- **Sync** = ekspor/impor file `.stvault` terenkripsi (**Argon2id + AES-256-GCM**) ke path pilihan Anda, misalnya folder Google Drive, OneDrive, atau USB. Belum ada sinkronisasi cloud otomatis.

### 7. Tampilan
- Mode Night / Day / Auto (mengikuti OS).
- Tema terminal: Dracula, Tokyo Night, Nord, Monokai Pro, Cyberpunk, Daylight Clean, Solarized Light.
- Ukuran font terminal 11–24 px dan zoom UI.

---

## Lokasi data

| OS | Path |
|---|---|
| Windows | `%APPDATA%\Superterm\superterm.vault` |
| macOS | `~/Library/Application Support/Superterm/superterm.vault` |
| Linux | `~/.config/Superterm/superterm.vault` |

> **Penting:** `superterm.vault` lokal berupa **JSON biasa (tidak terenkripsi)**, termasuk password. Yang terenkripsi hanya file `.stvault` hasil ekspor. Lindungi folder tersebut dengan izin akun OS Anda.

Untuk reset total: tutup Superterm, cadangkan folder di atas, lalu hapus foldernya.

---

## Build

```powershell
cd Z:\ProjectsX\Experiments\Superterm
cargo build --release
```

Hasil: `target/release/superterm.exe` (Windows) atau `target/release/superterm` (macOS/Linux).

Proyek ini dibangun dengan toolchain `x86_64-pc-windows-gnu` tanpa compiler C, sehingga dependensi yang membutuhkan C (misalnya `ring` untuk rustls) sengaja dihindari. GitHub Actions (`.github/workflows/build.yml`) mem-build Windows dan macOS (Apple Silicon & Intel) secara otomatis.

---

## Struktur kode

```
Superterm/
├── Cargo.toml
├── build.rs / app.rc / app.ico      # Ikon & resource Windows
├── docs/
│   ├── ARCHITECTURE.md
│   ├── DATA_SCHEMA.md
│   ├── ROADMAP.md
│   └── TUNNELING_GUIDE.md
└── src/
    ├── main.rs                      # Entry point (eframe::run_native)
    ├── core/
    │   ├── ssh.rs                   # Sesi SSH async & streaming PTY
    │   ├── tunnel.rs                # Local / Remote / SOCKS5
    │   ├── localshell.rs            # PowerShell / CMD / Terminal di PTY
    │   ├── sqlclient.rs             # Driver MySQL & PostgreSQL (sqlx)
    │   ├── tools.rs                 # Ping, DNS, Whois, port scan, keygen
    │   └── vault.rs                 # Penyimpanan vault & .stvault terenkripsi
    └── ui/
        ├── app.rs                   # Shell UI: sidebar, tab, modal
        ├── terminal.rs              # Emulator VT100 & renderer
        └── tools_view.rs            # UI tools, SQL Client, Note Vault
```

Lihat [`docs/`](docs/) untuk detail arsitektur, skema data, dan tunneling.
