//! Local shell sessions backed by a real PTY.
//!
//! These behave like the SSH tabs: a genuine terminal with colours, TUI apps
//! and resizing, rather than a line-buffered command runner.

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Which local shell a tab is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellKind {
    /// PowerShell on Windows, the login shell elsewhere.
    PowerShell,
    /// `cmd.exe`; Windows only.
    Cmd,
}

impl ShellKind {
    /// Menu and tab label for this shell on the current platform.
    pub fn title(self) -> &'static str {
        match self {
            #[cfg(windows)]
            ShellKind::PowerShell => "PowerShell",
            #[cfg(not(windows))]
            ShellKind::PowerShell => "Terminal",
            ShellKind::Cmd => "CMD",
        }
    }

    /// `cmd.exe` exists only on Windows.
    pub fn is_available(self) -> bool {
        match self {
            ShellKind::PowerShell => true,
            ShellKind::Cmd => cfg!(windows),
        }
    }

    /// Shells offered on this platform, in menu order.
    pub fn all() -> Vec<ShellKind> {
        let mut v = vec![ShellKind::PowerShell];
        if ShellKind::Cmd.is_available() {
            v.push(ShellKind::Cmd);
        }
        v
    }

    /// Absolute path of the executable to launch.
    ///
    /// An absolute path is used rather than a bare name because the PTY child
    /// resolves `PATH` differently from this process, and a failed lookup
    /// there surfaces as an opaque spawn error.
    fn program(self) -> Result<String, String> {
        #[cfg(windows)]
        {
            let system_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());

            let candidates: Vec<String> = match self {
                ShellKind::Cmd => vec![
                    // ComSpec is the authoritative location of cmd.exe.
                    std::env::var("ComSpec").unwrap_or_default(),
                    format!(r"{}\System32\cmd.exe", system_root),
                ],
                ShellKind::PowerShell => {
                    let mut v = Vec::new();
                    // PowerShell 7 ships outside System32 when installed.
                    if let Some(p) = which_full("pwsh.exe") {
                        v.push(p);
                    }
                    v.push(format!(
                        r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
                        system_root
                    ));
                    if let Some(p) = which_full("powershell.exe") {
                        v.push(p);
                    }
                    v
                }
            };

            for c in &candidates {
                if !c.is_empty() && std::path::Path::new(c).is_file() {
                    return Ok(c.clone());
                }
            }
            Err(format!(
                "{} was not found. Looked in: {}",
                self.title(),
                candidates
                    .iter()
                    .filter(|c| !c.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }

        #[cfg(not(windows))]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            if std::path::Path::new(&shell).is_file() {
                Ok(shell)
            } else if std::path::Path::new("/bin/sh").is_file() {
                Ok("/bin/sh".to_string())
            } else {
                Err(format!("No usable shell found (tried {} and /bin/sh)", shell))
            }
        }
    }
}

/// Full path of `exe` if it is on `PATH`.
#[cfg(windows)]
fn which_full(exe: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe))
            .find(|p| p.is_file())
            .map(|p| p.display().to_string())
    })
}

/// Events streamed from a running local shell to the UI.
pub enum ShellEvent {
    Output {
        session_key: String,
        bytes: Vec<u8>,
    },
    Closed {
        session_key: String,
        reason: String,
    },
}

/// True when this Windows build exposes the ConPTY API that PTYs rely on.
///
/// `portable-pty` panics rather than returning an error when the symbols are
/// absent, so this is checked up front.
#[cfg(windows)]
pub fn conpty_available() -> bool {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};

    extern "system" {
        fn GetModuleHandleA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    // Safe: both calls take NUL-terminated strings and only read.
    unsafe {
        let Ok(dll) = CString::new("kernel32.dll") else {
            return false;
        };
        let handle = GetModuleHandleA(dll.as_ptr());
        if handle.is_null() {
            return false;
        }
        let Ok(sym) = CString::new("CreatePseudoConsole") else {
            return false;
        };
        !GetProcAddress(handle, sym.as_ptr()).is_null()
    }
}

#[cfg(not(windows))]
pub fn conpty_available() -> bool {
    true
}

