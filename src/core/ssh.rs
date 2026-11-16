use async_trait::async_trait;
use parking_lot::Mutex;
use russh::client;
use russh::{Channel, ChannelMsg};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

pub enum SshEvent {
    Connected {
        session_key: String,
        host_id: String,
        session: Arc<ActiveSshSession>,
    },
    Output {
        session_key: String,
        bytes: Vec<u8>,
    },
    Error {
        session_key: String,
        message: String,
    },
    Disconnected {
        session_key: String,
        host_id: String,
        reason: String,
    },
    HostKeyPrompt {
        session_key: String,
        host: String,
        port: u16,
        key_type: String,
        fingerprint: String,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    HostKeyMismatch {
        session_key: String,
        host: String,
        port: u16,
        fingerprint: String,
        line: usize,
    },
    TunnelLog {
        host_id: String,
        message: String,
    },
}

pub struct ActiveSshSession {
    #[allow(dead_code)]
    pub session_key: String,
    pub host_id: String,
    pub tx_input: mpsc::Sender<Vec<u8>>,
    pub tx_resize: mpsc::Sender<(u32, u32)>,
    pub ssh_handle: Arc<tokio::sync::Mutex<client::Handle<SshClientHandler>>>,
    pub remote_forwards: Arc<Mutex<HashMap<u32, (String, u16, tokio::sync::watch::Receiver<bool>)>>>,
    pub task_abort: tokio::sync::watch::Sender<bool>,
}

impl ActiveSshSession {
    pub fn send_bytes(&self, bytes: Vec<u8>) {
        let _ = self.tx_input.try_send(bytes);
    }

    pub fn send_resize(&self, cols: u32, rows: u32) {
        let _ = self.tx_resize.try_send((cols, rows));
    }

    pub fn disconnect(&self) {
        let _ = self.task_abort.send(true);
    }
}

pub struct SshClientHandler {
    pub remote_forwards: Arc<Mutex<HashMap<u32, (String, u16, tokio::sync::watch::Receiver<bool>)>>>,
    pub tx_events: mpsc::UnboundedSender<SshEvent>,
    pub host: String,
    pub port: u16,
    pub session_key: String,
}

#[async_trait]
impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint();
        let key_type = server_public_key.name().to_string();

        match russh_keys::check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => {
                // Host key matches known_hosts
                Ok(true)
            }
            Ok(false) => {
                // Unknown host key: Trust On First Use (TOFU) prompt to user
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let _ = self.tx_events.send(SshEvent::HostKeyPrompt {
                    session_key: self.session_key.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    reply: reply_tx,
                });

                match reply_rx.await {
                    Ok(true) => {
                        // User trusted host key: save to known_hosts
                        if let Err(e) = russh_keys::learn_known_hosts(&self.host, self.port, server_public_key) {
                            eprintln!("Warning: failed to write to known_hosts: {:?}", e);
                        }
                        Ok(true)
                    }
                    _ => {
                        // User rejected or canceled
                        Ok(false)
                    }
                }
            }
            Err(russh_keys::Error::KeyChanged { line }) => {
                let _ = self.tx_events.send(SshEvent::HostKeyMismatch {
                    session_key: self.session_key.clone(),
                    host: self.host.clone(),
                    port: self.port,
                    fingerprint,
                    line,
                });
                Err(russh::Error::UnknownKey)
            }
            Err(e) => {
                let _ = self.tx_events.send(SshEvent::Error {
                    session_key: self.session_key.clone(),
                    message: format!("Host key verification error: {:?}", e),
                });
                Err(russh::Error::UnknownKey)
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let target = {
            let map = self.remote_forwards.lock();
            map.get(&connected_port).cloned()
        };

        if let Some((local_host, local_port, mut abort_rx)) = target {
            tokio::spawn(async move {
                if *abort_rx.borrow() {
                    return;
                }
                let addr = format!("{}:{}", local_host, local_port);
                if let Ok(mut local_stream) = TcpStream::connect(&addr).await {
                    let mut ssh_stream = channel.into_stream();
                    tokio::select! {
                        _ = abort_rx.changed() => {
                            let _ = local_stream.shutdown().await;
                            let _ = ssh_stream.shutdown().await;
                        }
                        _ = copy_bidirectional(&mut local_stream, &mut ssh_stream) => {}
                    }
                }
            });
        }
        Ok(())
    }
}

