use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Semaphore};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveTool {
    Ping,
    Traceroute,
    PortScanner,
    DnsLookup,
    DnsWhois,
    NetworkInterfaces,
    SshKeyGen,
    PasswordVault,
    Sql,
    NoteVault,
}

impl ActiveTool {
    /// Menu and tab caption, in title case.
    pub fn title(&self) -> &'static str {
        match self {
            ActiveTool::Ping => "Ping",
            ActiveTool::Traceroute => "Traceroute",
            ActiveTool::PortScanner => "Port Scanner",
            ActiveTool::DnsLookup => "DNS Lookup",
            ActiveTool::DnsWhois => "DNS Whois",
            ActiveTool::NetworkInterfaces => "Network Interfaces",
            ActiveTool::SshKeyGen => "SSH Key Generator",
            ActiveTool::PasswordVault => "Password Vault",
            ActiveTool::Sql => "SQL Client",
            ActiveTool::NoteVault => "Note Vault",
        }
    }

    pub fn all() -> &'static [ActiveTool] {
        &[
            ActiveTool::Ping,
            ActiveTool::Traceroute,
            ActiveTool::PortScanner,
            ActiveTool::DnsLookup,
            ActiveTool::DnsWhois,
            ActiveTool::NetworkInterfaces,
            ActiveTool::SshKeyGen,
            ActiveTool::PasswordVault,
            ActiveTool::Sql,
            ActiveTool::NoteVault,
        ]
    }
}

pub enum ToolEvent {
    Line { tool: ActiveTool, line: String },
    Clear { tool: ActiveTool },
    Finished { tool: ActiveTool },
}

fn emit_line(
    tx: &mpsc::UnboundedSender<ToolEvent>,
    ctx: &eframe::egui::Context,
    tool: ActiveTool,
    line: String,
) {
    let _ = tx.send(ToolEvent::Line { tool, line });
    ctx.request_repaint();
}

fn run_command_streaming(
    program: &str,
    args: &[String],
    tool: ActiveTool,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.spawn() {
        Ok(mut child) => {
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    emit_line(&tx, &ctx, tool.clone(), line);
                }
            }
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        emit_line(&tx, &ctx, tool.clone(), line);
                    }
                }
            }
            let _ = child.wait();
        }
        Err(e) => {
            emit_line(&tx, &ctx, tool.clone(), format!("Failed to execute {}: {}", program, e));
        }
    }
    let _ = tx.send(ToolEvent::Finished { tool });
    ctx.request_repaint();
}

pub fn spawn_ping(
    rt: &tokio::runtime::Handle,
    target: String,
    count: u32,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let target = target.trim().to_string();
    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::Ping });
    emit_line(&tx, &ctx, ActiveTool::Ping, format!("$ ping {} (count={})", target, count));

    rt.spawn_blocking(move || {
        #[cfg(windows)]
        let args = vec!["-n".to_string(), count.to_string(), target];
        #[cfg(not(windows))]
        let args = vec!["-c".to_string(), count.to_string(), target];

        run_command_streaming("ping", &args, ActiveTool::Ping, tx, ctx);
    });
}

pub fn spawn_traceroute(
    rt: &tokio::runtime::Handle,
    target: String,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let target = target.trim().to_string();
    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::Traceroute });
    emit_line(&tx, &ctx, ActiveTool::Traceroute, format!("$ tracert -d -h 20 {}", target));

    rt.spawn_blocking(move || {
        #[cfg(windows)]
        let args = vec!["-d".to_string(), "-h".to_string(), "20".to_string(), "-w".to_string(), "1500".to_string(), target];
        #[cfg(not(windows))]
        let args = vec!["-n".to_string(), "-m".to_string(), "20".to_string(), target];

        #[cfg(windows)]
        let prog = "tracert";
        #[cfg(not(windows))]
        let prog = "traceroute";

        run_command_streaming(prog, &args, ActiveTool::Traceroute, tx, ctx);
    });
}