/// Scan PTY output for terminal queries and produce the replies the host
/// expects on its input side.
///
/// ConPTY opens every session with `ESC[6n` (cursor position request) and
/// blocks until it hears `ESC[row;colR`. The `vt100` emulator renders but
/// never answers, so without this the shell waits forever and the tab stays
/// blank. A couple of other common probes are answered too so TUI programs
/// that send them do not stall.
///
/// Returns the bytes to write back to the PTY. The output itself is left
/// untouched; the emulator ignores these sequences harmlessly.
fn terminal_query_replies(output: &[u8], cursor_row: u16, cursor_col: u16) -> Vec<u8> {
    let mut reply = Vec::new();
    let mut i = 0;
    while i + 2 < output.len() {
        if output[i] == 0x1b && output[i + 1] == b'[' {
            // Collect the parameter bytes up to the final letter.
            let mut j = i + 2;
            while j < output.len()
                && (output[j] == b'?' || output[j] == b'>' || output[j] == b';' || output[j].is_ascii_digit())
            {
                j += 1;
            }
            if j < output.len() {
                let params = &output[i + 2..j];
                let final_byte = output[j];
                match (params, final_byte) {
                    // DSR cursor position: ESC[6n -> ESC[row;colR (1-based)
                    (b"6", b'n') => {
                        reply.extend_from_slice(
                            format!("\x1b[{};{}R", cursor_row + 1, cursor_col + 1).as_bytes(),
                        );
                    }
                    // DSR status: ESC[5n -> ESC[0n ("ready")
                    (b"5", b'n') => reply.extend_from_slice(b"\x1b[0n"),
                    // Primary Device Attributes: ESC[c or ESC[0c
                    (b"", b'c') | (b"0", b'c') => reply.extend_from_slice(b"\x1b[?62;22c"),
                    // Secondary DA: ESC[>c -> xterm-style version string
                    (b">", b'c') | (b">0", b'c') => reply.extend_from_slice(b"\x1b[>0;10;1c"),
                    _ => {}
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    reply
}

/// Handle used by the UI to drive one live shell.
pub struct ActiveShellSession {
    tx_input: mpsc::UnboundedSender<Vec<u8>>,
    master: parking_lot::Mutex<Option<Box<dyn portable_pty::MasterPty + Send>>>,
    killer: parking_lot::Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
    /// Latest cursor position reported by the UI, used to answer DSR queries.
    cursor_row: Arc<AtomicU16>,
    cursor_col: Arc<AtomicU16>,
}

impl ActiveShellSession {
    pub fn send_bytes(&self, bytes: Vec<u8>) {
        let _ = self.tx_input.send(bytes);
    }

    pub fn send_resize(&self, cols: u16, rows: u16) {
        if let Some(master) = self.master.lock().as_ref() {
            let _ = master.resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(8),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Keep the reader's idea of the cursor current so DSR replies are right.
    pub fn report_cursor(&self, row: u16, col: u16) {
        self.cursor_row.store(row, Ordering::Relaxed);
        self.cursor_col.store(col, Ordering::Relaxed);
    }

    /// Terminate the shell; used when its tab closes.
    pub fn kill(&self) {
        if let Some(k) = self.killer.lock().as_mut() {
            let _ = k.kill();
        }
    }
}

/// Start a local shell on a PTY and stream its output back.
pub fn spawn_local_shell(
    rt: &tokio::runtime::Handle,
    session_key: String,
    kind: ShellKind,
    cols: u16,
    rows: u16,
    tx_events: mpsc::UnboundedSender<ShellEvent>,
    egui_ctx: eframe::egui::Context,
) -> Result<Arc<ActiveShellSession>, String> {
    // `portable-pty` panics if the ConPTY symbols are missing, which would
    // take the whole GUI down. Fail politely instead.
    if !conpty_available() {
        return Err(
            "This Windows build does not provide ConPTY (Windows 10 1809 or newer is required)."
                .to_string(),
        );
    }

    // Resolve the executable first: a clear "not found" beats an opaque
    // CreateProcess error from inside the PTY layer.
    let program = kind.program()?;

    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(2),
            cols: cols.max(8),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let mut cmd = CommandBuilder::new(&program);
    // PowerShell's startup banner delays the first paint.
    if matches!(kind, ShellKind::PowerShell) && cfg!(windows) {
        cmd.arg("-NoLogo");
    }
    if let Some(home) = dirs::home_dir() {
        if home.is_dir() {
            cmd.cwd(home);
        }
    }
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to start {} ({}): {}", kind.title(), program, e))?;

    // The slave handle must be dropped or the reader never sees EOF.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to read from PTY: {}", e))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to write to PTY: {}", e))?;

    let killer = parking_lot::Mutex::new(Some(child.clone_killer()));
    let cursor_row = Arc::new(AtomicU16::new(0));
    let cursor_col = Arc::new(AtomicU16::new(0));

    // All writes to the PTY go through one channel so the reader's automatic
    // query replies and the user's keystrokes never interleave mid-sequence.
    let (tx_input, mut rx_input) = mpsc::unbounded_channel::<Vec<u8>>();

    // Reader: PTY -> UI, answering terminal queries on the way.
    {
        let tx = tx_events.clone();
        let ctx = egui_ctx.clone();
        let key = session_key.clone();
        let tx_reply = tx_input.clone();
        let crow = cursor_row.clone();
        let ccol = cursor_col.clone();
        rt.spawn_blocking(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];

                        let replies = terminal_query_replies(
                            chunk,
                            crow.load(Ordering::Relaxed),
                            ccol.load(Ordering::Relaxed),
                        );
                        if !replies.is_empty() {
                            let _ = tx_reply.send(replies);
                        }

                        let _ = tx.send(ShellEvent::Output {
                            session_key: key.clone(),
                            bytes: chunk.to_vec(),
                        });
                        ctx.request_repaint();
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(ShellEvent::Closed {
                session_key: key,
                reason: "Shell exited".to_string(),
            });
            ctx.request_repaint();
        });
    }

    // Writer: (UI keystrokes + query replies) -> PTY.
    rt.spawn_blocking(move || {
        while let Some(bytes) = rx_input.blocking_recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    // Reap the child so the process does not linger as a zombie.
    {
        let mut child = child;
        rt.spawn_blocking(move || {
            let _ = child.wait();
        });
    }

    Ok(Arc::new(ActiveShellSession {
        tx_input,
        master: parking_lot::Mutex::new(Some(pair.master)),
        killer,
        cursor_row,
        cursor_col,
    }))
}

/// True when this process already holds Administrator (Windows) or root rights.
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        // An elevated process can open a handle to a system directory for
        // writing; a standard user cannot. Cheap and dependency-free.
        let test = std::path::Path::new(r"C:\Windows\System32\config\systemprofile");
        std::fs::read_dir(test).is_ok()
    }
    #[cfg(not(windows))]
    {
        // Safe: geteuid() takes no arguments and cannot fail.
        unsafe { libc_geteuid() == 0 }
    }
}

#[cfg(not(windows))]
extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

/// Relaunch Superterm with elevated rights and ask the current instance to quit.
///
/// On Windows this triggers the UAC prompt via ShellExecute's `runas` verb.
/// Elevation cannot be granted to an already-running process, so a restart is
/// the only way to obtain it.
pub fn relaunch_elevated() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot locate executable: {}", e))?;

    #[cfg(windows)]
    {
        let exe_str = exe.display().to_string();
        // PowerShell's Start-Process exposes the `runas` verb without pulling
        // in a Windows API crate.
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &format!("Start-Process -FilePath '{}' -Verb RunAs", exe_str.replace('\'', "''")),
            ])
            .spawn();

        match status {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to request elevation: {}", e)),
        }
    }

    #[cfg(target_os = "macos")]
    {
        let exe_str = exe.display().to_string();
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            exe_str.replace('\\', "\\\\").replace('"', "\\\"")
        );
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to request elevation: {}", e))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let exe_str = exe.display().to_string();
        // pkexec is the desktop-standard privilege helper; fall back to
        // a terminal sudo if it is missing.
        for (prog, args) in [
            ("pkexec", vec![exe_str.clone()]),
            ("x-terminal-emulator", vec!["-e".to_string(), format!("sudo {}", exe_str)]),
        ] {
            if std::process::Command::new(prog).args(&args).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("No graphical privilege helper (pkexec) was found.".to_string())
    }
}
