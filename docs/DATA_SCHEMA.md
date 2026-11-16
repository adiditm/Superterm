# SUPERTERM — Skema Data & Format Vault

Dokumen ini menjelaskan struktur data yang **benar-benar** disimpan oleh Superterm, sesuai `src/core/vault.rs`.

Semua field baru memakai `#[serde(default)]`, sehingga vault lama tetap bisa dibaca setelah aplikasi diperbarui.

---

## 1. Berkas

| Berkas | Isi | Enkripsi |
|---|---|---|
| `superterm.vault` | `AppVault` sebagai JSON (pretty-printed) | **Tidak** |
| `*.stvault` | `AppVault` terenkripsi, hasil ekspor Sync | Argon2id + AES-256-GCM |

Lokasi `superterm.vault`: direktori config OS + `Superterm/` (`%APPDATA%` di Windows, `~/Library/Application Support` di macOS, `~/.config` di Linux).

---

## 2. `AppVault` (root)

```json
{
  "version": 1,
  "appearance_mode": "night",
  "terminal_theme": "Dracula",
  "font_size": 14.0,
  "ui_zoom": 1.21,
  "sync_email": "",
  "sync_revision": 1,
  "custom_folders": [],
  "sql_connections": [],
  "notes": [],
  "profiles": []
}
```

| Field | Tipe | Keterangan |
|---|---|---|
| `appearance_mode` | `"night"` \| `"day"` \| `"auto"` | `auto` mengikuti tema OS |
| `terminal_theme` | string | Nama tema terminal |
| `font_size` | f32 | Dibatasi 10–26 saat dimuat |
| `ui_zoom` | f32 | Dibatasi 0.8–2.2 |
| `sync_email` | string | Hanya label di kartu Sync |
| `sync_revision` | u32 | Naik setiap ekspor `.stvault` |
| `custom_folders` | string[] | Folder tanpa host (agar folder kosong tetap ada) |
| `sql_connections` | `SqlConfig[]` | Koneksi SQL Client tersimpan |
| `notes` | `NoteEntry[]` | Isi Note Vault |
| `profiles` | `HostProfile[]` | Host SSH beserta tunnel-nya |

---

## 3. `HostProfile`

```json
{
  "id": "host-<uuid>",
  "name": "Web Server",
  "folder": "Production",
  "tags": ["ssh"],
  "host": "server.example.com",
  "port": 22,
  "username": "ubuntu",
  "auth_type": "password",
  "password": "…",
  "key_path": null,
  "passphrase": null,
  "tunnels": [],
  "last_connected": null,

  "session_type": "ssh",
  "use_jump_host": false,
  "jump_host": "",
  "jump_port": 22,
  "jump_username": "",
  "startup_command": "",
  "sftp_start_dir": "",
  "keepalive_secs": 30,
  "compression": false,
  "agent_forwarding": false,
  "auto_reconnect": false,
  "open_sftp_on_connect": false
}
```

- `auth_type`: yang berfungsi adalah `"password"` dan `"private_key"`. Nilai `"agent"` dan `"keyboard_interactive"` bisa tersimpan tetapi belum diimplementasikan.
- Field mulai `session_type` ke bawah (tab *Advanced*) **tersimpan tetapi belum diterapkan** saat koneksi.

---

## 4. `TunnelConfig`

| Field | Tipe | Keterangan |
|---|---|---|
| `id` | string | ID unik |
| `name` | string | Label, misalnya "Postgres via bastion" |
| `tunnel_type` | `"local"` \| `"remote"` \| `"dynamic"` | `-L` / `-R` / `-D` |
| `bind_host` | string | Biasanya `127.0.0.1` |
| `bind_port` | u16 | Port listener |
| `remote_host` | string \| null | Wajib untuk `local` & `remote` |
| `remote_port` | u16 \| null | Wajib untuk `local` & `remote` |
| `auto_start` | bool | Default `true`: aktif otomatis saat host terhubung |
| `is_active` | bool | Status tersimpan |
| `bytes_transferred` | u64 | Penghitung byte |

---

## 5. `SqlConfig`

```json
{
  "id": "sql-<uuid>",
  "name": "Reporting DB",
  "host": "127.0.0.1",
  "port": 15000,
  "user": "readonly",
  "pass": "…",
  "database": "reports",
  "db_type": "PostgreSQL"
}
```

- `db_type`: `"MySQL"` atau `"PostgreSQL"`.
- `host` / `port` adalah alamat yang **dihubungi langsung oleh Superterm**. Untuk database di balik SSH tunnel, isi dengan ujung lokal tunnel (`127.0.0.1:<bind_port>`), bukan host/port database di sisi remote.

---

## 6. `NoteEntry`

```json
{
  "id": "note-<uuid>",
  "title": "Runbook",
  "body": "…",
  "updated_at": "2026-11-14T10:00:00+07:00"
}
```

---

## 7. Format `.stvault`

```
+--------+-----------+-----------+--------------------------------------+
| "STV1" | salt (16) | nonce (12)| ciphertext AES-256-GCM (+16 B tag)   |
+--------+-----------+-----------+--------------------------------------+
  4 byte    acak        acak       JSON AppVault (compact, tanpa kompresi)
```

- **Derivasi kunci**: Argon2id dengan **parameter default crate `argon2` 0.5**, yaitu `m_cost = 19456 KiB` (19 MiB), `t_cost = 2`, `p_cost = 1`, menghasilkan kunci 32 byte.
- **Cipher**: AES-256-GCM. Tag autentikasi 16 byte ikut di akhir ciphertext, sehingga password salah atau file yang dimodifikasi pasti ditolak.
- Tidak ada field ukuran payload; panjang ciphertext adalah sisa file setelah byte ke-32.

> Mengubah parameter Argon2 akan membuat `.stvault` lama tidak bisa dibuka. Jika suatu saat parameter dinaikkan, header perlu diberi versi baru (mis. `STV2`).
