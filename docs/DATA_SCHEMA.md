# SUPERTERM — Data Schema & Vault Specification

Dokumen ini mendefinisikan struktur data, skema serialisasi JSON, dan format penyimpanan terenkripsi yang digunakan oleh Superterm agar kompatibel 100% di **Windows, macOS, dan Linux**.

---

## 1. Skema Profil Koneksi Host (`HostProfile`)

Setiap koneksi yang disimpan di panel kiri (sidebar) direpresentasikan oleh struktur JSON berikut:

```json
{
  "id": "uuid-v4-identifier",
  "name": "Production Web 01",
  "folder": "Production",
  "tags": ["web", "nginx", "aws-sg"],
  "host": "13.250.12.34",
  "port": 22,
  "username": "ubuntu",
  "auth_type": "private_key",
  "auth_payload": {
    "key_id": "key-uuid-ref-or-inline",
    "key_type": "ed25519",
    "passphrase_protected": true
  },
  "keepalive_interval_secs": 30,
  "connect_timeout_secs": 10,
  "tunnels": [
    {
      "id": "tun-001",
      "name": "MySQL DB Tunnel",
      "tunnel_type": "local",
      "bind_host": "127.0.0.1",
      "bind_port": 3307,
      "remote_host": "10.0.1.25",
      "remote_port": 3306,
      "auto_start": true
    },
    {
      "id": "tun-002",
      "name": "Bypass SOCKS5",
      "tunnel_type": "dynamic",
      "bind_host": "127.0.0.1",
      "bind_port": 1080,
      "auto_start": false
    }
  ],
  "terminal_override": {
    "font_family": "JetBrains Mono",
    "font_size": 14,
    "theme_id": "dracula"
  },
  "created_at": "2026-04-10T12:00:00Z",
  "updated_at": "2026-04-10T14:30:00Z",
  "last_connected_at": "2026-04-10T15:10:00Z"
}
```

---

## 2. Skema Tunneling (`TunnelConfig`)

| Field | Tipe | Deskripsi |
|---|---|---|
| `id` | `String (UUID)` | ID unik tunnel |
| `name` | `String` | Label deskriptif tunnel (misal "Redis Staging") |
| `tunnel_type` | `local` \| `remote` \| `dynamic` | Tipe port forward |
| `bind_host` | `String` | IP lokal tempat listener dibuka (default: `127.0.0.1`) |
| `bind_port` | `u16` | Port listener (misal `3307`, `1080`) |
| `remote_host` | `Option<String>` | Host tujuan (wajib untuk `local` & `remote`) |
| `remote_port` | `Option<u16>` | Port tujuan (wajib untuk `local` & `remote`) |
| `auto_start` | `bool` | Otomatis aktif saat SSH host terkoneksi |

---

## 3. Format Enkripsi Vault (`superterm.vault`)

File vault disimpan di disk dengan format biner atau JSON terenkripsi:

```
+---------------------------------------------------------------+
|                      SUPERTERM VAULT HEADER                   |
|  Magic Bytes: [0x53, 0x54, 0x56, 0x31] ("STV1" -> Superterm V1)|
|  Argon2id Salt: 16 bytes random                               |
|  AES-256-GCM Nonce / IV: 12 bytes random                      |
|  Payload Size: 4 bytes (u32 little-endian)                    |
+---------------------------------------------------------------+
|                   ENCRYPTED CIPHERTEXT (AES-256-GCM)          |
|  JSON payload terkompresi (gzip/zstd) yang berisi:            |
|  - Daftar host profiles                                       |
|  - Private keys / SSH credentials                             |
|  - App settings & tema                                        |
+---------------------------------------------------------------+
|                    GCM AUTHENTICATION TAG                     |
|  16 bytes MAC Tag (menjamin data tidak dimanipulasi)          |
+---------------------------------------------------------------+
```

### Derivasi Kunci Kriptografi (Argon2id)
- **Algorithm**: Argon2id v13
- **Memory Cost (m_cost)**: 65,536 KB (64 MB)
- **Time Cost (t_cost)**: 3 iterations
- **Parallelism (p_cost)**: 4 threads
- **Output Key**: 256 bits (32 bytes) untuk AES-256-GCM cipher.

---

## 4. Skema Kustomisasi Terminal (`TerminalPreferences`)

```json
{
  "font_family": "JetBrains Mono, Cascadia Code, Consolas, monospace",
  "font_size": 14,
  "line_height": 1.25,
  "letter_spacing": 0,
  "cursor_style": "block",
  "cursor_blink": true,
  "scrollback_lines": 10000,
  "copy_on_select": true,
  "right_click_action": "paste",
  "bell_style": "sound_and_visual",
  "color_scheme": {
    "name": "Superterm Obsidian",
    "background": "#0D1117",
    "foreground": "#E6EDF3",
    "cursor": "#58A6FF",
    "selection": "#264F78",
    "black": "#484F58",
    "red": "#FF7B72",
    "green": "#3FB950",
    "yellow": "#D29922",
    "blue": "#58A6FF",
    "magenta": "#BC8CFF",
    "cyan": "#39C5CF",
    "white": "#B1BAC4",
    "bright_black": "#6E7681",
    "bright_red": "#FFA198",
    "bright_green": "#56D364",
    "bright_yellow": "#E3B341",
    "bright_blue": "#79C0FF",
    "bright_magenta": "#D2A8FF",
    "bright_cyan": "#56D4DD",
    "bright_white": "#F0F6FC"
  }
}
```

---

## 5. Skema Metadata Sinkronisasi Google Drive (`sync_meta.json`)

Metadata disimpan di `drive.appdata` untuk mendeteksi revisi terbaru secara instan tanpa perlu mengunduh seluruh file vault jika belum ada perubahan:

```json
{
  "app_version": "1.0.0",
  "device_id": "win-desktop-7b89",
  "device_name": "Windows Workstation (Win 11)",
  "vault_revision": 42,
  "vault_hash_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "timestamp_utc": "2026-04-10T15:30:12Z"
}
```
