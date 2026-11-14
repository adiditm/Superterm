use crate::core::ssh::{ActiveSshSession, SshEvent};
use crate::core::vault::TunnelConfig;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub struct TunnelRuntimeHandle {
    #[allow(dead_code)]
    pub tunnel_id: String,
    pub bytes_counter: Arc<AtomicU64>,
    pub abort_tx: tokio::sync::watch::Sender<bool>,
}

impl TunnelRuntimeHandle {
    pub fn stop(&self) {
        let _ = self.abort_tx.send(true);
    }

    pub fn bytes_transferred(&self) -> u64 {
        self.bytes_counter.load(Ordering::Relaxed)
    }
}

pub fn spawn_tunnel(
    rt: &tokio::runtime::Handle,
    session: Arc<ActiveSshSession>,
    config: TunnelConfig,
    tx_events: mpsc::UnboundedSender<SshEvent>,
    egui_ctx: eframe::egui::Context,
) -> TunnelRuntimeHandle {
    let bytes_counter = Arc::new(AtomicU64::new(0));
    let (abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);

    let tunnel_id = config.id.clone();
    let host_id = session.host_id.clone();
    let counter_clone = bytes_counter.clone();

    rt.spawn(async move {
        let emit_log = |msg: String| {
            let _ = tx_events.send(SshEvent::TunnelLog {
                host_id: host_id.clone(),
                message: msg,
            });
            egui_ctx.request_repaint();
        };

        match config.tunnel_type.as_str() {
            "local" => {
                let bind_addr = format!("{}:{}", config.bind_host, config.bind_port);
                let remote_host = config.remote_host.unwrap_or_else(|| "127.0.0.1".to_string());
                let remote_port = config.remote_port.unwrap_or(3306) as u32;

                let listener = match TcpListener::bind(&bind_addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        emit_log(format!("[!] Failed to bind Local Tunnel {}: {}", bind_addr, e));
                        return;
                    }
                };
                emit_log(format!(
                    "[*] Local Tunnel ACTIVE: {} -> {}:{}",
                    bind_addr, remote_host, remote_port
                ));

                loop {
                    tokio::select! {
                        _ = abort_rx.changed() => {
                            if *abort_rx.borrow() {
                                emit_log(format!("[*] Local Tunnel STOPPED & connections closed: {}", bind_addr));
                                break;
                            }
                        }
                        accept_res = listener.accept() => {
                            if let Ok((mut local_stream, _)) = accept_res {
                                let sess = session.clone();
                                let r_host = remote_host.clone();
                                let bytes_cnt = counter_clone.clone();
                                let mut child_abort = abort_rx.clone();
                                tokio::spawn(async move {
                                    if *child_abort.borrow() {
                                        let _ = local_stream.shutdown().await;
                                        return;
                                    }
                                    let channel_res = {
                                        let guard = sess.ssh_handle.lock().await;
                                        guard.channel_open_direct_tcpip(r_host, remote_port, "127.0.0.1", 0).await
                                    };
                                    if let Ok(channel) = channel_res {
                                        let mut ssh_stream = channel.into_stream();
                                        tokio::select! {
                                            _ = child_abort.changed() => {
                                                let _ = local_stream.shutdown().await;
                                                let _ = ssh_stream.shutdown().await;
                                            }
                                            res = copy_bidirectional(&mut local_stream, &mut ssh_stream) => {
                                                if let Ok((tx, rx)) = res {
                                                    bytes_cnt.fetch_add(tx + rx, Ordering::Relaxed);
                                                }
                                            }
                                        }
                                    } else {
                                        let _ = local_stream.shutdown().await;
                                    }
                                });
                            }
                        }
                    }
                }
            }
            "dynamic" => {
                let bind_addr = format!("{}:{}", config.bind_host, config.bind_port);
                let listener = match TcpListener::bind(&bind_addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        emit_log(format!("[!] Failed to bind SOCKS5 Proxy {}: {}", bind_addr, e));
                        return;
                    }
                };
                emit_log(format!("[*] Dynamic SOCKS5 Proxy ACTIVE on {}", bind_addr));

                loop {
                    tokio::select! {
                        _ = abort_rx.changed() => {
                            if *abort_rx.borrow() {
                                emit_log(format!("[*] Dynamic SOCKS5 Proxy STOPPED & connections closed: {}", bind_addr));
                                break;
                            }
                        }
                        accept_res = listener.accept() => {
                            if let Ok((mut socket, _)) = accept_res {
                                let sess = session.clone();
                                let bytes_cnt = counter_clone.clone();
                                let mut child_abort = abort_rx.clone();
                                tokio::spawn(async move {
                                    if *child_abort.borrow() {
                                        let _ = socket.shutdown().await;
                                        return;
                                    }
                                    let _ = handle_socks5_client(&mut socket, sess, bytes_cnt, &mut child_abort).await;
                                    let _ = socket.shutdown().await;
                                });
                            }
                        }
                    }
                }
            }
            "remote" => {
                let bind_host = config.bind_host.clone();
                let bind_port = config.bind_port as u32;
                let remote_host = config.remote_host.unwrap_or_else(|| "127.0.0.1".to_string());
                let remote_port = config.remote_port.unwrap_or(3000);

                {
                    let mut map = session.remote_forwards.lock();
                    map.insert(bind_port, (remote_host.clone(), remote_port, abort_rx.clone()));
                }

                let fwd_res = {
                    let mut guard = session.ssh_handle.lock().await;
                    guard.tcpip_forward(bind_host.clone(), bind_port).await
                };

                match fwd_res {
                    Ok(_) => {
                        emit_log(format!(
                            "[*] Remote Tunnel ACTIVE: {}:{} -> {}:{}",
                            bind_host, bind_port, remote_host, remote_port
                        ));
                    }
                    Err(e) => {
                        emit_log(format!("[!] Failed to start Remote Forward {}:{}: {:?}", bind_host, bind_port, e));
                        return;
                    }
                }

                let _ = abort_rx.changed().await;
                {
                    let guard = session.ssh_handle.lock().await;
                    let _ = guard.cancel_tcpip_forward(bind_host.clone(), bind_port).await;
                }
                {
                    let mut map = session.remote_forwards.lock();
                    map.remove(&bind_port);
                }
                emit_log(format!("[*] Remote Tunnel STOPPED & connections closed: {}:{}", bind_host, bind_port));
            }
            _ => {}
        }
    });

    TunnelRuntimeHandle {
        tunnel_id,
        bytes_counter,
        abort_tx,
    }
}

