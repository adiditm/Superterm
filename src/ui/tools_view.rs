use crate::core::localshell::ShellKind;
use crate::core::tools::{
    generate_ssh_keypair, get_ssh_dir, list_ssh_keys, spawn_dns_lookup, spawn_network_interfaces,
    spawn_ping, spawn_port_scan, spawn_traceroute, spawn_whois, ActiveTool, ToolEvent,
};
use crate::core::sqlclient::{primary_table_name, spawn_query, DbKind, QueryOutcome, SqlEvent};
use crate::core::vault::{get_vault_path, AppVault, NoteEntry, SqlConfig};
use eframe::egui::{self, Color32, Pos2, RichText, Rounding, ScrollArea, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

pub struct ToolsState {
    pub tx_events: mpsc::UnboundedSender<ToolEvent>,
    pub rx_events: mpsc::UnboundedReceiver<ToolEvent>,
    pub outputs: HashMap<&'static str, Vec<String>>,
    pub running: HashSet<&'static str>,

    // Per-tool inputs
    pub ping_target: String,
    pub ping_count: u32,
    pub traceroute_target: String,
    pub scan_target: String,
    pub scan_ports: String,
    pub dns_target: String,
    pub dns_type: String,
    pub whois_target: String,

    // SSH Key Generator
    pub keygen_name: String,
    pub keygen_algo: String,
    pub keygen_comment: String,
    pub keygen_passphrase: String,
    pub keygen_last_pub: Option<String>,
    pub keygen_status: Option<String>,
    pub existing_keys: Vec<(String, String)>,

    // Password Vault
    pub revealed_host_ids: HashSet<String>,
    pub vault_notice: Option<String>,

    /// Deferred vault writes shared by every SQL session; the UI cannot
    /// borrow the vault mutably while drawing.
    pub sql_save_req: Option<SqlConfig>,
    pub sql_delete_req: Option<String>,

    // ---- Note Vault ----
    /// Id of the note currently open.
    pub note_selected: Option<String>,
    /// Working copies, so typing does not write to the vault on every keypress.
    pub note_title_buf: String,
    pub note_body_buf: String,
    /// True once the buffers differ from what is stored.
    pub note_dirty: bool,
    /// Note awaiting delete confirmation: (id, title).
    pub note_confirm_delete: Option<(String, String)>,
    /// Deferred vault writes, applied by the app after drawing.
    pub note_save_req: Option<NoteEntry>,
    pub note_delete_req: Option<String>,
    pub note_create_req: bool,
    pub note_notice: Option<String>,
}

/// One SQL Client session: its own connection, editor and result tabs.
///
/// Instanced per workspace tab so several databases can be open at once.
pub struct SqlSession {
    pub rx: mpsc::UnboundedReceiver<SqlEvent>,
    pub tx: mpsc::UnboundedSender<SqlEvent>,
    /// Vault id of the connection being edited, if it is a saved one.
    pub selected_id: Option<String>,
    pub conn_name: String,
    pub kind: DbKind,
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
    pub database: String,
    pub reveal_pass: bool,
    pub query: String,
    /// One tab per executed statement; a new run never overwrites an old one.
    pub tabs: Vec<SqlResultTab>,
    pub active_tab: Option<u64>,
    /// Monotonic source of result-tab ids within this session.
    pub next_tab_id: u64,
    /// Result tab whose query is open in the edit dialog, plus its working copy.
    pub editing_tab: Option<u64>,
    pub edit_buffer: String,
}

impl SqlSession {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            rx,
            tx,
            selected_id: None,
            conn_name: String::new(),
            kind: DbKind::MySql,
            host: "127.0.0.1".to_string(),
            port: DbKind::MySql.default_port().to_string(),
            user: "root".to_string(),
            pass: String::new(),
            database: String::new(),
            reveal_pass: false,
            query: "SELECT 1;".to_string(),
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            editing_tab: None,
            edit_buffer: String::new(),
        }
    }

    /// Short label for the workspace tab: the saved name, else the database,
    /// else the host.
    pub fn tab_title(&self) -> String {
        if !self.conn_name.trim().is_empty() {
            return self.conn_name.trim().to_string();
        }
        if !self.database.trim().is_empty() {
            return self.database.trim().to_string();
        }
        if !self.host.trim().is_empty() {
            return self.host.trim().to_string();
        }
        "SQL".to_string()
    }

    /// Copy a saved connection into this session's fields.
    pub fn load_config(&mut self, cfg: &SqlConfig) {
        self.selected_id = Some(cfg.id.clone());
        self.conn_name = cfg.name.clone();
        self.kind = DbKind::from_str(&cfg.db_type);
        self.host = cfg.host.clone();
        self.port = cfg.port.to_string();
        self.user = cfg.user.clone();
        self.pass = cfg.pass.clone();
        self.database = cfg.database.clone();
        self.reveal_pass = false;
    }

    /// Clear the connection fields so a new one can be entered.
    pub fn reset_editor(&mut self) {
        self.selected_id = None;
        self.conn_name = String::new();
        self.kind = DbKind::MySql;
        self.host = "127.0.0.1".to_string();
        self.port = DbKind::MySql.default_port().to_string();
        self.user = "root".to_string();
        self.pass = String::new();
        self.database = String::new();
        self.reveal_pass = false;
    }

    /// Open a result tab for a statement about to run, and return its id.
    pub fn begin_tab(&mut self, sql: &str) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;

        let title = primary_table_name(sql).unwrap_or_else(|| format!("Result {}", id));

        self.tabs.push(SqlResultTab {
            id,
            title,
            sql: sql.to_string(),
            running: true,
            status: Some("Running...".to_string()),
            error: false,
            columns: Vec::new(),
            rows: Vec::new(),
            truncated: false,
        });
        self.active_tab = Some(id);
        id
    }

    /// Reset a result tab in place so an edited query reuses it.
    pub fn restart_tab(&mut self, tab_id: u64, sql: &str) {
        let title = primary_table_name(sql).unwrap_or_else(|| format!("Result {}", tab_id));
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.title = title;
            tab.sql = sql.to_string();
            tab.running = true;
            tab.status = Some("Running...".to_string());
            tab.error = false;
            tab.columns.clear();
            tab.rows.clear();
            tab.truncated = false;
        }
        self.active_tab = Some(tab_id);
    }

    /// Drain query results produced on the Tokio runtime into their tabs.
    pub fn process_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                SqlEvent::Started { tab_id } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.running = true;
                        tab.status = Some("Running...".to_string());
                        tab.error = false;
                    }
                }
                SqlEvent::Finished {
                    tab_id,
                    outcome,
                    elapsed_ms,
                } => {
                    // The tab may have been closed while the query ran.
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    tab.running = false;

                    match outcome {
                        QueryOutcome::Rows {
                            columns,
                            rows,
                            truncated,
                        } => {
                            tab.error = false;
                            tab.status = Some(format!(
                                "{} row(s) returned in {} ms{}",
                                rows.len(),
                                elapsed_ms,
                                if truncated {
                                    format!(" (first {} shown)", rows.len())
                                } else {
                                    String::new()
                                }
                            ));
                            tab.columns = columns;
                            tab.rows = rows;
                            tab.truncated = truncated;
                        }
                        QueryOutcome::Affected { count } => {
                            tab.error = false;
                            tab.status =
                                Some(format!("{} row(s) affected in {} ms", count, elapsed_ms));
                            tab.columns.clear();
                            tab.rows.clear();
                            tab.truncated = false;
                        }
                        QueryOutcome::Error { message } => {
                            tab.error = true;
                            tab.status = Some(message);
                            tab.columns.clear();
                            tab.rows.clear();
                            tab.truncated = false;
                        }
                    }
                }
            }
        }
    }
}

impl Default for SqlSession {
    fn default() -> Self {
        Self::new()
    }
}

/// One result tab: the statement that produced it plus whatever came back.
pub struct SqlResultTab {
    pub id: u64,
    /// Caption, normally the first table the statement touches.
    pub title: String,
    /// The exact SQL this tab ran, so it can be copied or re-run after editing.
    pub sql: String,
    pub running: bool,
    pub status: Option<String>,
    pub error: bool,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

impl ToolsState {
    pub fn new() -> Self {
        let (tx_events, rx_events) = mpsc::unbounded_channel();
        Self {
            tx_events,
            rx_events,
            outputs: HashMap::new(),
            running: HashSet::new(),
            ping_target: "8.8.8.8".to_string(),
            ping_count: 4,
            traceroute_target: "1.1.1.1".to_string(),
            scan_target: "127.0.0.1".to_string(),
            scan_ports: "21,22,80,443,2222,3306,3389,4322,5432,6379,8080".to_string(),
            dns_target: "example.com".to_string(),
            dns_type: "ALL".to_string(),
            whois_target: "example.com".to_string(),
            keygen_name: "id_ed25519_superterm".to_string(),
            keygen_algo: "Ed25519".to_string(),
            keygen_comment: "superterm@workstation".to_string(),
            keygen_passphrase: String::new(),
            keygen_last_pub: None,
            keygen_status: None,
            existing_keys: list_ssh_keys(),
            revealed_host_ids: HashSet::new(),
            vault_notice: None,
            sql_save_req: None,
            sql_delete_req: None,
            note_selected: None,
            note_title_buf: String::new(),
            note_body_buf: String::new(),
            note_dirty: false,
            note_confirm_delete: None,
            note_save_req: None,
            note_delete_req: None,
            note_create_req: false,
            note_notice: None,
        }
    }

