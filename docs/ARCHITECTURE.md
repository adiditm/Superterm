# SUPERTERM — Arsitektur

> Aplikasi desktop 100% Rust, tanpa HTML / CSS / JavaScript / WebView.
> Target utama Windows, juga di-build untuk macOS.

---

## 1. Ringkasan teknologi

| Lapisan | Teknologi |
|---|---|
| GUI | `eframe` / `egui` 0.29, immediate mode, backend OpenGL (`glow`) |
| Async runtime | `tokio` (multi-thread) |
| SSH | `russh` / `russh-keys` 0.45 |
| Terminal | Parser `vt100` + renderer grid buatan sendiri (`ui/terminal.rs`) |
| Local shell | `portable-pty` (ConPTY di Windows) |
| SQL | `sqlx` 0.8 (MySQL, PostgreSQL), TLS via `native-tls` |
| Kriptografi | `argon2` + `aes-gcm` |
| Clipboard | `arboard` |

---

## 2. Diagram

```
+-----------------------------------------------------------------------------------+
|                          SUPERTERM (eframe / egui, UI thread)                     |
|                                                                                   |
|  +--------------+  +---------------------------------------------+  +-----------+ |
|  | ACTIVITY RAIL|  | TAB STRIP: SSH | Shell | SQL | Tool | [+]   |  | PORT      | |
|  |  Hosts       |  +---------------------------------------------+  | FORWARDING| |
|  |  Tools       |  |                                             |  | & PROXY   | |
|  +--------------+  |   CENTRAL PANEL (sesuai jenis tab aktif)    |  | (drawer)  | |
|  | SIDEBAR      |  |   - Terminal VT100 (SSH / Local Shell)      |  |  Start /  | |
|  |  Search      |  |   - Tool workspace (Ping, DNS, ...)         |  |  Stop,    | |
|  |  Folder tree |  |   - SQL Client session                      |  |  byte     | |
|  |  Tools list  |  |   - Note Vault                              |  |  counter  | |
|  |  Sync card   |  |                                             |  |           | |
|  +--------------+  +---------------------------------------------+  +-----------+ |
+-----------------------------------------+-----------------------------------------+
                                          | tokio::sync::mpsc (event per subsistem)
                                          v
+-----------------------------------------------------------------------------------+
|                               TOKIO RUNTIME (background)                          |
|                                                                                   |
|  ssh.rs        tunnel.rs        localshell.rs     sqlclient.rs      tools.rs      |
|  sesi SSH,     -L / -R / -D,    PTY + balasan     koneksi per       ping, dns,    |
|  PTY stream    copy dua arah    query DSR         query, TLS        whois, scan   |
+-----------------------------------------+-----------------------------------------+
                                          |
                                          v
                        vault.rs: superterm.vault (JSON) + .stvault (terenkripsi)
```

UI tidak pernah memblokir. Setiap subsistem mengirim event lewat channel `mpsc`, lalu UI menguras channel tersebut di awal tiap frame (`process_ssh_events`, `process_shell_events`, `process_events`, `SqlSession::process_events`).

---

## 3. Model tab

Satu tab strip menampung beberapa jenis tab (`TabKind`):

| Jenis | Kunci sesi | Catatan |
|---|---|---|
| `Ssh` | `ssh:<host_id>` / `ssh:<host_id>:<seq>` | Satu host bisa punya banyak tab |
| `Shell` | `shell:<nama>:<seq>` | PowerShell / CMD / Terminal |
| `Sql` | `sql:<seq>` | Tiap tab punya `SqlSession` sendiri, sehingga beberapa database bisa terbuka paralel |
| `Tool` | `tool:<nama>` | Satu instance per tool |

Tunnel Auto-ON dinyalakan saat host terhubung, dan dihentikan oleh `stop_tunnels_for_host_if_last` hanya ketika tab terakhir host itu ditutup.

---

## 4. Keputusan desain penting

**Local shell dan DSR.** ConPTY mengirim `ESC[6n` (Device Status Report) di awal sesi dan **menunggu balasan** posisi kursor. Parser `vt100` tidak membalas, sehingga shell menggantung tanpa prompt. `localshell.rs` memindai output dan menjawab `ESC[6n`, `ESC[5n`, `ESC[c`, serta `ESC[>c` sendiri.

**Menu klik kanan pada input teks.** `TextEdit` di egui 0.29 meruntuhkan seleksi pada *setiap* penekanan tombol mouse, termasuk tombol kanan, kecuali Shift ditahan. `MenuEdit` (`ui/tools_view.rs`) menyimpan seleksi tiap frame. Saat tombol kanan ditekan di atas field, MenuEdit melaporkan Shift ke widget untuk satu panggilan itu saja, lalu menulis balik seleksi setelah widget diproses. Context menu tidak terdeteksi oleh `any_popup_open()`, jadi status menu dilacak per field lewat `context_menu_opened()`.

**ID jendela modal.** egui menurunkan ID jendela dari judulnya. Dialog yang judulnya ikut berubah (misalnya *Edit session – nama*) diberi `.id()` tetap, supaya jendela tidak melompat saat mengetik.

**TLS tanpa compiler C.** Toolchain GNU di mesin build tidak punya compiler C, sehingga rustls (`ring`) gagal dikompilasi. `sqlx` memakai `tls-native-tls`: SChannel di Windows, Security.framework di macOS.

**Shutdown.** `on_exit` memutus semua sesi SSH dan tunnel serta membunuh proses shell lokal, agar aplikasi tidak menggantung saat ditutup (kasus yang ditemukan di macOS).

---

## 5. Keterbatasan saat ini

- `superterm.vault` di disk **tidak terenkripsi** (JSON biasa). Hanya ekspor `.stvault` yang terenkripsi.
- Field tab *Advanced* (jump host, keepalive, compression, agent forwarding, auto-reconnect, startup command, SFTP) dan `session_type` **tersimpan tetapi belum diterapkan** oleh lapisan SSH.
- Autentikasi yang benar-benar diimplementasikan baru **password** dan **private key**. Pilihan *agent* dan *keyboard-interactive* ada di form tapi belum berfungsi.
- Verifikasi host key SSH belum ada (`check_server_key` selalu menerima).
- Sync belum otomatis; masih ekspor/impor file manual.

---

## 6. Build

```powershell
cargo build --release
```

Profil release memakai `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, dan `strip = true`, dengan hasil sekitar 5 MB.