fn parse_port_spec(spec: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for part in spec.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some((start_s, end_s)) = p.split_once('-') {
            if let (Ok(s), Ok(e)) = (start_s.trim().parse::<u16>(), end_s.trim().parse::<u16>()) {
                let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
                for port in lo..=hi {
                    ports.push(port);
                }
            }
        } else if let Ok(port) = p.parse::<u16>() {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn guess_service_name(port: u16) -> &'static str {
    match port {
        21 => "FTP",
        22 | 228 | 2222 | 4322 => "SSH",
        23 => "Telnet",
        25 | 587 => "SMTP",
        53 => "DNS",
        80 | 8080 | 8000 => "HTTP",
        110 => "POP3",
        143 => "IMAP",
        443 | 8443 => "HTTPS",
        445 => "SMB",
        1080 => "SOCKS5",
        1433 => "MSSQL",
        3306 => "MySQL",
        3389 => "RDP",
        5432 | 5433 | 5434 | 6432 => "PostgreSQL",
        6379 => "Redis",
        27017 => "MongoDB",
        _ => "TCP",
    }
}

pub fn spawn_port_scan(
    rt: &tokio::runtime::Handle,
    host: String,
    port_spec: String,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let host = host.trim().to_string();
    let ports = parse_port_spec(&port_spec);
    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::PortScanner });

    if ports.is_empty() {
        emit_line(&tx, &ctx, ActiveTool::PortScanner, "[!] No valid ports specified (e.g. 22,80,443,3306 or 1-1024).".to_string());
        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::PortScanner });
        return;
    }

    emit_line(
        &tx,
        &ctx,
        ActiveTool::PortScanner,
        format!("Scanning {} ({} ports)...", host, ports.len()),
    );

    rt.spawn(async move {
        let resolved: Vec<SocketAddr> = match tokio::net::lookup_host((host.as_str(), 0)).await {
            Ok(addrs) => addrs.collect(),
            Err(e) => {
                emit_line(&tx, &ctx, ActiveTool::PortScanner, format!("[!] DNS resolution failed for {}: {}", host, e));
                let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::PortScanner });
                ctx.request_repaint();
                return;
            }
        };

        let base_ip = match resolved.first() {
            Some(a) => a.ip(),
            None => {
                emit_line(&tx, &ctx, ActiveTool::PortScanner, "[!] Could not resolve host IP.".to_string());
                let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::PortScanner });
                ctx.request_repaint();
                return;
            }
        };

        emit_line(&tx, &ctx, ActiveTool::PortScanner, format!("Resolved IP: {}\n", base_ip));

        let sem = Arc::new(Semaphore::new(64));
        let open_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::new();

        for port in ports {
            let permit = sem.clone().acquire_owned().await.unwrap();
            let tx_c = tx.clone();
            let ctx_c = ctx.clone();
            let oc = open_count.clone();
            let target_addr = SocketAddr::new(base_ip, port);

            handles.push(tokio::spawn(async move {
                let _p = permit;
                let start = std::time::Instant::now();
                let res = tokio::time::timeout(
                    std::time::Duration::from_millis(750),
                    TcpStream::connect(target_addr),
                )
                .await;

                if let Ok(Ok(stream)) = res {
                    drop(stream);
                    let ms = start.elapsed().as_millis();
                    oc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    emit_line(
                        &tx_c,
                        &ctx_c,
                        ActiveTool::PortScanner,
                        format!("  [OPEN]  {:>5}/tcp   {:<12} ({} ms)", port, guess_service_name(port), ms),
                    );
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }

        let found = open_count.load(std::sync::atomic::Ordering::Relaxed);
        emit_line(
            &tx,
            &ctx,
            ActiveTool::PortScanner,
            format!("\nScan complete. {} open port(s) found.", found),
        );
        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::PortScanner });
        ctx.request_repaint();
    });
}