    /// Load a note into the editor buffers, discarding any unsaved edits.
    pub fn load_note(&mut self, note: &NoteEntry) {
        self.note_selected = Some(note.id.clone());
        self.note_title_buf = note.title.clone();
        self.note_body_buf = note.body.clone();
        self.note_dirty = false;
    }

    /// Clear the editor when no note is selected.
    pub fn clear_note_editor(&mut self) {
        self.note_selected = None;
        self.note_title_buf.clear();
        self.note_body_buf.clear();
        self.note_dirty = false;
    }

    pub fn process_events(&mut self) {
        while let Ok(ev) = self.rx_events.try_recv() {
            match ev {
                ToolEvent::Clear { tool } => {
                    self.outputs.insert(tool.title(), Vec::new());
                    self.running.insert(tool.title());
                }
                ToolEvent::Line { tool, line } => {
                    let entry = self.outputs.entry(tool.title()).or_default();
                    entry.push(line);
                }
                ToolEvent::Finished { tool } => {
                    self.running.remove(tool.title());
                }
            }
        }
    }
}

pub fn draw_nav_icon(ui: &mut egui::Ui, is_tools: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0_f32), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let stroke = Stroke::new(1.6_f32, color);

    if !is_tools {
        // Server Stack Icon (Hosts)
        let r1 = egui::Rect::from_center_size(Pos2::new(c.x, c.y - 4.5), Vec2::new(15.0, 6.0));
        let r2 = egui::Rect::from_center_size(Pos2::new(c.x, c.y + 4.5), Vec2::new(15.0, 6.0));
        p.rect_stroke(r1, 1.5_f32, stroke);
        p.rect_stroke(r2, 1.5_f32, stroke);
        p.circle_filled(Pos2::new(c.x - 4.0, c.y - 4.5), 1.2_f32, color);
        p.circle_filled(Pos2::new(c.x - 4.0, c.y + 4.5), 1.2_f32, color);
    } else {
        // Wrench / Tool Icon
        p.circle_stroke(Pos2::new(c.x - 3.5, c.y - 3.5), 4.0_f32, stroke);
        p.line_segment(
            [Pos2::new(c.x - 0.5, c.y - 0.5), Pos2::new(c.x + 6.5, c.y + 6.5)],
            Stroke::new(2.2_f32, color),
        );
    }
}

/// Console window icon for the local shell entries.
///
/// PowerShell/Terminal gets a `>_` prompt; CMD gets a plain `C:\` style box.
pub fn draw_shell_icon(ui: &mut egui::Ui, kind: ShellKind, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0_f32), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let stroke = Stroke::new(1.4_f32, color);

    let box_rect = egui::Rect::from_center_size(c, Vec2::new(17.0, 14.0));
    p.rect_stroke(box_rect, 2.5_f32, stroke);
    // Title bar.
    p.line_segment(
        [
            Pos2::new(box_rect.min.x, box_rect.min.y + 3.5),
            Pos2::new(box_rect.max.x, box_rect.min.y + 3.5),
        ],
        Stroke::new(1.2_f32, color),
    );

    match kind {
        ShellKind::PowerShell => {
            // Chevron ">" plus an underscore caret.
            let bx = box_rect.min.x + 4.0;
            let by = c.y + 1.0;
            p.line_segment([Pos2::new(bx, by - 3.0), Pos2::new(bx + 3.0, by)], stroke);
            p.line_segment([Pos2::new(bx + 3.0, by), Pos2::new(bx, by + 3.0)], stroke);
            p.line_segment(
                [
                    Pos2::new(bx + 5.0, by + 3.0),
                    Pos2::new(bx + 9.5, by + 3.0),
                ],
                stroke,
            );
        }
        ShellKind::Cmd => {
            // Two short command lines.
            let bx = box_rect.min.x + 4.0;
            p.line_segment(
                [Pos2::new(bx, c.y), Pos2::new(bx + 9.0, c.y)],
                Stroke::new(1.2_f32, color),
            );
            p.line_segment(
                [Pos2::new(bx, c.y + 3.5), Pos2::new(bx + 5.5, c.y + 3.5)],
                Stroke::new(1.2_f32, color),
            );
        }
    }
}

pub fn draw_tool_icon(ui: &mut egui::Ui, tool: &ActiveTool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0_f32), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let stroke = Stroke::new(1.5_f32, color);

    match tool {
        ActiveTool::Ping => {
            // Pulse / ECG waveform
            let pts = [
                Pos2::new(c.x - 8.0, c.y),
                Pos2::new(c.x - 4.0, c.y),
                Pos2::new(c.x - 1.5, c.y - 6.0),
                Pos2::new(c.x + 1.5, c.y + 6.0),
                Pos2::new(c.x + 4.0, c.y),
                Pos2::new(c.x + 8.0, c.y),
            ];
            p.add(egui::Shape::line(pts.to_vec(), stroke));
        }
        ActiveTool::Traceroute => {
            // Concentric radar / hop target
            p.circle_stroke(c, 7.5_f32, stroke);
            p.circle_stroke(c, 4.0_f32, stroke);
            p.circle_filled(c, 1.5_f32, color);
        }
        ActiveTool::PortScanner => {
            // Hierarchy / port nodes
            p.rect_stroke(egui::Rect::from_center_size(Pos2::new(c.x, c.y - 5.0), Vec2::new(5.0, 4.0)), 1.0, stroke);
            p.rect_stroke(egui::Rect::from_center_size(Pos2::new(c.x - 5.0, c.y + 5.0), Vec2::new(5.0, 4.0)), 1.0, stroke);
            p.rect_stroke(egui::Rect::from_center_size(Pos2::new(c.x + 5.0, c.y + 5.0), Vec2::new(5.0, 4.0)), 1.0, stroke);
            p.line_segment([Pos2::new(c.x, c.y - 3.0), Pos2::new(c.x, c.y + 1.0)], stroke);
            p.line_segment([Pos2::new(c.x - 5.0, c.y + 1.0), Pos2::new(c.x + 5.0, c.y + 1.0)], stroke);
            p.line_segment([Pos2::new(c.x - 5.0, c.y + 1.0), Pos2::new(c.x - 5.0, c.y + 3.0)], stroke);
            p.line_segment([Pos2::new(c.x + 5.0, c.y + 1.0), Pos2::new(c.x + 5.0, c.y + 3.0)], stroke);
        }
        ActiveTool::DnsLookup => {
            // DNS Asterisk / Star
            p.line_segment([Pos2::new(c.x, c.y - 7.5), Pos2::new(c.x, c.y + 7.5)], stroke);
            p.line_segment([Pos2::new(c.x - 7.5, c.y), Pos2::new(c.x + 7.5, c.y)], stroke);
            p.line_segment([Pos2::new(c.x - 5.5, c.y - 5.5), Pos2::new(c.x + 5.5, c.y + 5.5)], stroke);
            p.line_segment([Pos2::new(c.x + 5.5, c.y - 5.5), Pos2::new(c.x - 5.5, c.y + 5.5)], stroke);
        }
        ActiveTool::DnsWhois => {
            // Globe + Meridian for TLD/ccTLD Whois
            p.circle_stroke(c, 7.5_f32, stroke);
            p.line_segment([Pos2::new(c.x - 7.5, c.y), Pos2::new(c.x + 7.5, c.y)], stroke);
            p.line_segment([Pos2::new(c.x, c.y - 7.5), Pos2::new(c.x, c.y + 7.5)], stroke);
        }
        ActiveTool::NetworkInterfaces => {
            // Signal waves
            p.circle_filled(Pos2::new(c.x, c.y + 5.0), 2.0_f32, color);
            p.line_segment([Pos2::new(c.x - 4.0, c.y + 1.5), Pos2::new(c.x, c.y - 1.5)], stroke);
            p.line_segment([Pos2::new(c.x, c.y - 1.5), Pos2::new(c.x + 4.0, c.y + 1.5)], stroke);
            p.line_segment([Pos2::new(c.x - 7.0, c.y - 2.0), Pos2::new(c.x, c.y - 6.0)], stroke);
            p.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x + 7.0, c.y - 2.0)], stroke);
        }
        ActiveTool::SshKeyGen => {
            // Key icon
            p.circle_stroke(Pos2::new(c.x - 3.5, c.y + 3.0), 3.5_f32, stroke);
            p.line_segment([Pos2::new(c.x - 1.0, c.y + 0.5), Pos2::new(c.x + 6.5, c.y - 6.5)], stroke);
            p.line_segment([Pos2::new(c.x + 4.0, c.y - 4.0), Pos2::new(c.x + 6.5, c.y - 1.5)], stroke);
        }
        ActiveTool::PasswordVault => {
            // Padlock
            let body = egui::Rect::from_center_size(Pos2::new(c.x, c.y + 3.0), Vec2::new(12.0, 9.0));
            p.rect_stroke(body, 1.5_f32, stroke);
            p.circle_stroke(Pos2::new(c.x, c.y - 3.5), 3.5_f32, stroke);
        }
        ActiveTool::NoteVault => {
            // Notebook: a page with ruled lines and a bound spine.
            let page = egui::Rect::from_center_size(
                Pos2::new(c.x + 1.0, c.y),
                Vec2::new(12.0, 15.0),
            );
            p.rect_stroke(page, 1.5_f32, stroke);
            // Spine.
            p.line_segment(
                [
                    Pos2::new(page.min.x - 3.0, page.min.y + 1.0),
                    Pos2::new(page.min.x - 3.0, page.max.y - 1.0),
                ],
                stroke,
            );
            // Ruled lines.
            for i in 0..3 {
                let y = page.min.y + 4.0 + i as f32 * 3.5;
                p.line_segment(
                    [Pos2::new(page.min.x + 3.0, y), Pos2::new(page.max.x - 2.5, y)],
                    Stroke::new(1.1_f32, color),
                );
            }
        }
        ActiveTool::Sql => {
            // Database cylinder: three stacked discs joined by side walls.
            // `Painter` has no ellipse primitive, so each disc is a polyline.
            let rx = 7.0_f32;
            let ry = 2.6_f32;
            let disc = |cy: f32| -> Vec<Pos2> {
                (0..=18)
                    .map(|i| {
                        let t = (i as f32 / 18.0) * std::f32::consts::TAU;
                        Pos2::new(c.x + rx * t.cos(), cy + ry * t.sin())
                    })
                    .collect()
            };

            let top = c.y - 6.0_f32;
            let mid = c.y;
            let bot = c.y + 6.0_f32;

            p.add(egui::Shape::line(disc(top), stroke));
            // Only the front half of the lower discs is drawn, so they read as
            // ridges on a solid body rather than floating rings.
            let front_half = |cy: f32| -> Vec<Pos2> {
                (0..=9)
                    .map(|i| {
                        let t = (i as f32 / 9.0) * std::f32::consts::PI;
                        Pos2::new(c.x + rx * t.cos(), cy + ry * t.sin())
                    })
                    .collect()
            };
            p.add(egui::Shape::line(front_half(mid), stroke));
            p.add(egui::Shape::line(front_half(bot), stroke));

            p.line_segment([Pos2::new(c.x - rx, top), Pos2::new(c.x - rx, bot)], stroke);
            p.line_segment([Pos2::new(c.x + rx, top), Pos2::new(c.x + rx, bot)], stroke);
        }
    }
}