pub fn spawn_live_ssh_connection(
    rt: &tokio::runtime::Handle,
    session_key: String,
    host_id: String,
    host: String,
    port: u16,
    username: String,
    auth_type: String,
    password: Option<String>,
    key_path: Option<String>,
    passphrase: Option<String>,
    initial_cols: u32,
    initial_rows: u32,
    tx_events: mpsc::UnboundedSender<SshEvent>,
    egui_ctx: eframe::egui::Context,
) {
    rt.spawn(async move {
        let emit = |ev: SshEvent| {
            let _ = tx_events.send(ev);
            egui_ctx.request_repaint();
        };

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            keepalive_interval: Some(std::time::Duration::from_secs(20)),
            ..Default::default()
        });

        let remote_forwards = Arc::new(Mutex::new(HashMap::new()));
        let handler = SshClientHandler {
            remote_forwards: remote_forwards.clone(),
            tx_events: tx_events.clone(),
            host: host.clone(),
            port,
            session_key: session_key.clone(),
        };

        let addr = format!("{}:{}", host, port);
        let mut session = match tokio::time::timeout(
            std::time::Duration::from_secs(12),
            client::connect(config, addr.as_str(), handler),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                emit(SshEvent::Error {
                    session_key,
                    message: format!("TCP/SSH connection to {} failed: {:?}", addr, e),
                });
                return;
            }
            Err(_) => {
                emit(SshEvent::Error {
                    session_key,
                    message: format!("Connection to {} timed out (12s)", addr),
                });
                return;
            }
        };

        // Authentication
        let auth_ok = if auth_type == "private_key" {
            if let Some(ref kp) = key_path {
                let expanded_path = if kp.starts_with("~/") || kp.starts_with("~\\") {
                    if let Some(home) = dirs::home_dir() {
                        home.join(&kp[2..])
                    } else {
                        std::path::PathBuf::from(kp)
                    }
                } else {
                    std::path::PathBuf::from(kp)
                };

                match russh_keys::load_secret_key(&expanded_path, passphrase.as_deref()) {
                    Ok(key_pair) => match session.authenticate_publickey(&username, Arc::new(key_pair)).await {
                        Ok(res) => res,
                        Err(e) => {
                            emit(SshEvent::Error {
                                session_key,
                                message: format!("Auth key error: {:?}", e),
                            });
                            return;
                        }
                    },
                    Err(e) => {
                        emit(SshEvent::Error {
                            session_key,
                            message: format!("Failed to read private key ({}): {:?}", expanded_path.display(), e),
                        });
                        return;
                    }
                }
            } else {
                emit(SshEvent::Error {
                    session_key,
                    message: "Private key path is empty.".to_string(),
                });
                return;
            }
        } else {
            let pwd = password.unwrap_or_default();
            match session.authenticate_password(&username, &pwd).await {
                Ok(res) => res,
                Err(e) => {
                    emit(SshEvent::Error {
                        session_key,
                        message: format!("Auth password error: {:?}", e),
                    });
                    return;
                }
            }
        };

        if !auth_ok {
            emit(SshEvent::Error {
                session_key,
                message: format!("Authentication rejected by {} (check username / password / key).", host),
            });
            return;
        }

        // Open PTY Session Channel
        let mut channel = match session.channel_open_session().await {
            Ok(ch) => ch,
            Err(e) => {
                emit(SshEvent::Error {
                    session_key,
                    message: format!("Failed to open PTY channel: {:?}", e),
                });
                return;
            }
        };

        if let Err(e) = channel
            .request_pty(true, "xterm-256color", initial_cols.max(20), initial_rows.max(8), 0, 0, &[])
            .await
        {
            emit(SshEvent::Error {
                session_key,
                message: format!("Failed to request PTY: {:?}", e),
            });
            return;
        }

        if let Err(e) = channel.request_shell(true).await {
            emit(SshEvent::Error {
                session_key,
                message: format!("Failed to start shell: {:?}", e),
            });
            return;
        }

        let (tx_input, mut rx_input) = mpsc::channel::<Vec<u8>>(512);
        let (tx_resize, mut rx_resize) = mpsc::channel::<(u32, u32)>(32);
        let (abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);

        let ssh_handle = Arc::new(tokio::sync::Mutex::new(session));
        let active_session = Arc::new(ActiveSshSession {
            session_key: session_key.clone(),
            host_id: host_id.clone(),
            tx_input,
            tx_resize,
            ssh_handle: ssh_handle.clone(),
            remote_forwards,
            task_abort: abort_tx,
        });

        emit(SshEvent::Connected {
            session_key: session_key.clone(),
            host_id: host_id.clone(),
            session: active_session,
        });

        // Bidirectional PTY loop
        loop {
            tokio::select! {
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        let _ = channel.eof().await;
                        let _ = channel.close().await;
                        emit(SshEvent::Disconnected {
                            session_key: session_key.clone(),
                            host_id: host_id.clone(),
                            reason: "Disconnected by user".to_string(),
                        });
                        break;
                    }
                }
                Some((cols, rows)) = rx_resize.recv() => {
                    let _ = channel.window_change(cols.max(20), rows.max(8), 0, 0).await;
                }
                Some(input_bytes) = rx_input.recv() => {
                    if channel.data(&input_bytes[..]).await.is_err() {
                        emit(SshEvent::Disconnected {
                            session_key: session_key.clone(),
                            host_id: host_id.clone(),
                            reason: "SSH write stream closed".to_string(),
                        });
                        break;
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            emit(SshEvent::Output {
                                session_key: session_key.clone(),
                                bytes: data.to_vec(),
                            });
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            emit(SshEvent::Output {
                                session_key: session_key.clone(),
                                bytes: data.to_vec(),
                            });
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                            emit(SshEvent::Disconnected {
                                session_key: session_key.clone(),
                                host_id: host_id.clone(),
                                reason: "Remote shell closed".to_string(),
                            });
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });
}