pub fn spawn_dns_lookup(
    rt: &tokio::runtime::Handle,
    domain: String,
    record_type: String,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let domain = domain.trim().to_string();
    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::DnsLookup });
    emit_line(&tx, &ctx, ActiveTool::DnsLookup, format!("DNS Lookup for '{}' [type={}]\n", domain, record_type));

    rt.spawn(async move {
        if record_type == "ALL" || record_type == "A" || record_type == "AAAA" {
            emit_line(&tx, &ctx, ActiveTool::DnsLookup, "--- Native OS Resolver ---".to_string());
            match tokio::net::lookup_host((domain.as_str(), 0)).await {
                Ok(addrs) => {
                    for addr in addrs {
                        emit_line(&tx, &ctx, ActiveTool::DnsLookup, format!("  {}", addr.ip()));
                    }
                }
                Err(e) => {
                    emit_line(&tx, &ctx, ActiveTool::DnsLookup, format!("  Error: {}", e));
                }
            }
            emit_line(&tx, &ctx, ActiveTool::DnsLookup, "".to_string());
        }

        let types_to_query: Vec<&str> = if record_type == "ALL" {
            vec!["A", "AAAA", "MX", "NS", "TXT", "CNAME"]
        } else {
            vec![record_type.as_str()]
        };

        for rtype in types_to_query {
            let dom = domain.clone();
            let rt_str = rtype.to_string();
            let out = tokio::task::spawn_blocking(move || {
                let mut cmd = std::process::Command::new("nslookup");
                cmd.arg(format!("-type={}", rt_str))
                    .arg(&dom)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                #[cfg(windows)]
                cmd.creation_flags(CREATE_NO_WINDOW);

                let output = cmd.output();
                match output {
                    Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
                    Err(e) => format!("Failed to run nslookup: {}", e),
                }
            })
            .await
            .unwrap_or_default();

            emit_line(&tx, &ctx, ActiveTool::DnsLookup, format!("--- {} Records ---", rtype));
            for line in out.lines() {
                if !line.trim().is_empty() {
                    emit_line(&tx, &ctx, ActiveTool::DnsLookup, format!("  {}", line));
                }
            }
            emit_line(&tx, &ctx, ActiveTool::DnsLookup, "".to_string());
        }

        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::DnsLookup });
        ctx.request_repaint();
    });
}

fn known_whois_server(tld: &str) -> Option<&'static str> {
    match tld {
        // Generic TLDs
        "com" | "net" => Some("whois.verisign-grs.com"),
        "org" => Some("whois.pir.org"),
        "info" => Some("whois.afilias.net"),
        "biz" => Some("whois.biz"),
        "io" => Some("whois.nic.io"),
        "dev" | "app" | "page" => Some("whois.nic.google"),
        "ai" => Some("whois.nic.ai"),
        "me" => Some("whois.nic.me"),
        "co" => Some("whois.nic.co"),
        "xyz" => Some("whois.nic.xyz"),
        "cloud" => Some("whois.nic.cloud"),
        "tech" | "online" | "site" | "store" => Some("whois.nic.tech"),
        // Country Code TLDs (ccTLDs)
        "id" => Some("whois.id"),
        "my" => Some("whois.mynic.my"),
        "sg" => Some("whois.sgnic.sg"),
        "uk" => Some("whois.nic.uk"),
        "us" => Some("whois.nic.us"),
        "au" => Some("whois.auda.org.au"),
        "jp" => Some("whois.jprs.jp"),
        "in" => Some("whois.registry.in"),
        "mx" => Some("whois.mx"),
        "de" => Some("whois.denic.de"),
        "fr" => Some("whois.nic.fr"),
        "nl" => Some("whois.domain-registry.nl"),
        "ca" => Some("whois.cira.ca"),
        "cn" => Some("whois.cnnic.cn"),
        "tw" => Some("whois.twnic.net.tw"),
        "kr" => Some("whois.kr"),
        "ru" => Some("whois.tcinet.ru"),
        "br" => Some("whois.registro.br"),
        "eu" => Some("whois.eu"),
        "hk" => Some("whois.hkirc.hk"),
        "nz" => Some("whois.irs.net.nz"),
        "th" => Some("whois.thnic.co.th"),
        "vn" => Some("whois.vnnic.vn"),
        "ph" => Some("whois.dot.ph"),
        _ => None,
    }
}

