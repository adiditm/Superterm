use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_session_type() -> String {
    "ssh".to_string()
}

fn default_keepalive_secs() -> u32 {
    30
}

fn default_jump_port() -> u16 {
    22
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HostProfile {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String, // "password", "vault", "private_key", "agent", "keyboard_interactive"
    #[serde(default)]
    pub password: Option<String>,
    pub key_path: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    pub tunnels: Vec<TunnelConfig>,
    pub last_connected: Option<String>,

    // ---- Connection tab ----
    /// "ssh" | "local_shell" | "telnet" | "serial" | "rdp" | "vnc"
    #[serde(default = "default_session_type")]
    pub session_type: String,

    // ---- Advanced tab: jump host (ProxyJump) ----
    #[serde(default)]
    pub use_jump_host: bool,
    #[serde(default)]
    pub jump_host: String,
    #[serde(default = "default_jump_port")]
    pub jump_port: u16,
    #[serde(default)]
    pub jump_username: String,

    // ---- Advanced tab: session ----
    #[serde(default)]
    pub startup_command: String,
    #[serde(default)]
    pub sftp_start_dir: String,
    /// Seconds between keep-alive probes; 0 disables.
    #[serde(default = "default_keepalive_secs")]
    pub keepalive_secs: u32,
    #[serde(default)]
    pub compression: bool,
    #[serde(default)]
    pub agent_forwarding: bool,
    #[serde(default)]
    pub auto_reconnect: bool,
    #[serde(default)]
    pub open_sftp_on_connect: bool,
}

impl HostProfile {
    /// A blank profile carrying every default, so call sites can fill only
    /// the fields they care about without repeating the whole literal.
    pub fn new_default(id: String, name: String) -> Self {
        Self {
            id,
            name,
            folder: "Production".to_string(),
            tags: vec!["ssh".to_string()],
            host: String::new(),
            port: 22,
            username: "root".to_string(),
            auth_type: "password".to_string(),
            password: None,
            key_path: None,
            passphrase: None,
            tunnels: Vec::new(),
            last_connected: None,
            session_type: default_session_type(),
            use_jump_host: false,
            jump_host: String::new(),
            jump_port: default_jump_port(),
            jump_username: String::new(),
            startup_command: String::new(),
            sftp_start_dir: String::new(),
            keepalive_secs: default_keepalive_secs(),
            compression: false,
            agent_forwarding: false,
            auto_reconnect: false,
            open_sftp_on_connect: false,
        }
    }
}

fn default_auto_start() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TunnelConfig {
    pub id: String,
    pub name: String,
    pub tunnel_type: String, // "local", "remote", "dynamic"
    pub bind_host: String,
    pub bind_port: u16,
    pub remote_host: Option<String>,
    pub remote_port: Option<u16>,
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
    pub is_active: bool,
    pub bytes_transferred: u64,
}

fn default_appearance_mode() -> String {
    "night".to_string()
}

fn default_terminal_theme() -> String {
    "Dracula".to_string()
}

fn default_font_size() -> f32 {
    14.0
}

fn default_ui_zoom() -> f32 {
    1.21
}

fn default_sync_revision() -> u32 {
    1
}

fn default_sql_port() -> u16 {
    3306
}

/// One saved SQL Client connection.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SqlConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_sql_port")]
    pub port: u16,
    pub user: String,
    pub pass: String,
    #[serde(default)]
    pub database: String,
    /// "MySQL" or "PostgreSQL".
    pub db_type: String,
}

/// One note in the Note Vault.
///
/// Stored inside `AppVault`, so notes inherit the same encryption as every
/// other secret when the vault is exported to `.stvault`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct NoteEntry {
    pub id: String,
    pub title: String,
    pub body: String,
    /// RFC 3339 timestamp of the last edit, shown in the UI.
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppVault {
    pub version: u32,
    #[serde(default = "default_appearance_mode")]
    pub appearance_mode: String,
    #[serde(default = "default_terminal_theme")]
    pub terminal_theme: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_ui_zoom")]
    pub ui_zoom: f32,
    #[serde(default)]
    pub sync_email: String,
    #[serde(default = "default_sync_revision")]
    pub sync_revision: u32,
    #[serde(default)]
    pub custom_folders: Vec<String>,
    #[serde(default)]
    pub sql_connections: Vec<SqlConfig>,
    #[serde(default)]
    pub notes: Vec<NoteEntry>,
    pub profiles: Vec<HostProfile>,
}