pub fn render_tool_workspace(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rt: &tokio::runtime::Handle,
    tools: &mut ToolsState,
    active_tool: &ActiveTool,
    vault: &AppVault,
    text_primary: Color32,
    text_secondary: Color32,
    card_bg: Color32,
    border_color: Color32,
    btn_neutral_bg: Color32,
    accent_blue: Color32,
    accent_green: Color32,
    accent_purple: Color32,
    back_to_terminal: &mut bool,
) {
    let border_stroke = Stroke::new(1.0_f32, border_color);

    // Top Tool Header
    ui.horizontal(|ui| {
        draw_tool_icon(ui, active_tool, accent_blue);
        ui.add_space(4.0_f32);
        ui.label(
            RichText::new(active_tool.title())
                .size(16.0_f32)
                .strong()
                .color(text_primary),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Back to Terminal")
                            .color(text_primary)
                            .size(12.0_f32),
                    )
                    .fill(btn_neutral_bg),
                )
                .clicked()
            {
                *back_to_terminal = true;
            }
        });
    });

    ui.add_space(6.0_f32);
    ui.separator();
    ui.add_space(8.0_f32);

    let is_running = tools.running.contains(active_tool.title());

    match active_tool {
        ActiveTool::Ping => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Host / IP:").color(text_primary));
                MenuEdit::single("ping_target")
                    .hint("e.g. server.example.com or 8.8.8.8")
                    .width(240.0_f32)
                    .show(ui, &mut tools.ping_target);
                ui.label(RichText::new("Count:").color(text_primary));
                ui.add(egui::DragValue::new(&mut tools.ping_count).range(1..=20));

                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Pinging..." } else { "Run Ping" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                    || (!is_running && enter_pressed && !tools.ping_target.trim().is_empty())
                {
                    spawn_ping(rt, tools.ping_target.clone(), tools.ping_count, tools.tx_events.clone(), ctx.clone());
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::Traceroute => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Target Host / IP:").color(text_primary));
                MenuEdit::single("traceroute_target")
                    .hint("e.g. server.example.com or 1.1.1.1")
                    .width(280.0_f32)
                    .show(ui, &mut tools.traceroute_target);
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Tracing..." } else { "Run Traceroute" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    spawn_traceroute(rt, tools.traceroute_target.clone(), tools.tx_events.clone(), ctx.clone());
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::PortScanner => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Host / IP:").color(text_primary));
                MenuEdit::single("scan_target")
                    .width(180.0_f32)
                    .show(ui, &mut tools.scan_target);
                ui.label(RichText::new("Ports:").color(text_primary));
                MenuEdit::single("scan_ports")
                    .hint("22,80,443 or 1-1024")
                    .width(240.0_f32)
                    .show(ui, &mut tools.scan_ports);
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Scanning..." } else { "Scan Ports" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    spawn_port_scan(rt, tools.scan_target.clone(), tools.scan_ports.clone(), tools.tx_events.clone(), ctx.clone());
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::DnsLookup => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Domain / Host:").color(text_primary));
                MenuEdit::single("dns_target")
                    .width(220.0_f32)
                    .show(ui, &mut tools.dns_target);
                ui.label(RichText::new("Type:").color(text_primary));
                for t in ["ALL", "A", "AAAA", "MX", "NS", "TXT", "CNAME"] {
                    if ui.selectable_label(tools.dns_type == t, t).clicked() {
                        tools.dns_type = t.to_string();
                    }
                }
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Resolving..." } else { "Resolve" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    spawn_dns_lookup(rt, tools.dns_target.clone(), tools.dns_type.clone(), tools.tx_events.clone(), ctx.clone());
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::DnsWhois => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Domain (gTLD / ccTLD):").color(text_primary));
                MenuEdit::single("whois_target")
                    .hint("e.g. example.com, google.co.id, nic.io, bbc.co.uk")
                    .width(250.0_f32)
                    .show(ui, &mut tools.whois_target);
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Querying..." } else { "Whois Lookup" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    spawn_whois(rt, tools.whois_target.clone(), tools.tx_events.clone(), ctx.clone());
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Examples:").size(11.0_f32).color(text_secondary));
                for ex in ["example.com", "pandi.id", "google.co.id", "github.io", "bbc.co.uk", "nic.sg", "amazon.co.jp"] {
                    if ui.small_button(ex).clicked() {
                        tools.whois_target = ex.to_string();
                        spawn_whois(rt, ex.to_string(), tools.tx_events.clone(), ctx.clone());
                    }
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::NetworkInterfaces => {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !is_running,
                        egui::Button::new(RichText::new(if is_running { "Refreshing..." } else { "Refresh Adapters" }).color(Color32::WHITE))
                            .fill(Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    spawn_network_interfaces(rt, tools.tx_events.clone(), ctx.clone());
                }
            });
            render_tool_output_box(ui, tools, active_tool, text_primary, text_secondary, card_bg, border_stroke, btn_neutral_bg);
        }
        ActiveTool::Sql => {
            // SQL Client runs as its own instanced tab, not inside the shared
            // tool workspace, so several databases can be open at once.
            // Clicking the sidebar entry opens one; this branch is unreachable
            // in normal use.
            ui.label(
                RichText::new("Opening a SQL Client session...")
                    .size(12.0_f32)
                    .color(text_secondary),
            );
        }
        ActiveTool::NoteVault => {
            render_note_vault(
                ui,
                tools,
                vault,
                text_primary,
                text_secondary,
                card_bg,
                border_stroke,
                btn_neutral_bg,
                accent_blue,
                accent_green,
                accent_purple,
            );
        }
        ActiveTool::SshKeyGen => {
            egui::Frame::none()
                .fill(card_bg)
                .stroke(border_stroke)
                .rounding(Rounding::same(6.0_f32))
                .inner_margin(egui::Margin::same(12.0_f32))
                .show(ui, |ui| {
                    ui.label(RichText::new(format!("Target SSH Directory: {}", get_ssh_dir().display())).size(11.5_f32).color(accent_blue));
                    ui.add_space(6.0_f32);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Key File Name:").color(text_primary));
                        MenuEdit::single("keygen_name")
                            .width(180.0_f32)
                            .show(ui, &mut tools.keygen_name);
                        ui.label(RichText::new("Algorithm:").color(text_primary));
                        for alg in ["Ed25519", "ECDSA-P256"] {
                            if ui.selectable_label(tools.keygen_algo == alg, alg).clicked() {
                                tools.keygen_algo = alg.to_string();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Comment:").color(text_primary));
                        MenuEdit::single("keygen_comment")
                            .width(180.0_f32)
                            .show(ui, &mut tools.keygen_comment);
                        ui.label(RichText::new("Passphrase (optional):").color(text_primary));
                        MenuEdit::single("keygen_passphrase")
                            .password()
                            .width(150.0_f32)
                            .show(ui, &mut tools.keygen_passphrase);
                    });
                    ui.add_space(6.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("Generate Key Pair").color(Color32::WHITE)).fill(Color32::from_rgb(124, 58, 237)))
                            .clicked()
                        {
                            match generate_ssh_keypair(
                                &tools.keygen_name,
                                &tools.keygen_algo,
                                &tools.keygen_comment,
                                &tools.keygen_passphrase,
                            ) {
                                Ok((priv_p, _pub_p, pub_str)) => {
                                    tools.keygen_status = Some(format!("Generated key pair at {}", priv_p.display()));
                                    tools.keygen_last_pub = Some(pub_str);
                                    tools.existing_keys = list_ssh_keys();
                                }
                                Err(e) => {
                                    tools.keygen_status = Some(format!("Error: {}", e));
                                }
                            }
                        }
                        if ui.button("Refresh Key List").clicked() {
                            tools.existing_keys = list_ssh_keys();
                        }
                    });
                    if let Some(status) = &tools.keygen_status {
                        ui.add_space(4.0_f32);
                        ui.label(RichText::new(status).color(accent_green).size(11.5_f32));
                    }
                    if let Some(pub_key) = &tools.keygen_last_pub {
                        ui.horizontal(|ui| {
                            let mut pk_copy = pub_key.clone();
                            MenuEdit::single("keygen_pubkey")
                                .width(480.0_f32)
                                .show(ui, &mut pk_copy);
                            if ui.button("Copy Public Key").clicked() {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    let _ = cb.set_text(pub_key.clone());
                                }
                            }
                        });
                    }
                });

            ui.add_space(10.0_f32);
            ui.label(RichText::new(format!("Existing Keys in ~/.ssh ({})", tools.existing_keys.len())).color(text_primary).strong());
            ScrollArea::vertical().show(ui, |ui| {
                for (name, pub_content) in &tools.existing_keys {
                    egui::Frame::none()
                        .fill(card_bg)
                        .stroke(border_stroke)
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(8.0_f32))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).strong().color(accent_purple));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Copy .pub").clicked() {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(pub_content.clone());
                                        }
                                    }
                                });
                            });
                            ui.label(RichText::new(pub_content).monospace().size(10.5_f32).color(text_secondary));
                        });
                    ui.add_space(4.0_f32);
                }
            });
        }
        ActiveTool::PasswordVault => {
            egui::Frame::none()
                .fill(card_bg)
                .stroke(border_stroke)
                .rounding(Rounding::same(6.0_f32))
                .inner_margin(egui::Margin::same(10.0_f32))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Vault File:").strong().color(text_primary));
                        ui.label(RichText::new(get_vault_path().display().to_string()).monospace().size(11.5_f32).color(accent_blue));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Copy Vault Path").clicked() {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    let _ = cb.set_text(get_vault_path().display().to_string());
                                    tools.vault_notice = Some("Vault path copied to clipboard.".to_string());
                                }
                            }
                        });
                    });
                    if let Some(msg) = &tools.vault_notice {
                        ui.label(RichText::new(msg).size(11.0_f32).color(accent_green));
                    }
                });

            ui.add_space(8.0_f32);
            ui.label(RichText::new(format!("Stored Host Credentials ({})", vault.profiles.len())).color(text_primary).strong());
            ScrollArea::vertical().show(ui, |ui| {
                for p in &vault.profiles {
                    let is_revealed = tools.revealed_host_ids.contains(&p.id);
                    egui::Frame::none()
                        .fill(card_bg)
                        .stroke(border_stroke)
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(10.0_f32))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&p.name).strong().color(text_primary));
                                ui.label(RichText::new(format!("({}@{}:{})", p.username, p.host, p.port)).size(11.5_f32).color(text_secondary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(pwd) = &p.password {
                                        if !pwd.is_empty() {
                                            if ui.button("Copy Password").clicked() {
                                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                                    let _ = cb.set_text(pwd.clone());
                                                    tools.vault_notice = Some(format!("Copied password for '{}'.", p.name));
                                                }
                                            }
                                            if ui.button(if is_revealed { "Hide" } else { "Reveal" }).clicked() {
                                                if is_revealed {
                                                    tools.revealed_host_ids.remove(&p.id);
                                                } else {
                                                    tools.revealed_host_ids.insert(p.id.clone());
                                                }
                                            }
                                        }
                                    }
                                });
                            });
                            let cred_display = if p.auth_type == "private_key" {
                                format!("Auth: Private Key ({})", p.key_path.as_deref().unwrap_or("unset"))
                            } else if let Some(pwd) = &p.password {
                                if pwd.is_empty() {
                                    "Auth: Password (prompt on connect)".to_string()
                                } else if is_revealed {
                                    format!("Auth: Password -> {}", pwd)
                                } else {
                                    "Auth: Password -> ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€šÃ‚Â¢".to_string()
                                }
                            } else {
                                "Auth: Password (prompt on connect)".to_string()
                            };
                            ui.label(RichText::new(cred_display).monospace().size(11.0_f32).color(accent_purple));
                        });
                    ui.add_space(5.0_f32);
                }
            });
        }
    }
}