async fn handle_socks5_client(
    socket: &mut tokio::net::TcpStream,
    session: Arc<ActiveSshSession>,
    bytes_cnt: Arc<AtomicU64>,
    child_abort: &mut tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    // 1. Greeting: VER, NMETHODS, METHODS
    let mut header = [0u8; 2];
    socket.read_exact(&mut header).await?;
    if header[0] != 0x05 {
        return Ok(());
    }
    let n_methods = header[1] as usize;
    let mut methods = vec![0u8; n_methods];
    socket.read_exact(&mut methods).await?;

    // Reply: VER=5, METHOD=0 (NO AUTH)
    socket.write_all(&[0x05, 0x00]).await?;

    // 2. Request: VER, CMD, RSV, ATYP
    let mut req = [0u8; 4];
    socket.read_exact(&mut req).await?;
    if req[0] != 0x05 || req[1] != 0x01 {
        socket.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    }

    let target_host = match req[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            socket.read_exact(&mut ip).await?;
            Ipv4Addr::from(ip).to_string()
        }
        0x03 => {
            let len = socket.read_u8().await? as usize;
            let mut domain = vec![0u8; len];
            socket.read_exact(&mut domain).await?;
            String::from_utf8_lossy(&domain).to_string()
        }
        0x04 => {
            let mut ip = [0u8; 16];
            socket.read_exact(&mut ip).await?;
            Ipv6Addr::from(ip).to_string()
        }
        _ => return Ok(()),
    };

    let target_port = socket.read_u16().await? as u32;

    // 3. Open SSH direct-tcpip channel to target
    let channel_res = {
        let guard = session.ssh_handle.lock().await;
        guard
            .channel_open_direct_tcpip(target_host, target_port, "127.0.0.1", 0)
            .await
    };

    match channel_res {
        Ok(channel) => {
            socket.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await?;
            let mut ssh_stream = channel.into_stream();
            tokio::select! {
                _ = child_abort.changed() => {
                    let _ = socket.shutdown().await;
                    let _ = ssh_stream.shutdown().await;
                }
                res = copy_bidirectional(socket, &mut ssh_stream) => {
                    if let Ok((tx, rx)) = res {
                        bytes_cnt.fetch_add(tx + rx, Ordering::Relaxed);
                    }
                }
            }
        }
        Err(_) => {
            let _ = socket.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]).await;
        }
    }
    Ok(())
}