impl Default for AppVault {
    fn default() -> Self {
        Self {
            version: 1,
            appearance_mode: default_appearance_mode(),
            terminal_theme: default_terminal_theme(),
            font_size: default_font_size(),
            ui_zoom: default_ui_zoom(),
            sync_email: String::new(),
            sync_revision: default_sync_revision(),
            custom_folders: Vec::new(),
            sql_connections: Vec::new(),
            notes: Vec::new(),
            profiles: vec![
                HostProfile {
                    id: "host-1".to_string(),
                    name: "AWS Web Cluster 01".to_string(),
                    folder: "Production".to_string(),
                    tags: vec!["aws".to_string(), "nginx".to_string(), "sg-region".to_string()],
                    host: "13.250.12.89".to_string(),
                    port: 22,
                    username: "ubuntu".to_string(),
                    auth_type: "private_key".to_string(),
                    password: None,
                    key_path: Some("~/.ssh/id_ed25519".to_string()),
                    passphrase: None,
                    tunnels: vec![
                        TunnelConfig {
                            id: "tun-1".to_string(),
                            name: "MySQL Prod Tunnel".to_string(),
                            tunnel_type: "local".to_string(),
                            bind_host: "127.0.0.1".to_string(),
                            bind_port: 3307,
                            remote_host: Some("10.0.2.14".to_string()),
                            remote_port: Some(3306),
                            auto_start: true,
                            is_active: true,
                            bytes_transferred: 4829104,
                        },
                        TunnelConfig {
                            id: "tun-2".to_string(),
                            name: "SOCKS5 Proxy (Bypass)".to_string(),
                            tunnel_type: "dynamic".to_string(),
                            bind_host: "127.0.0.1".to_string(),
                            bind_port: 1080,
                            remote_host: None,
                            remote_port: None,
                            auto_start: false,
                            is_active: false,
                            bytes_transferred: 0,
                        },
                    ],
                    last_connected: Some("2 hours ago".to_string()),
                    ..HostProfile::new_default(String::new(), String::new())
                },
                HostProfile {
                    id: "host-2".to_string(),
                    name: "Staging API Gateway".to_string(),
                    folder: "Staging".to_string(),
                    tags: vec!["node".to_string(), "redis".to_string(), "k8s".to_string()],
                    host: "staging-api.internal".to_string(),
                    port: 2222,
                    username: "deploy".to_string(),
                    auth_type: "password".to_string(),
                    password: None,
                    key_path: None,
                    passphrase: None,
                    tunnels: vec![
                        TunnelConfig {
                            id: "tun-3".to_string(),
                            name: "Redis Cache Tunnel".to_string(),
                            tunnel_type: "local".to_string(),
                            bind_host: "127.0.0.1".to_string(),
                            bind_port: 6380,
                            remote_host: Some("127.0.0.1".to_string()),
                            remote_port: Some(6379),
                            auto_start: true,
                            is_active: true,
                            bytes_transferred: 194820,
                        },
                        TunnelConfig {
                            id: "tun-4".to_string(),
                            name: "Remote Ingress Webhook".to_string(),
                            tunnel_type: "remote".to_string(),
                            bind_host: "0.0.0.0".to_string(),
                            bind_port: 9000,
                            remote_host: Some("127.0.0.1".to_string()),
                            remote_port: Some(3000),
                            auto_start: false,
                            is_active: false,
                            bytes_transferred: 0,
                        },
                    ],
                    last_connected: Some("Yesterday".to_string()),
                    ..HostProfile::new_default(String::new(), String::new())
                },
                HostProfile {
                    id: "host-3".to_string(),
                    name: "HomeLab Proxmox Node".to_string(),
                    folder: "Homelab".to_string(),
                    tags: vec!["pve".to_string(), "lan".to_string(), "zfs".to_string()],
                    host: "192.168.1.200".to_string(),
                    port: 22,
                    username: "root".to_string(),
                    auth_type: "password".to_string(),
                    password: None,
                    key_path: None,
                    passphrase: None,
                    tunnels: vec![],
                    last_connected: Some("3 days ago".to_string()),
                    ..HostProfile::new_default(String::new(), String::new())
                },
            ],
        }
    }
}

pub fn get_vault_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("Superterm");
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir.push("superterm.vault");
    dir
}

pub fn load_vault() -> AppVault {
    let path = get_vault_path();
    if path.exists() {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(vault) = serde_json::from_slice::<AppVault>(&bytes) {
                return vault;
            }
        }
    }
    AppVault::default()
}

pub fn save_vault(vault: &AppVault) -> anyhow::Result<()> {
    let path = get_vault_path();
    let bytes = serde_json::to_vec_pretty(vault)?;
    fs::write(path, bytes)?;
    Ok(())
}

/// Export cross-platform E2EE encrypted vault (.stvault) using Argon2id + AES-256-GCM
pub fn export_encrypted_stvault(vault: &AppVault, master_password: &str, out_path: &std::path::Path) -> anyhow::Result<()> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use argon2::Argon2;
    use rand::RngCore;

    if master_password.is_empty() {
        anyhow::bail!("Master password cannot be empty.");
    }

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(master_password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Argon2 error: {}", e))?;

    let plaintext = serde_json::to_vec(vault)?;
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|e| anyhow::anyhow!("AES-GCM encrypt error: {}", e))?;

    // Header: Magic "STV1" (4 bytes) + Salt (16 bytes) + Nonce (12 bytes) + Ciphertext
    let mut output = Vec::with_capacity(4 + 16 + 12 + ciphertext.len());
    output.extend_from_slice(b"STV1");
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    fs::write(out_path, output)?;
    Ok(())
}

/// Import cross-platform E2EE encrypted vault (.stvault)
pub fn import_encrypted_stvault(master_password: &str, in_path: &std::path::Path) -> anyhow::Result<AppVault> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use argon2::Argon2;

    let data = fs::read(in_path)?;
    if data.len() < 32 || &data[0..4] != b"STV1" {
        anyhow::bail!("Invalid .stvault file format.");
    }

    let salt = &data[4..20];
    let nonce_bytes = &data[20..32];
    let ciphertext = &data[32..];

    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(master_password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Argon2 error: {}", e))?;

    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed. Incorrect master password or corrupted file."))?;

    let vault: AppVault = serde_json::from_slice(&plaintext)?;
    Ok(vault)
}