/// A text field carrying the usual Cut / Copy / Paste / Select All menu.
///
/// egui gives `TextEdit` keyboard shortcuts but no context menu, so this adds
/// one and drives it through the same selection state the keyboard uses.
///
/// Build with [`MenuEdit::single`] or [`MenuEdit::multi`], then `.show(ui)`.
pub struct MenuEdit<'a> {
    id_salt: &'a str,
    multiline: bool,
    hint: &'a str,
    width: f32,
    rows: usize,
    monospace: bool,
    password: bool,
}

impl<'a> MenuEdit<'a> {
    /// A one-line field.
    pub fn single(id_salt: &'a str) -> Self {
        Self {
            id_salt,
            multiline: false,
            hint: "",
            width: 200.0_f32,
            rows: 1,
            monospace: false,
            password: false,
        }
    }

    /// A multi-line field.
    pub fn multi(id_salt: &'a str, rows: usize) -> Self {
        Self {
            id_salt,
            multiline: true,
            hint: "",
            width: f32::INFINITY,
            rows,
            monospace: false,
            password: false,
        }
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }

    /// Mask the text. Copy and Cut are withheld so a masked secret cannot be
    /// lifted out through the menu.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// Draw the field; returns true when the text changed.
    pub fn show(self, ui: &mut egui::Ui, text: &mut String) -> bool {
        self.show_response(ui, text).1
    }

