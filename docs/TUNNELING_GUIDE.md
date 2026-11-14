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

### A. Static Local Port Forwarding (`local.rs`)
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

### B. Dynamic SOCKS5 Proxy Server (`socks5.rs`)
Menggunakan parser protokol RFC 1928 ringan:
1. Menerima byte `[0x05, NMETHODS, METHODS...]`.
2. Menjawab `[0x05, 0x00]` (No authentication required).
3. Membaca paket permintaan `[0x05, CMD=0x01, RSV=0x00, ATYP, ADDR, PORT]`.
4. Menyambungkan socket channel `direct-tcpip` SSH ke destination address tersebut.
5. Menjawab status sukses `[0x05, 0x00, 0x00, 0x01, 0,0,0,0, 0,0]`.
6. Memulai bidirectional I/O copy via `tokio::io::copy_bidirectional`.

---

## 4. Tampilan UI Port Forwarding di Superterm

Di frontend, setiap host memiliki tab / drawer **"Tunnels"**:
- **Daftar Port Forward**: Menampilkan port lokal, target remote, dan tipe icon (`L`, `R`, `D`).
- **Live Toggle**: Saklar hidup/mati cepat untuk setiap tunnel tanpa perlu memutus koneksi terminal SSH.
- **Traffic Monitor**: Menampilkan kecepatan transfer (KB/s, MB/s) dan jumlah total bytes yang lewat.
- **Auto-reconnect**: Jika koneksi SSH sempat putus (misal WiFi berubah), tunnel otomatis di-bind ulang saat SSH reconnect.