async fn whois_tcp_query(server: &str, query: &str) -> anyhow::Result<String> {
    let addr = format!("{}:43", server);
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(&addr),
    )
    .await??;

    let formatted_query = if server.eq_ignore_ascii_case("whois.verisign-grs.com") {
        format!("domain {}\r\n", query)
    } else if server.eq_ignore_ascii_case("whois.jprs.jp") {
        format!("{}/e\r\n", query)
    } else if server.eq_ignore_ascii_case("whois.denic.de") {
        format!("-T dn,ace {}\r\n", query)
    } else {
        format!("{}\r\n", query)
    };

    stream.write_all(formatted_query.as_bytes()).await?;
    stream.flush().await?;

    let mut buf = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        stream.read_to_end(&mut buf),
    )
    .await;

    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn extract_referral_server(response: &str, keys: &[&str]) -> Option<String> {
    for line in response.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        for k in keys {
            if lower.starts_with(k) {
                if let Some((_, val)) = trimmed.split_once(':') {
                    let candidate = val
                        .trim()
                        .trim_start_matches("whois://")
                        .trim_start_matches("http://")
                        .trim_start_matches("https://")
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !candidate.is_empty() && candidate.contains('.') {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn spawn_whois(
    rt: &tokio::runtime::Handle,
    raw_input: String,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let cleaned = raw_input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();

    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::DnsWhois });

    if cleaned.is_empty() || !cleaned.contains('.') {
        emit_line(&tx, &ctx, ActiveTool::DnsWhois, "[!] Please enter a valid domain name (e.g. example.com, google.co.id, nic.io).".to_string());
        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::DnsWhois });
        return;
    }

    rt.spawn(async move {
        let tld = cleaned.rsplit('.').next().unwrap_or("");
        emit_line(
            &tx,
            &ctx,
            ActiveTool::DnsWhois,
            format!("WHOIS Lookup for '{}' (TLD: .{})\n", cleaned, tld),
        );

        // Step 1: Determine authoritative registry WHOIS server (built-in fast map or dynamic IANA lookup)
        let registry_server = if let Some(srv) = known_whois_server(tld) {
            srv.to_string()
        } else {
            emit_line(&tx, &ctx, ActiveTool::DnsWhois, "[*] Querying whois.iana.org for TLD/ccTLD referral...".to_string());
            match whois_tcp_query("whois.iana.org", tld).await {
                Ok(iana_resp) => {
                    if let Some(refer) = extract_referral_server(&iana_resp, &["refer:", "whois:"]) {
                        refer
                    } else {
                        emit_line(&tx, &ctx, ActiveTool::DnsWhois, "[!] IANA did not return a WHOIS referral server. Showing IANA output:\n".to_string());
                        for l in iana_resp.lines() {
                            emit_line(&tx, &ctx, ActiveTool::DnsWhois, l.to_string());
                        }
                        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::DnsWhois });
                        ctx.request_repaint();
                        return;
                    }
                }
                Err(e) => {
                    emit_line(&tx, &ctx, ActiveTool::DnsWhois, format!("[!] Failed querying whois.iana.org: {}", e));
                    let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::DnsWhois });
                    ctx.request_repaint();
                    return;
                }
            }
        };

        emit_line(&tx, &ctx, ActiveTool::DnsWhois, format!("[*] Registry WHOIS Server: {}\n", registry_server));

        // Step 2: Query authoritative TLD/ccTLD registry server
        match whois_tcp_query(&registry_server, &cleaned).await {
            Ok(reg_resp) => {
                for l in reg_resp.lines() {
                    emit_line(&tx, &ctx, ActiveTool::DnsWhois, l.to_string());
                }

                // Step 3: Follow Registrar WHOIS referral if provided (common for .com, .net, .org, etc.)
                if let Some(registrar_srv) = extract_referral_server(
                    &reg_resp,
                    &["registrar whois server:", "whois server:", "refer:"],
                ) {
                    if !registrar_srv.eq_ignore_ascii_case(&registry_server) {
                        emit_line(
                            &tx,
                            &ctx,
                            ActiveTool::DnsWhois,
                            format!("\n[*] Following Registrar WHOIS Server: {}\n--------------------------------------------------", registrar_srv),
                        );
                        match whois_tcp_query(&registrar_srv, &cleaned).await {
                            Ok(ref_resp) => {
                                for l in ref_resp.lines() {
                                    emit_line(&tx, &ctx, ActiveTool::DnsWhois, l.to_string());
                                }
                            }
                            Err(e) => {
                                emit_line(&tx, &ctx, ActiveTool::DnsWhois, format!("[!] Registrar referral query failed: {}", e));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                emit_line(&tx, &ctx, ActiveTool::DnsWhois, format!("[!] Failed querying {}: {}", registry_server, e));
            }
        }

        let _ = tx.send(ToolEvent::Finished { tool: ActiveTool::DnsWhois });
        ctx.request_repaint();
    });
}

pub fn spawn_network_interfaces(
    rt: &tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<ToolEvent>,
    ctx: eframe::egui::Context,
) {
    let _ = tx.send(ToolEvent::Clear { tool: ActiveTool::NetworkInterfaces });
    emit_line(&tx, &ctx, ActiveTool::NetworkInterfaces, "Inspecting local network interfaces...\n".to_string());

    rt.spawn_blocking(move || {
        // Detect primary local outbound IP via UDP connect trick (no packet sent)
        if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if sock.connect("8.8.8.8:80").is_ok() {
                if let Ok(local_addr) = sock.local_addr() {
                    emit_line(
                        &tx,
                        &ctx,
                        ActiveTool::NetworkInterfaces,
                        format!("Primary Outbound IPv4: {}\n", local_addr.ip()),
                    );
                }
            }
        }

        #[cfg(windows)]
        run_command_streaming("ipconfig", &["/all".to_string()], ActiveTool::NetworkInterfaces, tx, ctx);
        #[cfg(target_os = "macos")]
        run_command_streaming("ifconfig", &[], ActiveTool::NetworkInterfaces, tx, ctx);
        #[cfg(all(not(windows), not(target_os = "macos")))]
        run_command_streaming("ip", &["addr".to_string()], ActiveTool::NetworkInterfaces, tx, ctx);
    });
}