    /// Draw the field and hand back the `Response` too, for callers that need
    /// focus handling or Enter-to-submit.
    pub fn show_response(
        self,
        ui: &mut egui::Ui,
        text: &mut String,
    ) -> (egui::Response, bool) {
        text_edit_with_menu(
            ui,
            text,
            self.multiline,
            self.id_salt,
            self.hint,
            self.width,
            self.rows,
            self.monospace,
            self.password,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn text_edit_with_menu(
    ui: &mut egui::Ui,
    text: &mut String,
    multiline: bool,
    id_salt: &str,
    hint: &str,
    width: f32,
    rows: usize,
    monospace: bool,
    password: bool,
) -> (egui::Response, bool) {
    let mut builder = if multiline {
        egui::TextEdit::multiline(text)
    } else {
        egui::TextEdit::singleline(text)
    }
    .id_salt(id_salt)
    .hint_text(hint)
    .desired_width(width);

    if multiline {
        builder = builder.desired_rows(rows);
    }
    if monospace {
        builder = builder.font(egui::TextStyle::Monospace);
    }
    if password {
        builder = builder.password(true);
    }

    // Right-clicking destroys the selection before any menu can read it:
    // egui's `pointer_interaction` collapses the range on *any* pointer
    // press, and that includes the secondary button. Nothing can be
    // recovered after the fact, so the last non-empty selection is recorded
    // every frame and used when the menu opens.
    let remembered_id = ui.id().with(("menu_edit_sel", id_salt));

    // `show` is used instead of `ui.add` so the cursor range comes back.
    let out = builder.show(ui);
    let mut changed = out.response.changed();

    let live = out
        .state
        .cursor
        .range(&out.galley)
        .or(out.cursor_range)
        .map(|range| {
            let [min, max] = range.sorted_cursors();
            let a = byte_index_from_char_index(text, min.ccursor.index);
            let b = byte_index_from_char_index(text, max.ccursor.index);
            (a, b)
        });

    // Only a real selection is worth remembering; a bare caret is not.
    let secondary_down = ui.input(|i| i.pointer.secondary_down());
    if let Some((a, b)) = live {
        if a != b && !secondary_down {
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(remembered_id, (a, b)));
        }
    }

    let remembered: Option<(usize, usize)> = ui
        .ctx()
        .memory(|m| m.data.get_temp::<(usize, usize)>(remembered_id));

    // Prefer the live range, but fall back to what was saved before the
    // right-click wiped it. Guard the bounds: the text may have changed
    // since, and stale offsets would panic when sliced.
    let selection: Option<(usize, usize)> = live
        .filter(|(a, b)| a != b)
        .or(remembered)
        .filter(|&(a, b)| a < b && b <= text.len() && text.is_char_boundary(a) && text.is_char_boundary(b));

    let has_selection = selection.is_some();

    // Insertion point for a paste with no selection. Taken from the live
    // caret, not the remembered selection, so it follows the cursor.
    let caret = live.map(|(a, _)| a).unwrap_or(text.len());

    let edit_id = out.response.id;
    let mut new_text: Option<(String, usize)> = None;
    let mut select_all = false;

    // `context_menu` consumes the response, so keep a copy to return.
    let response = out.response.clone();

    out.response.context_menu(|ui| {
        ui.set_min_width(150.0_f32);

        // Reading a masked field through the menu would defeat the mask, so
        // a password field offers only Paste and Select All.
        if !password {
            if ui
                .add_enabled(has_selection, egui::Button::new("Cut"))
                .clicked()
            {
                if let Some((a, b)) = selection {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(text[a..b].to_string());
                    }
                    let mut t = text.clone();
                    t.replace_range(a..b, "");
                    new_text = Some((t, a));
                }
                ui.close_menu();
            }

            if ui
                .add_enabled(has_selection, egui::Button::new("Copy"))
                .clicked()
            {
                if let Some((a, b)) = selection {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(text[a..b].to_string());
                    }
                }
                ui.close_menu();
            }
        }

        if ui.button("Paste").clicked() {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Ok(pasted) = cb.get_text() {
                    // A single-line field must not gain newlines.
                    let pasted = if multiline {
                        pasted
                    } else {
                        pasted.replace(['\n', '\r'], " ")
                    };
                    let mut t = text.clone();
                    match selection {
                        Some((a, b)) if a != b => {
                            t.replace_range(a..b, &pasted);
                            new_text = Some((t, a + pasted.len()));
                        }
                        _ => {
                            // Clamping with `min` alone could land inside a
                            // multi-byte character and panic on insert.
                            let mut at = caret.min(t.len());
                            while at > 0 && !t.is_char_boundary(at) {
                                at -= 1;
                            }
                            t.insert_str(at, &pasted);
                            new_text = Some((t, at + pasted.len()));
                        }
                    }
                }
            }
            ui.close_menu();
        }

        ui.separator();

        if ui
            .add_enabled(!text.is_empty(), egui::Button::new("Select All"))
            .clicked()
        {
            select_all = true;
            ui.close_menu();
        }
    });

    // Apply the edit and put the caret where the user would expect it.
    if let Some((t, caret_byte)) = new_text {
        *text = t;
        changed = true;

        // The remembered selection describes text that no longer exists.
        ui.ctx()
            .memory_mut(|m| m.data.remove::<(usize, usize)>(remembered_id));

        let mut at = caret_byte.min(text.len());
        while at > 0 && !text.is_char_boundary(at) {
            at -= 1;
        }
        let char_idx = text[..at].chars().count();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(char_idx),
            )));
            state.store(ui.ctx(), edit_id);
        }
        ui.ctx().memory_mut(|m| m.request_focus(edit_id));
    }

    if select_all {
        let full = (0usize, text.len());
        ui.ctx()
            .memory_mut(|m| m.data.insert_temp(remembered_id, full));

        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text.chars().count()),
            )));
            state.store(ui.ctx(), edit_id);
        }
        ui.ctx().memory_mut(|m| m.request_focus(edit_id));
    }

    (response, changed)
}

