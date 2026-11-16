# SUPERTERM — Panduan Teknis & Implementasi SSH Tunneling

Fitur Port Forwarding di Superterm mengimplementasikan standar spesifikasi SSHv2 (RFC 4254 §7) dengan engine asynchronous non-blocking berbasis Rust Tokio.

---

## 1. Jenis-Jenis Tunneling di Superterm

| Tipe Tunnel | Parameter Perintah SSH Biasa | Mekanisme di Rust | Kasus Penggunaan Utama |
|---|---|---|---|
| **Static Local** | `-L [local_ip:]local_port:remote_host:remote_port` | `TcpListener` lokal -> Russh `channel_open_direct_tcpip` | Akses database internal server (MySQL, Redis, Mongo, Postgres) |
| **Static Remote** | `-R [remote_ip:]remote_port:local_host:local_port` | Russh `tcpip_forward` -> SSH Server forward ke `TcpStream` lokal | Ekspos aplikasi localhost / webhook ke internet melalui IP server |
| **Dynamic SOCKS5** | `-D [bind_ip:]bind_port` | SOCKS5 Server lokal -> Handshake RFC 1928 -> Russh `channel_open_direct_tcpip` | Proxy internet aman, browsing, unblock geo-restriction |

---

## 2. Alur Arsitektur Dynamic SOCKS5 Proxy Server di Rust

```
+--------------------+
| Browser / App      |
| (Firefox, etc)     |
+---------+----------+
          | SOCKS5 Handshake (Port 1080)
          v
+-----------------------------------------------------------+
| SUPERTERM SOCKS5 SERVER (Rust tokio::net::TcpListener)     |
|                                                           |
| 1. Method Selection (RFC 1928): 0x05, NO_AUTH             |
| 2. Request Parsing: CMD=0x01 (CONNECT),                   |
|    Target Addr (IPv4 / IPv6 / FQDN Domain) & Target Port  |
+-----------------------------+-----------------------------+
                              | Target: api.internal:443
                              v
+-----------------------------------------------------------+
| RUSSH SSH SESSION (Multiplexed Encrypted SSH Channel)     |
|                                                           |
| Kirim SSH MSG_CHANNEL_OPEN: "direct-tcpip"                |
| - Host: "api.internal"                                    |
| - Port: 443                                               |
| - Originator: "127.0.0.1", port: 51234                    |
+-----------------------------+-----------------------------+
                              | Terenkripsi melalui Internet
                              v
+-----------------------------------------------------------+
| REMOTE SSH SERVER (sshd daemon di VPS)                    |
|                                                           |
| sshd membuka socket TCP langsung ke api.internal:443      |
| dan streaming data bolak-balik                            |
+-----------------------------------------------------------+
```

---

## 3. Desain Kode Rust Tokio (Implementasi Ringkas)

Semua jenis tunnel ada di satu berkas, `src/core/tunnel.rs`. Titik masuknya `spawn_tunnel(...)`, yang mengembalikan `TunnelRuntimeHandle` (`stop()` dan `bytes_transferred()`). Contoh di bawah adalah versi ringkas untuk menjelaskan alur, bukan salinan persis kodenya.

### A. Static Local Port Forwarding
```rust
use tokio::net::{TcpListener, TcpStream};
use tokio::io::copy_bidirectional;

pub async fn start_local_tunnel(
    bind_addr: &str,
    remote_host: String,
    remote_port: u32,
    ssh_session: russh::client::Handle<MyClientHandler>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    println!("Local tunnel active on {}", bind_addr);

    loop {
        let (mut local_socket, _) = listener.accept().await?;
        let session = ssh_session.clone();
        let r_host = remote_host.clone();

        tokio::spawn(async move {
            // Membuka channel direct-tcpip di server SSH
            let mut channel = session
                .channel_open_direct_tcpip(r_host, remote_port, "127.0.0.1", 0)
                .await?;

            // Salurkan stream TCP lokal secara bidirectional dengan channel SSH
            let mut stream = channel.into_stream();
            let _ = copy_bidirectional(&mut local_socket, &mut stream).await;
            Ok::<(), anyhow::Error>(())
        });
    }
}
```

### B. Static Remote Port Forwarding
1. Mengirim `tcpip-forward` ke server SSH untuk `bind_host:bind_port`.
2. Saat server membuka channel `forwarded-tcpip`, handler SSH mencocokkan port-nya lalu menyambung ke `remote_host:remote_port` di sisi lokal.
3. Saat tunnel dihentikan, `cancel-tcpip-forward` dikirim ke server.

### C. Dynamic SOCKS5 Proxy Server
Menggunakan parser protokol RFC 1928 ringan:
1. Menerima byte `[0x05, NMETHODS, METHODS...]`.
2. Menjawab `[0x05, 0x00]` (tanpa autentikasi).
3. Membaca permintaan `[0x05, CMD, RSV, ATYP, ADDR, PORT]`. Hanya `CMD = 0x01` (CONNECT) yang didukung; perintah lain dijawab `0x07` (command not supported).
4. `ATYP` yang didukung: `0x01` IPv4, `0x03` nama domain, `0x04` IPv6.
5. Membuka channel `direct-tcpip` SSH ke alamat tujuan.
6. Menjawab sukses `[0x05, 0x00, 0x00, 0x01, 0,0,0,0, 0,0]`, atau `0x05` (connection refused) jika channel gagal dibuka.
7. Menyalurkan data dua arah.

---

## 4. Tampilan UI Port Forwarding di Superterm

Panel **Port Forwarding & Proxy** di sisi kanan:
- **Daftar tunnel** per host, dengan filter *Selected Host* / *All Hosts*. Setiap tunnel menampilkan tipe (`LOCAL`, `REMOTE`, `DYNAMIC`), port lokal, target, nama host, dan status Online / Auto-ON.
- **Start / Stop** per tunnel tanpa memutus terminal SSH.
- **Penghitung byte**: total data yang sudah lewat, misalnya `Live (0.3 KB)`. Kecepatan transfer per detik belum ditampilkan.
- **Auto-ON**: tunnel dengan `auto_start = true` aktif otomatis saat host terhubung, dan dihentikan saat tab terakhir host itu ditutup.

Belum ada: bind ulang otomatis setelah SSH terputus. Opsi *auto-reconnect* di editor host sudah tersimpan, tapi belum diterapkan.

---

## 5. Contoh: database di balik bastion

```
PC (Superterm)                 Bastion (SSH)              Database
127.0.0.1:15000  ── SSH -L ──►  bastion.example.com  ──►  10.0.0.5:5432
```

- Tunnel: `local`, bind `127.0.0.1:15000`, remote `10.0.0.5:5432`.
- SQL Client terhubung ke **`127.0.0.1:15000`**, yaitu ujung lokal tunnel, bukan `10.0.0.5:5432`.
- Database melihat koneksi datang dari IP bastion, bukan dari PC Anda. Aturan `pg_hba.conf` di server harus mengizinkan IP itu.
- Jika aturannya `hostssl`, koneksi wajib terenkripsi. SQL Client menegosiasikan TLS secara otomatis (`sslmode=prefer`).