pub fn get_ssh_dir() -> PathBuf {
    let mut dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push(".ssh");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

pub fn list_ssh_keys() -> Vec<(String, String)> {
    let ssh_dir = get_ssh_dir();
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&ssh_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("pub") {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("key.pub")
                    .to_string();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                results.push((name, content.trim().to_string()));
            }
        }
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

pub fn generate_ssh_keypair(
    filename: &str,
    algo: &str,
    comment: &str,
    passphrase: &str,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    use ssh_key::{Algorithm, LineEnding, PrivateKey};

    let clean_name = filename.trim();
    if clean_name.is_empty() {
        anyhow::bail!("Key file name cannot be empty.");
    }

    let ssh_dir = get_ssh_dir();
    let priv_path = ssh_dir.join(clean_name);
    let pub_path = ssh_dir.join(format!("{}.pub", clean_name));

    let alg = match algo {
        "ECDSA-P256" => Algorithm::Ecdsa {
            curve: ssh_key::EcdsaCurve::NistP256,
        },
        _ => Algorithm::Ed25519,
    };

    let mut priv_key = PrivateKey::random(&mut rand::rngs::OsRng, alg)?;
    if !comment.trim().is_empty() {
        priv_key.set_comment(comment.trim());
    }

    let final_priv = if !passphrase.is_empty() {
        priv_key.encrypt(&mut rand::rngs::OsRng, passphrase)?
    } else {
        priv_key
    };

    let priv_pem = final_priv.to_openssh(LineEnding::LF)?;
    let pub_openssh = final_priv.public_key().to_openssh()?;

    std::fs::write(&priv_path, priv_pem.as_bytes())?;
    std::fs::write(&pub_path, format!("{}\n", pub_openssh))?;

    Ok((priv_path, pub_path, pub_openssh))
}