/// Convert a char index into a byte index, which is what `String` slicing needs.
fn byte_index_from_char_index(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// The Note Vault workspace: a list of notes down the left, editor on the right.
///
/// Notes live in `AppVault`, so they are saved with everything else and are
/// encrypted by the same `.stvault` export.
#[allow(clippy::too_many_arguments)]
fn render_note_vault(
    ui: &mut egui::Ui,
    tools: &mut ToolsState,
    vault: &AppVault,
    text_primary: Color32,
    text_secondary: Color32,
    card_bg: Color32,
    border_stroke: Stroke,
    btn_neutral_bg: Color32,
    accent_blue: Color32,
    accent_green: Color32,
    accent_purple: Color32,
) {
    // Selecting a note that no longer exists would leave a stale editor.
    if let Some(sel) = tools.note_selected.clone() {
        if !vault.notes.iter().any(|n| n.id == sel) {
            tools.clear_note_editor();
        }
    }
    // Open the first note on arrival so the pane is never blank for no reason.
    if tools.note_selected.is_none() {
        if let Some(first) = vault.notes.first() {
            let first = first.clone();
            tools.load_note(&first);
        }
    }

    let mut select_req: Option<String> = None;

    ui.horizontal_top(|ui| {
        // ---------------- Left: note list ----------------
        const LIST_W: f32 = 190.0_f32;
        ui.allocate_ui_with_layout(
            Vec2::new(LIST_W, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(LIST_W);

                if ui
                    .add_sized(
                        [LIST_W, 26.0_f32],
                        egui::Button::new(
                            RichText::new("+ New Note")
                                .color(Color32::WHITE)
                                .size(12.0_f32)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(124, 58, 237)),
                    )
                    .clicked()
                {
                    tools.note_create_req = true;
                }

                ui.add_space(6.0_f32);

                ScrollArea::vertical()
                    .id_salt("note_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if vault.notes.is_empty() {
                            ui.label(
                                RichText::new("No notes yet.")
                                    .size(11.0_f32)
                                    .color(text_secondary),
                            );
                        }

                        for note in &vault.notes {
                            let is_sel = tools.note_selected.as_deref() == Some(note.id.as_str());
                            let caption = if note.title.trim().is_empty() {
                                "(untitled)".to_string()
                            } else {
                                note.title.clone()
                            };

                            let res = egui::Frame::none()
                                .fill(if is_sel { card_bg } else { Color32::TRANSPARENT })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if is_sel { accent_purple } else { Color32::TRANSPARENT },
                                ))
                                .rounding(Rounding::same(5.0_f32))
                                .inner_margin(egui::Margin::symmetric(8.0_f32, 5.0_f32))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(caption)
                                                .size(12.0_f32)
                                                .color(if is_sel { text_primary } else { text_secondary })
                                                .strong(),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    );
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);

                            if res.clicked() {
                                select_req = Some(note.id.clone());
                            }
                            res.context_menu(|ui| {
                                ui.set_min_width(140.0_f32);
                                if ui.button("Open").clicked() {
                                    select_req = Some(note.id.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Copy body").clicked() {
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        let _ = cb.set_text(note.body.clone());
                                        tools.note_notice =
                                            Some(format!("Copied '{}' to the clipboard.", note.title));
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui
                                    .button(
                                        RichText::new("Delete note")
                                            .color(Color32::from_rgb(248, 113, 113)),
                                    )
                                    .clicked()
                                {
                                    tools.note_confirm_delete =
                                        Some((note.id.clone(), note.title.clone()));
                                    ui.close_menu();
                                }
                            });

                            ui.add_space(2.0_f32);
                        }
                    });
            },
        );

        ui.add_space(10.0_f32);
        ui.separator();
        ui.add_space(10.0_f32);

        // ---------------- Right: editor ----------------
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            if tools.note_selected.is_none() {
                ui.add_space(20.0_f32);
                ui.label(
                    RichText::new("Select a note on the left, or create a new one.")
                        .size(12.0_f32)
                        .color(text_secondary),
                );
                return;
            }

            ui.horizontal(|ui| {
                ui.label(RichText::new("Title:").size(11.5_f32).color(text_secondary));
                if MenuEdit::single("note_title")
                    .hint("Note title")
                    .width(260.0_f32)
                    .show(ui, &mut tools.note_title_buf)
                {
                    tools.note_dirty = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Delete")
                                    .size(11.5_f32)
                                    .color(Color32::from_rgb(248, 113, 113)),
                            )
                            .fill(btn_neutral_bg),
                        )
                        .clicked()
                    {
                        if let Some(id) = tools.note_selected.clone() {
                            let title = tools.note_title_buf.clone();
                            tools.note_confirm_delete = Some((id, title));
                        }
                    }

                    let can_save = tools.note_dirty;
                    if ui
                        .add_enabled(
                            can_save,
                            egui::Button::new(
                                RichText::new(if can_save { "Save" } else { "Saved" })
                                    .size(11.5_f32)
                                    .color(Color32::WHITE),
                            )
                            .fill(if can_save {
                                Color32::from_rgb(37, 99, 235)
                            } else {
                                btn_neutral_bg
                            }),
                        )
                        .on_hover_text("Write this note to the vault (Ctrl+S)")
                        .clicked()
                    {
                        if let Some(id) = tools.note_selected.clone() {
                            tools.note_save_req = Some(NoteEntry {
                                id,
                                title: tools.note_title_buf.clone(),
                                body: tools.note_body_buf.clone(),
                                updated_at: chrono::Local::now().to_rfc3339(),
                            });
                        }
                    }

                    if tools.note_dirty {
                        ui.label(
                            RichText::new("unsaved")
                                .size(10.5_f32)
                                .color(accent_purple),
                        );
                    }
                });
            });

            ui.add_space(6.0_f32);

            // Ctrl+S is the habit most users already have.
            let ctrl_s = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
            if ctrl_s && tools.note_dirty {
                if let Some(id) = tools.note_selected.clone() {
                    tools.note_save_req = Some(NoteEntry {
                        id,
                        title: tools.note_title_buf.clone(),
                        body: tools.note_body_buf.clone(),
                        updated_at: chrono::Local::now().to_rfc3339(),
                    });
                }
            }

            egui::Frame::none()
                .fill(card_bg)
                .stroke(border_stroke)
                .rounding(Rounding::same(6.0_f32))
                .inner_margin(egui::Margin::same(8.0_f32))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ScrollArea::vertical()
                        .id_salt("note_body_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if MenuEdit::multi("note_body", 18)
                                .hint("Write anything here - credentials, runbooks, snippets.")
                                .monospace()
                                .show(ui, &mut tools.note_body_buf)
                            {
                                tools.note_dirty = true;
                            }
                        });
                });

            ui.add_space(4.0_f32);
            ui.horizontal(|ui| {
                let stamp = tools
                    .note_selected
                    .as_ref()
                    .and_then(|id| vault.notes.iter().find(|n| &n.id == id))
                    .map(|n| n.updated_at.clone())
                    .unwrap_or_default();
                if !stamp.is_empty() {
                    // Seconds are enough; the timezone suffix only adds noise.
                    let shown = stamp.split('.').next().unwrap_or(&stamp).replace('T', " ");
                    ui.label(
                        RichText::new(format!("Last saved {}", shown))
                            .size(10.5_f32)
                            .color(text_secondary),
                    );
                }
                if let Some(msg) = &tools.note_notice {
                    ui.label(RichText::new(msg).size(10.5_f32).color(accent_green));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} note(s) - stored in the vault, included in Sync export",
                            vault.notes.len()
                        ))
                        .size(10.0_f32)
                        .color(text_secondary),
                    );
                });
            });
        });
    });

    // Switching notes discards nothing silently: save first if needed.
    if let Some(id) = select_req {
        if tools.note_dirty {
            if let Some(cur) = tools.note_selected.clone() {
                tools.note_save_req = Some(NoteEntry {
                    id: cur,
                    title: tools.note_title_buf.clone(),
                    body: tools.note_body_buf.clone(),
                    updated_at: chrono::Local::now().to_rfc3339(),
                });
            }
        }
        if let Some(note) = vault.notes.iter().find(|n| n.id == id) {
            let note = note.clone();
            tools.load_note(&note);
        }
    }

    let _ = accent_blue;
}

/// The SQL Client workspace: saved connections, an editor, and a result grid.
#[allow(clippy::too_many_arguments)]
pub fn render_sql_client(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rt: &tokio::runtime::Handle,
    sess: &mut SqlSession,
    tools: &mut ToolsState,
    vault: &AppVault,
    text_primary: Color32,
    text_secondary: Color32,
    card_bg: Color32,
    border_stroke: Stroke,
    btn_neutral_bg: Color32,
    accent_blue: Color32,
    accent_green: Color32,
    accent_purple: Color32,
    // Set when the user asks for another SQL Client tab; the app owns tab
    // creation, so this reports the request rather than acting on it.
    new_session_req: &mut bool,
    // Unique per workspace tab. Widget ids must include it: two sessions
    // sharing an id would fight over focus and scroll state.
    sess_key: &str,
) {
    let label_col = text_secondary;

    // ---------------- Saved connections ----------------
    ui.horizontal(|ui| {
        ui.label(RichText::new("Connection:").strong().color(text_primary));

        let selected_label = if sess.conn_name.trim().is_empty() {
            "<unsaved>".to_string()
        } else {
            sess.conn_name.clone()
        };

        egui::ComboBox::from_id_salt(("sql_saved_conns", sess_key))
            .selected_text(selected_label)
            .width(220.0_f32)
            .show_ui(ui, |ui| {
                if vault.sql_connections.is_empty() {
                    ui.label(RichText::new("(no saved connections)").color(text_secondary));
                }
                for cfg in &vault.sql_connections {
                    let is_sel = sess.selected_id.as_deref() == Some(cfg.id.as_str());
                    let label = format!("{}  [{}]", cfg.name, cfg.db_type);
                    if ui.selectable_label(is_sel, label).clicked() {
                        sess.load_config(cfg);
                    }
                }
            });

        if ui
            .add(egui::Button::new(RichText::new("Clear").size(11.5_f32).color(text_primary)).fill(btn_neutral_bg))
            .on_hover_text("Clear this form for a new connection")
            .clicked()
        {
            sess.reset_editor();
        }

        // Opening a second database should not disturb this one, so this asks
        // the app for another tab rather than reusing the current session.
        if ui
            .add(
                egui::Button::new(
                    RichText::new("New Session").size(11.5_f32).color(text_primary),
                )
                .fill(btn_neutral_bg),
            )
            .on_hover_text("Open another SQL Client tab")
            .clicked()
        {
            *new_session_req = true;
        }

        // Saving requires a name, so the entry can be found again.
        let can_save = !sess.conn_name.trim().is_empty();
        if ui
            .add_enabled(
                can_save,
                egui::Button::new(RichText::new("Save").size(11.5_f32).color(Color32::WHITE))
                    .fill(Color32::from_rgb(124, 58, 237)),
            )
            .on_hover_text("Store this connection in the vault")
            .clicked()
        {
            let id = sess
                .selected_id
                .clone()
                .unwrap_or_else(|| format!("sql-{}", uuid::Uuid::new_v4()));
            sess.selected_id = Some(id.clone());
            // Vault writes are shared, so they are parked on ToolsState.
            tools.sql_save_req = Some(SqlConfig {
                id,
                name: sess.conn_name.trim().to_string(),
                host: sess.host.trim().to_string(),
                port: sess
                    .port
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| sess.kind.default_port()),
                user: sess.user.trim().to_string(),
                pass: sess.pass.clone(),
                database: sess.database.trim().to_string(),
                db_type: sess.kind.title().to_string(),
            });
        }

        if sess.selected_id.is_some() {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Delete")
                            .size(11.5_f32)
                            .color(Color32::from_rgb(248, 113, 113)),
                    )
                    .fill(btn_neutral_bg),
                )
                .clicked()
            {
                tools.sql_delete_req = sess.selected_id.clone();
            }
        }
    });

    ui.add_space(8.0_f32);

    // ---------------- Connection details ----------------
    egui::Frame::none()
        .fill(card_bg)
        .stroke(border_stroke)
        .rounding(Rounding::same(6.0_f32))
        .inner_margin(egui::Margin::same(10.0_f32))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(RichText::new("Name:").size(11.5_f32).color(label_col));
                MenuEdit::single(&format!("sql_name:{}", sess_key))
                    .hint("e.g. India Live DB")
                    .width(170.0_f32)
                    .show(ui, &mut sess.conn_name);

                ui.add_space(8.0_f32);
                ui.label(RichText::new("Type:").size(11.5_f32).color(label_col));
                for kind in DbKind::all() {
                    if ui
                        .selectable_label(sess.kind == kind, kind.title())
                        .clicked()
                    {
                        // Follow the engine's default port unless the user
                        // already moved away from it.
                        let was_default = sess
                            .port
                            .trim()
                            .parse::<u16>()
                            .map(|p| p == sess.kind.default_port())
                            .unwrap_or(true);
                        sess.kind = kind;
                        if was_default {
                            sess.port = kind.default_port().to_string();
                        }
                    }
                }
            });

            ui.add_space(6.0_f32);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Host:").size(11.5_f32).color(label_col));
                MenuEdit::single(&format!("sql_host:{}", sess_key))
                    .width(150.0_f32)
                    .show(ui, &mut sess.host);

                ui.label(RichText::new("Port:").size(11.5_f32).color(label_col));
                MenuEdit::single(&format!("sql_port:{}", sess_key))
                    .width(60.0_f32)
                    .show(ui, &mut sess.port);

                // Active local tunnels make a convenient source of host:port.
                let tunnels: Vec<(String, String, u16)> = vault
                    .profiles
                    .iter()
                    .flat_map(|p| {
                        p.tunnels.iter().filter_map(move |t| {
                            if t.tunnel_type == "local" {
                                Some((
                                    format!("{} - {}", p.name, t.name),
                                    t.bind_host.clone(),
                                    t.bind_port,
                                ))
                            } else {
                                None
                            }
                        })
                    })
                    .collect();

                if !tunnels.is_empty() {
                    ui.add_space(6.0_f32);
                    egui::ComboBox::from_id_salt(("sql_tunnel_pick", sess_key))
                        .selected_text("Use tunnel...")
                        .width(170.0_f32)
                        .show_ui(ui, |ui| {
                            for (label, bind_host, bind_port) in &tunnels {
                                if ui.selectable_label(false, label).clicked() {
                                    // A tunnel listening on 0.0.0.0 is still
                                    // reached through loopback from here.
                                    sess.host = if bind_host == "0.0.0.0" {
                                        "127.0.0.1".to_string()
                                    } else {
                                        bind_host.clone()
                                    };
                                    sess.port = bind_port.to_string();
                                }
                            }
                        });
                }
            });

            ui.add_space(6.0_f32);

            ui.horizontal(|ui| {
                ui.label(RichText::new("User:").size(11.5_f32).color(label_col));
                MenuEdit::single(&format!("sql_user:{}", sess_key))
                    .width(120.0_f32)
                    .show(ui, &mut sess.user);

                ui.label(RichText::new("Password:").size(11.5_f32).color(label_col));
                // Revealing the password also restores Copy and Cut, since the
                // user has already chosen to show it.
                let pass_id = format!("sql_pass:{}", sess_key);
                let mut pass_edit = MenuEdit::single(&pass_id).width(130.0_f32);
                if !sess.reveal_pass {
                    pass_edit = pass_edit.password();
                }
                pass_edit.show(ui, &mut sess.pass);
                ui.checkbox(&mut sess.reveal_pass, RichText::new("Show").size(11.0_f32).color(label_col));

                ui.label(RichText::new("Database:").size(11.5_f32).color(label_col));
                MenuEdit::single(&format!("sql_db:{}", sess_key))
                    .width(130.0_f32)
                    .show(ui, &mut sess.database);
            });
        });

    ui.add_space(8.0_f32);

    // ---------------- Query editor ----------------
    ui.horizontal(|ui| {
        ui.label(RichText::new("SQL:").strong().color(text_primary));
        ui.label(
            RichText::new("Ctrl+Enter to execute")
                .size(10.5_f32)
                .color(text_secondary),
        );
    });
    ui.add_space(3.0_f32);

    MenuEdit::multi(&format!("sql_query:{}", sess_key), 6)
        .monospace()
        .show(ui, &mut sess.query);

    let ctrl_enter = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

    // Connection details, captured once so the closures below stay simple.
    let conn_port: u16 = sess
        .port
        .trim()
        .parse()
        .unwrap_or_else(|_| sess.kind.default_port());
    let conn_kind = sess.kind;
    let conn_host = sess.host.trim().to_string();
    let conn_user = sess.user.trim().to_string();
    let conn_pass = sess.pass.clone();
    let conn_db = sess.database.trim().to_string();

    ui.add_space(6.0_f32);
    ui.horizontal(|ui| {
        let ready = !sess.query.trim().is_empty() && !conn_host.is_empty();

        let run_clicked = ui
            .add_enabled(
                ready,
                egui::Button::new(
                    RichText::new("Execute").color(Color32::WHITE).strong(),
                )
                .fill(Color32::from_rgb(37, 99, 235)),
            )
            .on_hover_text("Run in a new result tab")
            .clicked();

        if (run_clicked || ctrl_enter) && ready {
            // Every run opens its own tab, so earlier results stay put.
            let sql = sess.query.clone();
            let tab_id = sess.begin_tab(&sql);
            spawn_query(
                rt,
                tab_id,
                conn_kind,
                conn_host.clone(),
                conn_port,
                conn_user.clone(),
                conn_pass.clone(),
                conn_db.clone(),
                sql,
                sess.tx.clone(),
                ctx.clone(),
            );
        }

        if !sess.tabs.is_empty()
            && ui
                .add(
                    egui::Button::new(
                        RichText::new("Close All Tabs").size(11.5_f32).color(text_primary),
                    )
                    .fill(btn_neutral_bg),
                )
                .clicked()
        {
            sess.tabs.clear();
            sess.active_tab = None;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} result tab(s)", sess.tabs.len()))
                    .size(10.5_f32)
                    .color(text_secondary),
            );
        });
    });

    ui.add_space(8.0_f32);

    // ---------------- Result tabs ----------------
    if sess.tabs.is_empty() {
        egui::Frame::none()
            .fill(card_bg)
            .stroke(border_stroke)
            .rounding(Rounding::same(6.0_f32))
            .inner_margin(egui::Margin::same(10.0_f32))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(
                        "No results yet. Execute a statement to open its own result tab.",
                    )
                    .size(11.5_f32)
                    .color(text_secondary),
                );
            });
        return;
    }

    // Actions are collected while drawing and applied afterwards, because the
    // tab list cannot be mutated while it is being iterated.
    let mut close_req: Option<u64> = None;
    let mut select_req: Option<u64> = None;
    let mut edit_req: Option<u64> = None;
    let mut rerun_req: Option<u64> = None;

    ScrollArea::horizontal()
        .id_salt(("sql_tabstrip", sess_key))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0_f32;

                for tab in &sess.tabs {
                    let is_active = sess.active_tab == Some(tab.id);
                    let fill = if is_active { card_bg } else { btn_neutral_bg };
                    let label_col = if tab.error {
                        Color32::from_rgb(248, 113, 113)
                    } else if is_active {
                        accent_blue
                    } else {
                        text_secondary
                    };

                    let caption = if tab.running {
                        format!("{} ...", tab.title)
                    } else {
                        tab.title.clone()
                    };

                    let resp = ui
                        .add(
                            egui::Button::new(
                                RichText::new(caption).size(11.5_f32).color(label_col),
                            )
                            .fill(fill)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if is_active { accent_blue } else { Color32::TRANSPARENT },
                            )),
                        )
                        .on_hover_text(tab.sql.as_str());

                    if resp.clicked() {
                        select_req = Some(tab.id);
                    }

                    resp.context_menu(|ui| {
                        ui.set_min_width(160.0_f32);
                        if ui.button("Copy query").clicked() {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(tab.sql.clone());
                            }
                            ui.close_menu();
                        }
                        if ui.button("Edit query...").clicked() {
                            edit_req = Some(tab.id);
                            ui.close_menu();
                        }
                        if ui.button("Re-run").clicked() {
                            rerun_req = Some(tab.id);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(
                                RichText::new("Close tab")
                                    .color(Color32::from_rgb(248, 113, 113)),
                            )
                            .clicked()
                        {
                            close_req = Some(tab.id);
                            ui.close_menu();
                        }
                    });

                    // Small 'x' beside each caption for one-click closing.
                    if ui
                        .add(
                            egui::Button::new(RichText::new("x").size(10.0_f32).color(text_secondary))
                                .fill(fill)
                                .small(),
                        )
                        .on_hover_text("Close this result tab")
                        .clicked()
                    {
                        close_req = Some(tab.id);
                    }

                    ui.add_space(6.0_f32);
                }
            });
        });

    ui.add_space(4.0_f32);

    // ---------------- Active tab body ----------------
    let active_id = sess
        .active_tab
        .filter(|id| sess.tabs.iter().any(|t| t.id == *id))
        .or_else(|| sess.tabs.last().map(|t| t.id));

    if let Some(active_id) = active_id {
        sess.active_tab = Some(active_id);

        // Copy out what the body needs; `sess` is borrowed again below.
        let mut copy_tsv_req = false;
        if let Some(tab) = sess.tabs.iter().find(|t| t.id == active_id) {
            egui::Frame::none()
                .fill(card_bg)
                .stroke(border_stroke)
                .rounding(Rounding::same(6.0_f32))
                .inner_margin(egui::Margin::same(8.0_f32))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // Status line for this tab.
                    ui.horizontal(|ui| {
                        if let Some(status) = &tab.status {
                            let col = if tab.error {
                                Color32::from_rgb(248, 113, 113)
                            } else {
                                accent_green
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(status).size(11.5_f32).color(col),
                                )
                                .selectable(true),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !tab.rows.is_empty()
                                && ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Copy as TSV")
                                                .size(11.0_f32)
                                                .color(text_primary),
                                        )
                                        .fill(btn_neutral_bg),
                                    )
                                    .clicked()
                            {
                                copy_tsv_req = true;
                            }
                        });
                    });

                    ui.add_space(4.0_f32);
                    ui.separator();
                    ui.add_space(4.0_f32);

                    if tab.columns.is_empty() {
                        ui.label(
                            RichText::new(if tab.error {
                                "The statement failed - see the message above."
                            } else if tab.running {
                                "Running..."
                            } else {
                                "No result set. SELECT returns a grid; INSERT/UPDATE/DELETE report affected rows."
                            })
                            .size(11.5_f32)
                            .color(text_secondary),
                        );
                        return;
                    }

                    if tab.truncated {
                        ui.label(
                            RichText::new(format!(
                                "Showing the first {} rows; narrow the query to see the rest.",
                                tab.rows.len()
                            ))
                            .size(10.5_f32)
                            .color(accent_purple),
                        );
                        ui.add_space(4.0_f32);
                    }

                    // Both axes scroll: wide result sets are the norm.
                    ScrollArea::both()
                        .id_salt(("sql_grid_scroll", tab.id))
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            egui::Grid::new(("sql_result_grid", tab.id))
                                .striped(true)
                                .spacing(Vec2::new(14.0_f32, 4.0_f32))
                                .show(ui, |ui| {
                                    for col in &tab.columns {
                                        ui.label(
                                            RichText::new(col)
                                                .strong()
                                                .monospace()
                                                .size(11.5_f32)
                                                .color(accent_blue),
                                        );
                                    }
                                    ui.end_row();

                                    for row in &tab.rows {
                                        for cell in row {
                                            // Long values would otherwise
                                            // stretch the grid past the window.
                                            let shown: String = if cell.chars().count() > 80 {
                                                format!(
                                                    "{}...",
                                                    cell.chars().take(80).collect::<String>()
                                                )
                                            } else {
                                                cell.clone()
                                            };
                                            let col = if cell == "NULL" {
                                                text_secondary
                                            } else {
                                                text_primary
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(shown)
                                                        .monospace()
                                                        .size(11.5_f32)
                                                        .color(col),
                                                )
                                                .selectable(true),
                                            );
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                });
        }

        if copy_tsv_req {
            if let Some(tab) = sess.tabs.iter().find(|t| t.id == active_id) {
                let mut out = tab.columns.join("\t");
                for row in &tab.rows {
                    out.push('\n');
                    out.push_str(&row.join("\t"));
                }
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(out);
                }
            }
        }
    }

    // ---------------- Apply deferred tab actions ----------------
    if let Some(id) = select_req {
        sess.active_tab = Some(id);
    }
    if let Some(id) = close_req {
        sess.tabs.retain(|t| t.id != id);
        if sess.active_tab == Some(id) {
            sess.active_tab = sess.tabs.last().map(|t| t.id);
        }
        if sess.editing_tab == Some(id) {
            sess.editing_tab = None;
        }
    }
    if let Some(id) = edit_req {
        if let Some(tab) = sess.tabs.iter().find(|t| t.id == id) {
            sess.edit_buffer = tab.sql.clone();
            sess.editing_tab = Some(id);
        }
    }
    if let Some(id) = rerun_req {
        if let Some(sql) = sess
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.sql.clone())
        {
            sess.restart_tab(id, &sql);
            spawn_query(
                rt,
                id,
                conn_kind,
                conn_host.clone(),
                conn_port,
                conn_user.clone(),
                conn_pass.clone(),
                conn_db.clone(),
                sql,
                sess.tx.clone(),
                ctx.clone(),
            );
        }
    }

    // ---------------- Edit-query dialog ----------------
    if let Some(edit_id) = sess.editing_tab {
        let mut keep_open = true;
        let mut run_edited = false;

        egui::Window::new("Edit query")
            // Fixed id: the title never varies, but this guards against the
            // window-jump bug if a tab name is ever added to it.
            .id(egui::Id::new(("sql_edit_query_window", sess_key)))
            .collapsible(false)
            .resizable(false)
            .fixed_size(Vec2::new(560.0_f32, 320.0_f32))
            .show(ctx, |ui| {
                ui.set_width(540.0_f32);
                ui.label(
                    RichText::new("The tab will refresh with this statement.")
                        .size(11.0_f32)
                        .color(text_secondary),
                );
                ui.add_space(6.0_f32);
                MenuEdit::multi(&format!("sql_edit_buffer:{}", sess_key), 10)
                    .monospace()
                    .show(ui, &mut sess.edit_buffer);
                ui.add_space(8.0_f32);
                ui.horizontal(|ui| {
                    let can_run = !sess.edit_buffer.trim().is_empty();
                    if ui
                        .add_enabled(
                            can_run,
                            egui::Button::new(RichText::new("OK").color(Color32::WHITE).strong())
                                .fill(Color32::from_rgb(37, 99, 235)),
                        )
                        .clicked()
                    {
                        run_edited = true;
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });

        if run_edited {
            let sql = sess.edit_buffer.clone();
            sess.restart_tab(edit_id, &sql);
            spawn_query(
                rt,
                edit_id,
                conn_kind,
                conn_host,
                conn_port,
                conn_user,
                conn_pass,
                conn_db,
                sql,
                sess.tx.clone(),
                ctx.clone(),
            );
        }
        if !keep_open {
            sess.editing_tab = None;
        }
    }
}

fn render_tool_output_box(
    ui: &mut egui::Ui,
    tools: &mut ToolsState,
    active_tool: &ActiveTool,
    text_primary: Color32,
    text_secondary: Color32,
    card_bg: Color32,
    border_stroke: Stroke,
    btn_neutral_bg: Color32,
) {
    ui.add_space(8.0_f32);
    let lines = tools.outputs.get(active_tool.title()).cloned().unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Output:").strong().color(text_primary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new("Clear").size(11.0_f32).color(text_primary)).fill(btn_neutral_bg))
                .clicked()
            {
                tools.outputs.insert(active_tool.title(), Vec::new());
            }
            if ui
                .add(egui::Button::new(RichText::new("Copy Output").size(11.0_f32).color(text_primary)).fill(btn_neutral_bg))
                .clicked()
            {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(lines.join("\n"));
                }
            }
        });
    });

    ui.add_space(4.0_f32);
    egui::Frame::none()
        .fill(card_bg)
        .stroke(border_stroke)
        .rounding(Rounding::same(6.0_f32))
        .inner_margin(egui::Margin::same(10.0_f32))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if lines.is_empty() {
                        ui.label(
                            RichText::new("Click the action button above to run this tool.")
                                .monospace()
                                .size(12.0_f32)
                                .color(text_secondary),
                        );
                    } else {
                        for l in &lines {
                            ui.add(egui::Label::new(RichText::new(l).monospace().size(12.0_f32).color(text_primary)).selectable(true));
                        }
                    }
                });
        });
}
