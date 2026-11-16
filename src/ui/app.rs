use crate::core::localshell::{
    is_elevated, relaunch_elevated, spawn_local_shell, ActiveShellSession, ShellEvent, ShellKind,
};
use crate::core::ssh::{spawn_live_ssh_connection, ActiveSshSession, SshEvent};
use crate::core::tools::{spawn_network_interfaces, ActiveTool};
use crate::core::tunnel::{spawn_tunnel, TunnelRuntimeHandle};
use crate::core::vault::{
    export_encrypted_stvault, get_vault_path, import_encrypted_stvault, load_vault, save_vault,
    AppVault, HostProfile, NoteEntry, SqlConfig, TunnelConfig,
};
use eframe::glow;
use crate::ui::terminal::{render_terminal_viewport, TerminalEmulator};
use crate::ui::tools_view::{
    draw_nav_icon, draw_shell_icon, draw_tool_icon, render_sql_client, render_tool_workspace,
    MenuEdit, SqlSession, ToolsState,
};
use eframe::egui::{self, Color32, RichText, Rounding, ScrollArea, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Hosts,
    Tools,
}

/// Tabs inside the Add/Edit session dialog.
///
/// Tunnels live in their own drawer, so they are not duplicated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEditorTab {
    Connection,
    Advanced,
}

impl HostEditorTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Connection => "Connection",
            Self::Advanced => "Advanced",
        }
    }

    pub fn all() -> [Self; 2] {
        [Self::Connection, Self::Advanced]
    }
}

/// Session types offered by the Connection tab.
const SESSION_TYPES: [(&str, &str); 6] = [
    ("ssh", "SSH"),
    ("local_shell", "Local shell"),
    ("telnet", "Telnet"),
    ("serial", "Serial (COM)"),
    ("rdp", "RDP (remote desktop)"),
    ("vnc", "VNC"),
];

fn session_type_label(id: &str) -> &'static str {
    SESSION_TYPES
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, label)| *label)
        .unwrap_or("SSH")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabKind {
    Ssh {
        session_key: String,
        host_id: String,
    },
    /// A local PowerShell/Terminal or CMD session on a PTY.
    Shell {
        session_key: String,
        kind: ShellKind,
    },
    /// One SQL Client session. Unlike the other tools this is instanced, so
    /// several databases can be open side by side.
    Sql {
        session_key: String,
    },
    Tool(ActiveTool),
}

#[derive(Debug, Clone)]
pub struct OpenTab {
    pub id: String,
    pub kind: TabKind,
    pub title: String,
}

pub fn configure_superterm_theme(ctx: &egui::Context, is_dark: bool) {
    if is_dark {
        let mut visuals = egui::Visuals::dark();
        visuals.dark_mode = true;
        visuals.override_text_color = Some(Color32::from_rgb(230, 237, 243));
        visuals.panel_fill = Color32::from_rgb(22, 27, 34);
        visuals.window_fill = Color32::from_rgb(22, 27, 34);
        visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(48, 54, 61));
        visuals.extreme_bg_color = Color32::from_rgb(13, 17, 23);
        visuals.faint_bg_color = Color32::from_rgb(28, 33, 40);

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 27, 34);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(48, 54, 61));
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(230, 237, 243));

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 38, 45);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(48, 54, 61));
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(230, 237, 243));
        visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 54, 61);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(139, 92, 246));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

        visuals.widgets.active.bg_fill = Color32::from_rgb(124, 58, 237);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(167, 139, 250));
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        visuals.widgets.active.rounding = Rounding::same(6.0_f32);

        visuals.selection.bg_fill = Color32::from_rgb(109, 40, 217);
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(196, 181, 253));
        ctx.set_visuals(visuals);
    } else {
        let mut visuals = egui::Visuals::light();
        visuals.dark_mode = false;
        visuals.override_text_color = Some(Color32::from_rgb(15, 23, 42));
        visuals.panel_fill = Color32::from_rgb(241, 245, 249);
        visuals.window_fill = Color32::from_rgb(255, 255, 255);
        visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225));
        visuals.extreme_bg_color = Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = Color32::from_rgb(248, 250, 252);

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(241, 245, 249);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225));
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(15, 23, 42));

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(226, 232, 240);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225));
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(15, 23, 42));
        visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(203, 213, 225);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(124, 58, 237));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(15, 23, 42));
        visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

        visuals.widgets.active.bg_fill = Color32::from_rgb(124, 58, 237);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(109, 40, 217));
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        visuals.widgets.active.rounding = Rounding::same(6.0_f32);

        visuals.selection.bg_fill = Color32::from_rgb(221, 214, 254);
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(124, 58, 237));
        ctx.set_visuals(visuals);
    }

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0_f32, 6.0_f32);
    style.spacing.button_padding = Vec2::new(10.0_f32, 5.0_f32);
    style.interaction.selectable_labels = false;
    ctx.set_style(style);
}

fn draw_status_dot(ui: &mut egui::Ui, color: Color32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(radius * 2.0_f32 + 2.0_f32), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

fn draw_sidebar_toggle_button(
    ui: &mut egui::Ui,
    is_open: bool,
    icon_color: Color32,
    pill_bg: Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0_f32), egui::Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if is_open {
            "Collapse left pane"
        } else {
            "Expand left pane"
        });

    let p = ui.painter();
    let center = rect.center();

    // Circular subtle pill background (brighter on hover)
    let circle_col = if response.hovered() {
        Color32::from_rgba_unmultiplied(pill_bg.r(), pill_bg.g(), pill_bg.b(), 230)
    } else {
        Color32::from_rgba_unmultiplied(pill_bg.r(), pill_bg.g(), pill_bg.b(), 140)
    };
    p.circle_filled(center, 13.5_f32, circle_col);

    // Outer rounded sidebar-window icon (15x13)
    let box_rect = egui::Rect::from_center_size(center, Vec2::new(15.0_f32, 12.5_f32));
    let stroke = Stroke::new(1.5_f32, icon_color);
    let div_x = box_rect.min.x + 5.0_f32;

    if is_open {
        let left_strip = egui::Rect::from_min_max(
            box_rect.min,
            egui::Pos2::new(div_x, box_rect.max.y),
        );
        p.rect_filled(
            left_strip,
            Rounding {
                nw: 3.0_f32,
                sw: 3.0_f32,
                ne: 0.0_f32,
                se: 0.0_f32,
            },
            Color32::from_rgba_unmultiplied(icon_color.r(), icon_color.g(), icon_color.b(), 65),
        );
    }

    p.rect_stroke(box_rect, Rounding::same(3.2_f32), stroke);
    p.line_segment(
        [
            egui::Pos2::new(div_x, box_rect.min.y),
            egui::Pos2::new(div_x, box_rect.max.y),
        ],
        stroke,
    );

    response
}

/// Which glyph a context-menu row shows on its left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuIcon {
    Connect,
    NewTab,
    Disconnect,
    Edit,
    Duplicate,
    Rename,
    Export,
    Delete,
}

/// Draw a small 14x14 line icon centred in `rect`.
fn paint_menu_icon(painter: &egui::Painter, rect: egui::Rect, icon: MenuIcon, color: Color32) {
    let s = Stroke::new(1.4_f32, color);
    let c = rect.center();
    let r = egui::Rect::from_center_size(c, Vec2::splat(12.0_f32));

    match icon {
        // Play triangle: start a connection.
        MenuIcon::Connect => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::Pos2::new(r.min.x + 1.5_f32, r.min.y),
                    egui::Pos2::new(r.max.x - 0.5_f32, c.y),
                    egui::Pos2::new(r.min.x + 1.5_f32, r.max.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        // Small window plus a "+": open in a new tab.
        MenuIcon::NewTab => {
            let box_rect = egui::Rect::from_min_max(
                egui::Pos2::new(r.min.x, r.min.y + 1.5_f32),
                egui::Pos2::new(r.max.x - 3.0_f32, r.max.y),
            );
            painter.rect_stroke(box_rect, Rounding::same(2.0_f32), s);
            painter.line_segment(
                [
                    egui::Pos2::new(box_rect.min.x, box_rect.min.y + 3.0_f32),
                    egui::Pos2::new(box_rect.max.x, box_rect.min.y + 3.0_f32),
                ],
                s,
            );
            let px = r.max.x - 1.0_f32;
            let py = r.min.y + 1.0_f32;
            painter.line_segment(
                [egui::Pos2::new(px - 2.5_f32, py), egui::Pos2::new(px + 0.5_f32, py)],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(px - 1.0_f32, py - 1.5_f32),
                    egui::Pos2::new(px - 1.0_f32, py + 1.5_f32),
                ],
                s,
            );
        }
        // Broken link: drop the connection.
        MenuIcon::Disconnect => {
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x, r.max.y - 1.0_f32),
                    egui::Pos2::new(c.x - 1.5_f32, c.y + 0.5_f32),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(c.x + 1.5_f32, c.y - 0.5_f32),
                    egui::Pos2::new(r.max.x, r.min.y + 1.0_f32),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(c.x - 3.0_f32, r.min.y + 1.0_f32),
                    egui::Pos2::new(c.x + 3.0_f32, r.max.y - 1.0_f32),
                ],
                Stroke::new(1.2_f32, color),
            );
        }
        // Pencil: edit.
        MenuIcon::Edit => {
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x + 1.0_f32, r.max.y - 1.0_f32),
                    egui::Pos2::new(r.max.x - 2.0_f32, r.min.y + 2.0_f32),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(r.max.x - 3.5_f32, r.min.y + 0.5_f32),
                    egui::Pos2::new(r.max.x - 0.5_f32, r.min.y + 3.5_f32),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x + 0.5_f32, r.max.y - 0.5_f32),
                    egui::Pos2::new(r.min.x + 3.0_f32, r.max.y - 1.5_f32),
                ],
                s,
            );
        }
        // Two overlapping sheets: duplicate.
        MenuIcon::Duplicate => {
            let back = egui::Rect::from_min_max(
                egui::Pos2::new(r.min.x, r.min.y),
                egui::Pos2::new(r.max.x - 3.5_f32, r.max.y - 3.5_f32),
            );
            let front = egui::Rect::from_min_max(
                egui::Pos2::new(r.min.x + 3.5_f32, r.min.y + 3.5_f32),
                egui::Pos2::new(r.max.x, r.max.y),
            );
            painter.rect_stroke(back, Rounding::same(1.5_f32), s);
            painter.rect_stroke(front, Rounding::same(1.5_f32), s);
        }
        // Tag with a dot: rename.
        MenuIcon::Rename => {
            painter.add(egui::Shape::closed_line(
                vec![
                    egui::Pos2::new(r.min.x, r.min.y + 1.0_f32),
                    egui::Pos2::new(c.x + 1.0_f32, r.min.y + 1.0_f32),
                    egui::Pos2::new(r.max.x, c.y),
                    egui::Pos2::new(c.x + 1.0_f32, r.max.y - 1.0_f32),
                    egui::Pos2::new(r.min.x, r.max.y - 1.0_f32),
                ],
                s,
            ));
            painter.circle_filled(
                egui::Pos2::new(r.min.x + 3.0_f32, c.y),
                1.3_f32,
                color,
            );
        }
        // Box with an up arrow: export.
        MenuIcon::Export => {
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x, c.y + 1.0_f32),
                    egui::Pos2::new(r.min.x, r.max.y),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x, r.max.y),
                    egui::Pos2::new(r.max.x, r.max.y),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(r.max.x, c.y + 1.0_f32),
                    egui::Pos2::new(r.max.x, r.max.y),
                ],
                s,
            );
            painter.line_segment([egui::Pos2::new(c.x, r.min.y), egui::Pos2::new(c.x, c.y + 2.0_f32)], s);
            painter.line_segment(
                [
                    egui::Pos2::new(c.x - 2.5_f32, r.min.y + 2.5_f32),
                    egui::Pos2::new(c.x, r.min.y),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(c.x + 2.5_f32, r.min.y + 2.5_f32),
                    egui::Pos2::new(c.x, r.min.y),
                ],
                s,
            );
        }
        // Trash can: delete.
        MenuIcon::Delete => {
            let lid_y = r.min.y + 2.5_f32;
            painter.line_segment(
                [
                    egui::Pos2::new(r.min.x, lid_y),
                    egui::Pos2::new(r.max.x, lid_y),
                ],
                s,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(c.x - 2.0_f32, r.min.y + 0.5_f32),
                    egui::Pos2::new(c.x + 2.0_f32, r.min.y + 0.5_f32),
                ],
                s,
            );
            let body = egui::Rect::from_min_max(
                egui::Pos2::new(r.min.x + 1.5_f32, lid_y),
                egui::Pos2::new(r.max.x - 1.5_f32, r.max.y),
            );
            painter.line_segment([body.left_top(), body.left_bottom()], s);
            painter.line_segment([body.right_top(), body.right_bottom()], s);
            painter.line_segment([body.left_bottom(), body.right_bottom()], s);
            painter.line_segment(
                [
                    egui::Pos2::new(c.x, lid_y + 2.0_f32),
                    egui::Pos2::new(c.x, r.max.y - 1.5_f32),
                ],
                Stroke::new(1.1_f32, color),
            );
        }
    }
}

/// Horizontal space a menu row needs besides its label text.
const MENU_ROW_CHROME: f32 = 6.0_f32 + 14.0_f32 + 9.0_f32 + 12.0_f32;

/// Measure the width the context menu needs for its longest label.
///
/// `Area` grants effectively unbounded width, so the row width has to be
/// computed from the text rather than read from the layout.
fn menu_width_for(ui: &egui::Ui, labels: &[&str]) -> f32 {
    let font = egui::FontId::proportional(13.0_f32);
    let widest = labels
        .iter()
        .map(|t| {
            ui.fonts(|f| {
                f.layout_no_wrap((*t).to_string(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            })
        })
        .fold(0.0_f32, f32::max);
    (widest + MENU_ROW_CHROME).ceil()
}

/// One row of the host context menu: icon on the left, label beside it.
///
/// The whole row is a single click target so the icon is never a dead zone.
fn menu_item(
    ui: &mut egui::Ui,
    width: f32,
    icon: MenuIcon,
    label: &str,
    color: Color32,
    hover_bg: Color32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, 24.0_f32), egui::Sense::click());

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, Rounding::same(4.0_f32), hover_bg);
    }

    let icon_rect = egui::Rect::from_min_size(
        egui::Pos2::new(rect.min.x + 6.0_f32, rect.center().y - 7.0_f32),
        Vec2::splat(14.0_f32),
    );
    paint_menu_icon(ui.painter(), icon_rect, icon, color);

    ui.painter().text(
        egui::Pos2::new(icon_rect.max.x + 9.0_f32, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0_f32),
        color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Paint the three-dot glyph and the seam divider into an exact rect.
///
/// The caller owns the rect and the interaction, so the button height is
/// guaranteed identical to the host frame beside it.
fn paint_kebab(
    painter: &egui::Painter,
    rect: egui::Rect,
    fill: Color32,
    stroke: Stroke,
    dot_color: Color32,
    hovered: bool,
    hover_fill: Color32,
) {
    // Flat on the left so it butts seamlessly against the host frame.
    let rounding = Rounding {
        nw: 0.0_f32,
        sw: 0.0_f32,
        ne: 5.0_f32,
        se: 5.0_f32,
    };

    painter.rect_filled(rect, rounding, if hovered { hover_fill } else { fill });
    painter.rect_stroke(rect, rounding, stroke);

    // Hide the doubled border at the seam, then draw a short inset divider so
    // the split reads as deliberate rather than as two separate boxes.
    painter.line_segment(
        [
            egui::Pos2::new(rect.min.x, rect.min.y + stroke.width),
            egui::Pos2::new(rect.min.x, rect.max.y - stroke.width),
        ],
        Stroke::new(stroke.width + 1.2_f32, if hovered { hover_fill } else { fill }),
    );
    painter.line_segment(
        [
            egui::Pos2::new(rect.min.x, rect.min.y + 6.0_f32),
            egui::Pos2::new(rect.min.x, rect.max.y - 6.0_f32),
        ],
        Stroke::new(1.0_f32, stroke.color),
    );

    let dots = if hovered {
        dot_color
    } else {
        Color32::from_rgba_unmultiplied(dot_color.r(), dot_color.g(), dot_color.b(), 200)
    };
    let c = rect.center();
    for dx in [-4.2_f32, 0.0_f32, 4.2_f32] {
        painter.circle_filled(egui::Pos2::new(c.x + dx, c.y), 1.6_f32, dots);
    }
}

/// Draw a sleek 1-stack server rack unit icon (like fa-server, 1 single unit)
fn draw_single_server_icon(ui: &mut egui::Ui, frame_color: Color32, led_color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0_f32, 12.0_f32), egui::Sense::hover());
    let p = ui.painter();
    let box_rect = egui::Rect::from_center_size(rect.center(), Vec2::new(14.5_f32, 8.0_f32));
    p.rect_stroke(box_rect, Rounding::same(2.0_f32), Stroke::new(1.3_f32, frame_color));
    // Status LED on left
    p.circle_filled(egui::Pos2::new(box_rect.min.x + 3.5_f32, box_rect.center().y), 1.4_f32, led_color);
    // Two tiny vent lines on right
    let y_mid = box_rect.center().y;
    p.line_segment(
        [
            egui::Pos2::new(box_rect.max.x - 6.0_f32, y_mid),
            egui::Pos2::new(box_rect.max.x - 2.5_f32, y_mid),
        ],
        Stroke::new(1.2_f32, frame_color),
    );
}

pub struct SupertermNativeApp {
    pub rt: tokio::runtime::Handle,
    pub tx_events: mpsc::UnboundedSender<SshEvent>,
    pub rx_events: mpsc::UnboundedReceiver<SshEvent>,

    pub vault: AppVault,
    pub selected_host_id: String,
    pub search_query: String,

    // Live SSH State (keyed by session_key = tab.id)
    pub active_sessions: HashMap<String, Arc<ActiveSshSession>>,
    pub connecting_sessions: HashSet<String>,
    pub host_tunnel_sessions: HashMap<String, Arc<ActiveSshSession>>, // keyed by host_id!
    pub active_tunnels: HashMap<String, TunnelRuntimeHandle>,
    pub terminals: HashMap<String, TerminalEmulator>,
    pub next_tab_seq: usize,

    // Local shell sessions (PowerShell / Terminal / CMD), keyed by session_key
    pub tx_shell_events: mpsc::UnboundedSender<ShellEvent>,
    pub rx_shell_events: mpsc::UnboundedReceiver<ShellEvent>,
    pub shell_sessions: HashMap<String, Arc<ActiveShellSession>>,
    /// Shell awaiting the "run elevated?" answer, and whether the request was
    /// an explicit "new session" (true) rather than a plain click (false).
    pub pending_shell_elevation: Option<(ShellKind, bool)>,
    /// True once this process itself runs as Administrator / root.
    pub running_elevated: bool,

    /// SQL Client sessions keyed by tab id, so several databases can be open
    /// at the same time.
    pub sql_sessions: HashMap<String, SqlSession>,

    // Password Prompt State
    pub password_prompt_host_id: Option<String>,
    pub password_prompt_new_tab: bool,
    pub password_prompt_input: String,
    pub password_prompt_save: bool,

    // Rename & Export JSON Modals
    pub rename_host_modal: Option<(String, String)>,
    pub renaming_folder: Option<String>,
    pub folder_rename_buffer: String,
    pub export_host_json_modal: Option<(String, String)>,
    pub host_menu_open: Option<(String, egui::Pos2)>,

    // Appearance & Customization
    pub appearance_mode: String, // "night", "day", "auto"
    pub last_applied_dark: Option<bool>,
    pub font_size: f32,
    pub ui_zoom: f32,
    pub zoom_initialized: bool,
    pub theme_name: String,

    // Panels & Modals
    pub show_left_sidebar: bool,
    pub last_terminal_size: (u16, u16),
    pub show_tunnel_drawer: bool,
    pub filter_tunnels_selected_host: bool,
    pub show_settings_modal: bool,
    pub show_add_host_modal: bool,
    pub show_add_tunnel_modal: bool,
    pub show_sync_modal: bool,

    // Host CRUD State
    pub editing_host_id: Option<String>,
    pub confirm_delete_host: Option<(String, String)>,
    pub new_host_name: String,
    pub new_host_ip: String,
    pub new_host_port: String,
    pub new_host_user: String,
    pub new_host_folder: String,
    pub new_host_auth_type: String,
    pub new_host_password: String,
    pub new_host_key_path: String,
    pub new_host_passphrase: String,

    // Host editor: which tab is showing, plus Connection/Advanced fields
    pub host_editor_tab: HostEditorTab,
    pub new_host_session_type: String,
    pub new_host_reveal_password: bool,
    pub new_host_use_jump: bool,
    pub new_host_jump_host: String,
    pub new_host_jump_port: String,
    pub new_host_jump_user: String,
    pub new_host_startup_cmd: String,
    pub new_host_sftp_dir: String,
    pub new_host_keepalive: String,
    pub new_host_compression: bool,
    pub new_host_agent_forward: bool,
    pub new_host_auto_reconnect: bool,
    pub new_host_open_sftp: bool,

    // Tunnel CRUD State
    pub editing_tunnel_id: Option<(String, String)>,
    pub confirm_delete_tunnel: Option<(String, String, String)>,
    pub new_tunnel_name: String,
    pub new_tunnel_type: String,
    pub new_tunnel_bind_host: String,
    pub new_tunnel_bind_port: String,
    pub new_tunnel_remote_host: String,
    pub new_tunnel_remote_port: String,
    pub new_tunnel_auto_start: bool,

    // Navigation & Tabs
    pub sidebar_tab: SidebarTab,
    pub open_tabs: Vec<OpenTab>,
    pub active_tab_id: Option<String>,
    pub tools_state: ToolsState,

    // Google Sync & Encrypted Export/Import
    pub sync_status_text: String,
    pub sync_email_input: String,
    pub sync_master_password: String,
    pub sync_file_path: String,
    pub sync_feedback: Option<String>,
}

impl SupertermNativeApp {
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        let vault = load_vault();
        let first_id = vault
            .profiles
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default();

        let (tx_events, rx_events) = mpsc::unbounded_channel();
        let (tx_shell_events, rx_shell_events) = mpsc::unbounded_channel();

        let mut terminals = HashMap::new();
        for p in &vault.profiles {
            let mut emu = TerminalEmulator::new(36, 120);
            emu.writeln("\x1b[35m   _____                         __                      \x1b[0m");
            emu.writeln("\x1b[35m  / ___/__  ______  ___  _____  / /____  _________ ___   \x1b[0m");
            emu.writeln("\x1b[35m  \\__ \\/ / / / __ \\/ _ \\/ ___/ / __/ _ \\/ ___/ __ `__ \\  \x1b[0m");
            emu.writeln("\x1b[35m ___/ / /_/ / /_/ /  __/ /    / /_/  __/ /  / / / / / /  \x1b[0m");
            emu.writeln("\x1b[35m/____/\\__,_/ .___/\\___/_/     \\__/\\___/_/  /_/ /_/ /_/   \x1b[0m");
            emu.writeln("\x1b[35m          /_/   Pure Native Rust SSH & Tunneling Vault   \x1b[0m");
            emu.writeln("");
            emu.writeln(&format!(
                "\x1b[36m[*] Ready to connect to {} ({}@{}:{})\x1b[0m",
                p.name, p.username, p.host, p.port
            ));
            emu.writeln("\x1b[90m[*] Click 'Connect' on a host card (or double-click a host) to open a live SSH terminal.\x1b[0m");
            terminals.insert(p.id.clone(), emu);
        }

        let initial_mode = vault.appearance_mode.clone();
        let initial_theme = vault.terminal_theme.clone();
        let initial_font = vault.font_size.clamp(10.0, 26.0);
        let initial_zoom = vault.ui_zoom.clamp(0.8, 2.2);

        let mut initial_tabs = Vec::new();
        let mut initial_active = None;
        if let Some(first) = vault.profiles.first() {
            let skey = format!("ssh:{}", first.id);
            initial_tabs.push(OpenTab {
                id: skey.clone(),
                kind: TabKind::Ssh {
                    session_key: skey.clone(),
                    host_id: first.id.clone(),
                },
                title: first.name.clone(),
            });
            if let Some(emu) = terminals.remove(&first.id) {
                terminals.insert(skey.clone(), emu);
            }
            initial_active = Some(skey);
        }

        let initial_sync_email = vault.sync_email.clone();
        let initial_sync_status = if initial_sync_email.is_empty() {
            "Local vault only (Rev #1)".to_string()
        } else {
            format!("Sync ready (Rev #{})", vault.sync_revision)
        };
        let default_stvault_path = get_vault_path()
            .with_extension("stvault")
            .display()
            .to_string();

        Self {
            rt,
            tx_events,
            rx_events,
            vault,
            selected_host_id: first_id,
            search_query: String::new(),
            active_sessions: HashMap::new(),
            connecting_sessions: HashSet::new(),
            host_tunnel_sessions: HashMap::new(),
            active_tunnels: HashMap::new(),
            terminals,
            next_tab_seq: 2,
            tx_shell_events,
            rx_shell_events,
            shell_sessions: HashMap::new(),
            pending_shell_elevation: None,
            running_elevated: is_elevated(),
            sql_sessions: HashMap::new(),
            password_prompt_host_id: None,
            password_prompt_new_tab: false,
            password_prompt_input: String::new(),
            password_prompt_save: true,
            rename_host_modal: None,
            renaming_folder: None,
            folder_rename_buffer: String::new(),
            export_host_json_modal: None,
            host_menu_open: None,
            appearance_mode: initial_mode,
            last_applied_dark: None,
            font_size: initial_font,
            ui_zoom: initial_zoom,
            zoom_initialized: false,
            theme_name: initial_theme,
            show_left_sidebar: true,
            last_terminal_size: (120, 36),
            show_tunnel_drawer: false,
            filter_tunnels_selected_host: true,
            show_settings_modal: false,
            show_add_host_modal: false,
            show_add_tunnel_modal: false,
            show_sync_modal: false,
            editing_host_id: None,
            confirm_delete_host: None,
            new_host_name: String::new(),
            new_host_ip: String::new(),
            new_host_port: "22".to_string(),
            new_host_user: "root".to_string(),
            new_host_folder: "Production".to_string(),
            new_host_auth_type: "password".to_string(),
            new_host_password: String::new(),
            new_host_key_path: "~/.ssh/id_ed25519".to_string(),
            new_host_passphrase: String::new(),
            host_editor_tab: HostEditorTab::Connection,
            new_host_session_type: "ssh".to_string(),
            new_host_reveal_password: false,
            new_host_use_jump: false,
            new_host_jump_host: String::new(),
            new_host_jump_port: "22".to_string(),
            new_host_jump_user: String::new(),
            new_host_startup_cmd: String::new(),
            new_host_sftp_dir: String::new(),
            new_host_keepalive: "30".to_string(),
            new_host_compression: false,
            new_host_agent_forward: false,
            new_host_auto_reconnect: false,
            new_host_open_sftp: false,
            editing_tunnel_id: None,
            confirm_delete_tunnel: None,
            new_tunnel_name: String::new(),
            new_tunnel_type: "local".to_string(),
            new_tunnel_bind_host: "127.0.0.1".to_string(),
            new_tunnel_bind_port: "3307".to_string(),
            new_tunnel_remote_host: "127.0.0.1".to_string(),
            new_tunnel_remote_port: "3306".to_string(),
            new_tunnel_auto_start: true,
            sidebar_tab: SidebarTab::Hosts,
            open_tabs: initial_tabs,
            active_tab_id: initial_active,
            tools_state: ToolsState::new(),
            sync_status_text: initial_sync_status,
            sync_email_input: initial_sync_email,
            sync_master_password: String::new(),
            sync_file_path: default_stvault_path,
            sync_feedback: None,
        }
    }

    fn count_active_tabs_for_host(&self, host_id: &str) -> usize {
        self.open_tabs
            .iter()
            .filter(|t| matches!(&t.kind, TabKind::Ssh { session_key, host_id: hid } if hid == host_id && (self.active_sessions.contains_key(session_key) || self.connecting_sessions.contains(session_key))))
            .count()
    }

    fn is_host_connected(&self, host_id: &str) -> bool {
        self.open_tabs.iter().any(|t| {
            matches!(&t.kind, TabKind::Ssh { session_key, host_id: hid } if hid == host_id && self.active_sessions.contains_key(session_key))
        })
    }

    fn is_host_connecting(&self, host_id: &str) -> bool {
        self.open_tabs.iter().any(|t| {
            matches!(&t.kind, TabKind::Ssh { session_key, host_id: hid } if hid == host_id && self.connecting_sessions.contains(session_key))
        })
    }

    /// Load an existing profile into the editor fields.
    fn load_host_into_editor(&mut self, host: &HostProfile) {
        self.editing_host_id = Some(host.id.clone());
        self.host_editor_tab = HostEditorTab::Connection;
        self.new_host_reveal_password = false;

        self.new_host_name = host.name.clone();
        self.new_host_ip = host.host.clone();
        self.new_host_port = host.port.to_string();
        self.new_host_user = host.username.clone();
        self.new_host_folder = host.folder.clone();
        self.new_host_auth_type = host.auth_type.clone();
        self.new_host_password = host.password.clone().unwrap_or_default();
        self.new_host_key_path = host
            .key_path
            .clone()
            .unwrap_or_else(|| "~/.ssh/id_ed25519".to_string());
        self.new_host_passphrase = host.passphrase.clone().unwrap_or_default();

        self.new_host_session_type = host.session_type.clone();
        self.new_host_use_jump = host.use_jump_host;
        self.new_host_jump_host = host.jump_host.clone();
        self.new_host_jump_port = host.jump_port.to_string();
        self.new_host_jump_user = host.jump_username.clone();
        self.new_host_startup_cmd = host.startup_command.clone();
        self.new_host_sftp_dir = host.sftp_start_dir.clone();
        self.new_host_keepalive = host.keepalive_secs.to_string();
        self.new_host_compression = host.compression;
        self.new_host_agent_forward = host.agent_forwarding;
        self.new_host_auto_reconnect = host.auto_reconnect;
        self.new_host_open_sftp = host.open_sftp_on_connect;

        self.show_add_host_modal = true;
    }

    /// Reset the editor to defaults for creating a brand-new session.
    fn reset_host_editor(&mut self) {
        let blank = HostProfile::new_default(String::new(), String::new());
        self.load_host_into_editor(&blank);
        self.editing_host_id = None;
        self.new_host_key_path = "~/.ssh/id_ed25519".to_string();
    }

    fn make_welcome_terminal(&self, host_id: &str) -> TerminalEmulator {
        let mut emu = TerminalEmulator::new(
            self.last_terminal_size.1.max(8),
            self.last_terminal_size.0.max(20),
        );
        if let Some(p) = self.vault.profiles.iter().find(|p| p.id == host_id) {
            emu.writeln(&format!(
                "\x1b[36m[*] Ready to connect to {} ({}@{}:{})\x1b[0m",
                p.name, p.username, p.host, p.port
            ));
        }
        emu
    }

    fn open_or_focus_ssh_tab(&mut self, host_id: &str) -> String {
        self.selected_host_id = host_id.to_string();
        if let Some(existing) = self
            .open_tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Ssh { host_id: hid, .. } if hid == host_id))
        {
            let tid = existing.id.clone();
            self.active_tab_id = Some(tid.clone());
            return tid;
        }
        let skey = format!("ssh:{}", host_id);
        let title = self
            .vault
            .profiles
            .iter()
            .find(|p| p.id == host_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "SSH".to_string());
        self.open_tabs.push(OpenTab {
            id: skey.clone(),
            kind: TabKind::Ssh {
                session_key: skey.clone(),
                host_id: host_id.to_string(),
            },
            title,
        });
        if !self.terminals.contains_key(&skey) {
            let emu = self.make_welcome_terminal(host_id);
            self.terminals.insert(skey.clone(), emu);
        }
        self.active_tab_id = Some(skey.clone());
        skey
    }

    fn open_new_ssh_tab(&mut self, host_id: &str) -> String {
        self.selected_host_id = host_id.to_string();
        let existing_count = self
            .open_tabs
            .iter()
            .filter(|t| matches!(&t.kind, TabKind::Ssh { host_id: hid, .. } if hid == host_id))
            .count();

        if existing_count == 0 {
            return self.open_or_focus_ssh_tab(host_id);
        }

        let seq = self.next_tab_seq;
        self.next_tab_seq += 1;
        let skey = format!("ssh:{}#{}", host_id, seq);
        let base_name = self
            .vault
            .profiles
            .iter()
            .find(|p| p.id == host_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "SSH".to_string());
        let title = format!("{} #{}", base_name, existing_count + 1);
        self.open_tabs.push(OpenTab {
            id: skey.clone(),
            kind: TabKind::Ssh {
                session_key: skey.clone(),
                host_id: host_id.to_string(),
            },
            title,
        });
        let emu = self.make_welcome_terminal(host_id);
        self.terminals.insert(skey.clone(), emu);
        self.active_tab_id = Some(skey.clone());
        skey
    }

    fn open_or_focus_tool_tab(&mut self, tool: ActiveTool) {
        // SQL Client is instanced. A plain click reuses an open session;
        // "New SQL session" from the context menu always adds one.
        if tool == ActiveTool::Sql {
            self.open_or_focus_sql_tab();
            return;
        }

        let tab_id = format!("tool:{}", tool.title());
        if !self.open_tabs.iter().any(|t| t.id == tab_id) {
            self.open_tabs.push(OpenTab {
                id: tab_id.clone(),
                kind: TabKind::Tool(tool.clone()),
                title: tool.title().to_string(),
            });
        }
        self.active_tab_id = Some(tab_id);
    }

    /// Open a fresh SQL Client tab, optionally preloaded with a saved
    /// connection.
    fn open_sql_tab(&mut self, preset: Option<SqlConfig>) {
        let seq = self.next_tab_seq;
        self.next_tab_seq += 1;
        let session_key = format!("sql:{}", seq);

        let mut sess = SqlSession::new();
        if let Some(cfg) = preset {
            sess.load_config(&cfg);
        }
        let title = format!("SQL - {}", sess.tab_title());

        self.sql_sessions.insert(session_key.clone(), sess);
        self.open_tabs.push(OpenTab {
            id: session_key.clone(),
            kind: TabKind::Sql {
                session_key: session_key.clone(),
            },
            title,
        });
        self.active_tab_id = Some(session_key);
    }

    /// Focus an existing SQL tab, or open one when none is present.
    fn open_or_focus_sql_tab(&mut self) {
        if let Some(existing) = self
            .open_tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Sql { .. }))
            .map(|t| t.id.clone())
        {
            self.active_tab_id = Some(existing);
        } else {
            self.open_sql_tab(None);
        }
    }

    /// Focus an existing tab for this shell, or start one if none is open.
    ///
    /// A plain click reuses the tab; "New session" bypasses this and always
    /// calls [`Self::open_shell_tab`] directly.
    fn open_or_focus_shell_tab(&mut self, ctx: &egui::Context, kind: ShellKind) {
        if let Some(existing) = self
            .open_tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Shell { kind: k, .. } if *k == kind))
        {
            self.active_tab_id = Some(existing.id.clone());
            return;
        }
        self.open_shell_tab(ctx, kind);
    }

    pub fn get_existing_folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = self
            .vault
            .profiles
            .iter()
            .map(|p| p.folder.clone())
            .chain(self.vault.custom_folders.iter().cloned())
            .collect();
        folders.retain(|f| !f.trim().is_empty());
        folders.sort();
        folders.dedup();
        folders
    }

    pub fn rename_folder(&mut self, old_name: &str, new_name: &str) {
        let trimmed_new = new_name.trim();
        if trimmed_new.is_empty() {
            return;
        }
        for profile in &mut self.vault.profiles {
            if profile.folder == old_name {
                profile.folder = trimmed_new.to_string();
            }
        }
        for f in &mut self.vault.custom_folders {
            if f == old_name {
                *f = trimmed_new.to_string();
            }
        }
        self.vault.custom_folders.sort();
        self.vault.custom_folders.dedup();
        let _ = save_vault(&self.vault);
    }

    pub fn delete_folder(&mut self, folder_name: &str) {
        // Hanya hapus jika tidak ada koneksi / host di dalamnya
        let has_hosts = self.vault.profiles.iter().any(|p| p.folder == folder_name);
        if !has_hosts {
            self.vault.custom_folders.retain(|f| f != folder_name);
            let _ = save_vault(&self.vault);
        }
    }

    /// Open a new local shell tab and start its PTY.
    ///
    /// Every invocation gets its own tab, so several shells can run at once.
    fn open_shell_tab(&mut self, ctx: &egui::Context, kind: ShellKind) {
        let seq = self.next_tab_seq;
        self.next_tab_seq += 1;
        let session_key = format!("shell:{}:{}", kind.title(), seq);

        let title = if self.running_elevated {
            #[cfg(windows)]
            {
                format!("{} (Administrator)", kind.title())
            }
            #[cfg(not(windows))]
            {
                format!("{} (Root)", kind.title())
            }
        } else {
            kind.title().to_string()
        };

        self.open_tabs.push(OpenTab {
            id: session_key.clone(),
            kind: TabKind::Shell {
                session_key: session_key.clone(),
                kind,
            },
            title,
        });
        self.active_tab_id = Some(session_key.clone());

        let (cols, rows) = self.last_terminal_size;

        // Register the emulator BEFORE spawning. The reader thread can deliver
        // output immediately, and inserting afterwards would discard it.
        self.terminals.insert(
            session_key.clone(),
            TerminalEmulator::new(rows.max(8), cols.max(20)),
        );

        match spawn_local_shell(
            &self.rt,
            session_key.clone(),
            kind,
            cols.max(20),
            rows.max(8),
            self.tx_shell_events.clone(),
            ctx.clone(),
        ) {
            Ok(session) => {
                self.shell_sessions.insert(session_key, session);
            }
            Err(e) => {
                if let Some(emu) = self.terminals.get_mut(&session_key) {
                    emu.writeln(&format!("\x1b[31m[!] {}\x1b[0m", e));
                    emu.writeln("");
                    emu.writeln(
                        "\x1b[90mThe tab stays open so the message above can be read. Close it with the 'x'.\x1b[0m",
                    );
                }
            }
        }
    }

    fn process_shell_events(&mut self) {
        while let Ok(ev) = self.rx_shell_events.try_recv() {
            match ev {
                ShellEvent::Output { session_key, bytes } => {
                    let emu = self
                        .terminals
                        .entry(session_key.clone())
                        .or_insert_with(|| TerminalEmulator::new(36, 120));
                    emu.process_bytes(&bytes);

                    // Keep the session's cursor view current so it can answer
                    // the next DSR query with the real position.
                    let (row, col) = emu.parser.screen().cursor_position();
                    if let Some(sess) = self.shell_sessions.get(&session_key) {
                        sess.report_cursor(row, col);
                    }
                }
                ShellEvent::Closed {
                    session_key,
                    reason,
                } => {
                    let was_live = self.shell_sessions.remove(&session_key).is_some();

                    // A shell that dies at once never produced a prompt, so
                    // closing the tab would hide why. Keep it and say so.
                    let produced_output = self
                        .terminals
                        .get(&session_key)
                        .map(|e| e.has_content())
                        .unwrap_or(false);

                    if was_live && !produced_output {
                        if let Some(emu) = self.terminals.get_mut(&session_key) {
                            emu.writeln(&format!(
                                "\x1b[31m[!] The shell exited immediately without output ({}).\x1b[0m",
                                reason
                            ));
                            emu.writeln("");
                            emu.writeln("\x1b[90mThis usually means the shell could not attach to the pseudo-console.\x1b[0m");
                            emu.writeln("\x1b[90mClose this tab with the 'x' when you are done reading.\x1b[0m");
                        }
                    } else {
                        // Normal exit: behave like an SSH shell and close.
                        self.close_tab(&session_key);
                    }
                }
            }
        }
    }

    fn stop_tunnels_for_host_if_last(&mut self, host_id: &str) {
        if self.count_active_tabs_for_host(host_id) == 0 {
            self.host_tunnel_sessions.remove(host_id);
            if let Some(profile) = self.vault.profiles.iter().find(|p| p.id == host_id) {
                for tun in &profile.tunnels {
                    if let Some(handle) = self.active_tunnels.remove(&tun.id) {
                        handle.stop();
                    }
                }
            }
        }
    }

    fn close_tab(&mut self, tab_id: &str) {
        if let Some(idx) = self.open_tabs.iter().position(|t| t.id == tab_id) {
            let removed = self.open_tabs.remove(idx);
            match removed.kind {
                TabKind::Ssh { session_key, host_id } => {
                    self.connecting_sessions.remove(&session_key);
                    if let Some(sess) = self.active_sessions.remove(&session_key) {
                        sess.disconnect();
                    }
                    self.stop_tunnels_for_host_if_last(&host_id);
                }
                TabKind::Shell { session_key, .. } => {
                    if let Some(sess) = self.shell_sessions.remove(&session_key) {
                        sess.kill();
                    }
                    self.terminals.remove(&session_key);
                }
                TabKind::Sql { session_key } => {
                    // Dropping the session also drops its result channel; any
                    // query still running finds no receiver and goes away.
                    self.sql_sessions.remove(&session_key);
                }
                TabKind::Tool(_) => {}
            }
            if self.active_tab_id.as_deref() == Some(tab_id) {
                if self.open_tabs.is_empty() {
                    self.active_tab_id = None;
                } else {
                    let next_idx = idx.min(self.open_tabs.len() - 1);
                    let next_tab = &self.open_tabs[next_idx];
                    self.active_tab_id = Some(next_tab.id.clone());
                    if let TabKind::Ssh { host_id, .. } = &next_tab.kind {
                        self.selected_host_id = host_id.clone();
                    }
                }
            }
        }
    }

    fn close_all_tabs_for_host(&mut self, host_id: &str) {
        let tab_ids: Vec<String> = self
            .open_tabs
            .iter()
            .filter(|t| matches!(&t.kind, TabKind::Ssh { host_id: hid, .. } if hid == host_id))
            .map(|t| t.id.clone())
            .collect();
        for tid in tab_ids {
            self.close_tab(&tid);
        }
    }

    fn active_tab_kind(&self) -> Option<TabKind> {
        let aid = self.active_tab_id.as_deref()?;
        self.open_tabs.iter().find(|t| t.id == aid).map(|t| t.kind.clone())
    }

    fn push_log(&mut self, session_key: &str, line: String) {
        let emu = self
            .terminals
            .entry(session_key.to_string())
            .or_insert_with(|| TerminalEmulator::new(36, 120));
        let colored = if line.starts_with("[+]") {
            format!("\x1b[32m{}\x1b[0m", line)
        } else if line.starts_with("[*]") {
            format!("\x1b[36m{}\x1b[0m", line)
        } else if line.starts_with("[!]") {
            format!("\x1b[31m{}\x1b[0m", line)
        } else {
            line
        };
        emu.writeln(&colored);
    }

    fn push_log_for_host(&mut self, host_id: &str, line: String) {
        let target_key = if let Some(TabKind::Ssh { session_key, host_id: hid }) = self.active_tab_kind() {
            if hid == host_id {
                Some(session_key)
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            self.open_tabs.iter().find_map(|t| match &t.kind {
                TabKind::Ssh { session_key, host_id: hid } if hid == host_id => Some(session_key.clone()),
                _ => None,
            })
        });

        if let Some(skey) = target_key {
            self.push_log(&skey, line);
        }
    }

    fn append_ssh_output(&mut self, session_key: &str, bytes: &[u8]) {
        let emu = self
            .terminals
            .entry(session_key.to_string())
            .or_insert_with(|| TerminalEmulator::new(36, 120));
        emu.process_bytes(bytes);
    }

    fn initiate_connect(
        &mut self,
        ctx: &egui::Context,
        host_id: &str,
        override_password: Option<String>,
        force_new_tab: bool,
    ) {
        if !force_new_tab {
            // If an existing tab for this host is already connected or connecting, just focus it!
            if let Some(existing) = self.open_tabs.iter().find(|t| {
                matches!(&t.kind, TabKind::Ssh { session_key, host_id: hid } if hid == host_id && (self.active_sessions.contains_key(session_key) || self.connecting_sessions.contains(session_key)))
            }) {
                self.active_tab_id = Some(existing.id.clone());
                self.selected_host_id = host_id.to_string();
                return;
            }
        }

        let profile = match self.vault.profiles.iter().find(|p| p.id == host_id) {
            Some(p) => p.clone(),
            None => return,
        };

        let effective_password = override_password.or_else(|| {
            profile
                .password
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
        });

        if profile.auth_type == "password" && effective_password.is_none() {
            self.password_prompt_host_id = Some(profile.id.clone());
            self.password_prompt_new_tab = force_new_tab;
            self.password_prompt_input.clear();
            return;
        }

        let session_key = if force_new_tab {
            self.open_new_ssh_tab(&profile.id)
        } else {
            self.open_or_focus_ssh_tab(&profile.id)
        };

        self.connecting_sessions.insert(session_key.clone());
        let (init_cols, init_rows) = (
            self.last_terminal_size.0.max(20) as u32,
            self.last_terminal_size.1.max(8) as u32,
        );
        self.push_log(
            &session_key,
            format!(
                "[*] Connecting via SSH to {}@{}:{}...",
                profile.username, profile.host, profile.port
            ),
        );

        spawn_live_ssh_connection(
            &self.rt,
            session_key,
            profile.id,
            profile.host,
            profile.port,
            profile.username,
            profile.auth_type,
            effective_password,
            profile.key_path,
            profile.passphrase.filter(|s| !s.is_empty()),
            init_cols,
            init_rows,
            self.tx_events.clone(),
            ctx.clone(),
        );
    }

    fn disconnect_session(&mut self, session_key: &str) {
        self.connecting_sessions.remove(session_key);
        if let Some(sess) = self.active_sessions.remove(session_key) {
            let host_id = sess.host_id.clone();
            sess.disconnect();
            self.stop_tunnels_for_host_if_last(&host_id);
        }
    }

    fn process_ssh_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx_events.try_recv() {
            match ev {
                SshEvent::Connected {
                    session_key,
                    host_id,
                    session,
                } => {
                    self.connecting_sessions.remove(&session_key);
                    let (cols, rows) = self.last_terminal_size;
                    session.send_resize(cols as u32, rows as u32);
                    self.active_sessions.insert(session_key.clone(), session.clone());
                    self.push_log(
                        &session_key,
                        "[+] SSH session connected! Type directly in the terminal.".to_string(),
                    );

                    // Shared tunnel protection: only start tunnels if NO active tunnel session owns them yet!
                    if self.host_tunnel_sessions.contains_key(&host_id) {
                        self.push_log(
                            &session_key,
                            "[*] Tunnels for this host are already active on another tab (kept running)."
                                .to_string(),
                        );
                    } else {
                        self.host_tunnel_sessions.insert(host_id.clone(), session.clone());
                        let mut started_count = 0usize;
                        if let Some(profile) = self.vault.profiles.iter_mut().find(|p| p.id == host_id) {
                            for tun in &mut profile.tunnels {
                                if (tun.auto_start || tun.is_active) && !self.active_tunnels.contains_key(&tun.id) {
                                    tun.is_active = true;
                                    let handle = spawn_tunnel(
                                        &self.rt,
                                        session.clone(),
                                        tun.clone(),
                                        self.tx_events.clone(),
                                        ctx.clone(),
                                    );
                                    self.active_tunnels.insert(tun.id.clone(), handle);
                                    started_count += 1;
                                }
                            }
                        }
                        if started_count > 0 {
                            self.push_log(
                                &session_key,
                                format!("[+] Auto-started {} port forward tunnel(s).", started_count),
                            );
                        }
                    }
                }
                SshEvent::Output { session_key, bytes } => {
                    self.append_ssh_output(&session_key, &bytes);
                }
                SshEvent::Error { session_key, message } => {
                    self.connecting_sessions.remove(&session_key);
                    self.push_log(&session_key, format!("[!] ERROR: {}", message));
                }
                SshEvent::Disconnected {
                    session_key,
                    host_id,
                    reason,
                } => {
                    self.connecting_sessions.remove(&session_key);
                    self.active_sessions.remove(&session_key);
                    self.stop_tunnels_for_host_if_last(&host_id);
                    self.push_log(&session_key, format!("[!] Disconnected: {}", reason));
                    if reason == "Remote shell closed" || reason == "Disconnected by user" {
                        self.close_tab(&session_key);
                    }
                }
                SshEvent::HostKeyVerification {
                    session_key,
                    host_id,
                    fingerprint,
                } => {
                    self.push_log(&session_key, format!("[!] Host key verification needed: {}", fingerprint));
                }
                SshEvent::TunnelLog { host_id, message } => {
                    self.push_log_for_host(&host_id, message);
                }
            }
        }
    }
}

impl eframe::App for SupertermNativeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.zoom_initialized {
            ctx.set_zoom_factor(self.ui_zoom);
            self.zoom_initialized = true;
        }

        // Deferred actions raised by the floating host context menu.
        let mut menu_connect_req: Option<String> = None;
        let mut menu_connect_new_tab_req: Option<String> = None;
        let mut menu_disconnect_req: Option<String> = None;
        let mut menu_disconnect_tab_req: Option<String> = None;
        let mut menu_edit_req: Option<HostProfile> = None;
        let mut menu_duplicate_req: Option<String> = None;
        let mut menu_rename_req: Option<(String, String)> = None;
        let mut menu_export_req: Option<HostProfile> = None;
        let mut menu_delete_req: Option<(String, String)> = None;

        let os_theme = ctx.system_theme();
        let effective_is_dark = match self.appearance_mode.as_str() {
            "day" => false,
            "auto" => os_theme != Some(egui::Theme::Light),
            _ => true, // "night"
        };

        if self.last_applied_dark != Some(effective_is_dark) {
            configure_superterm_theme(ctx, effective_is_dark);
            if !effective_is_dark && (self.theme_name == "Dracula" || self.theme_name == "Tokyo Night" || self.theme_name == "Nord" || self.theme_name == "Monokai Pro" || self.theme_name == "Cyberpunk") {
                self.theme_name = "Daylight Clean".to_string();
            } else if effective_is_dark && (self.theme_name == "Daylight Clean" || self.theme_name == "Solarized Light") {
                self.theme_name = "Dracula".to_string();
            }
            self.last_applied_dark = Some(effective_is_dark);
        }

        // Drain live SSH and Tool events from Tokio runtime
        self.process_ssh_events(ctx);
        self.process_shell_events();
        self.tools_state.process_events();

        // Every open SQL session drains its own results.
        for sess in self.sql_sessions.values_mut() {
            sess.process_events();
        }

        // The SQL Client cannot borrow the vault mutably while drawing, so it
        // parks its writes here instead.
        if let Some(cfg) = self.tools_state.sql_save_req.take() {
            match self
                .vault
                .sql_connections
                .iter_mut()
                .find(|c| c.id == cfg.id)
            {
                Some(existing) => *existing = cfg,
                None => self.vault.sql_connections.push(cfg),
            }
            let _ = save_vault(&self.vault);
        }
        if let Some(id) = self.tools_state.sql_delete_req.take() {
            self.vault.sql_connections.retain(|c| c.id != id);
            // Any session still showing that connection is detached from it.
            for sess in self.sql_sessions.values_mut() {
                if sess.selected_id.as_deref() == Some(id.as_str()) {
                    sess.reset_editor();
                }
            }
            let _ = save_vault(&self.vault);
        }

        // Note Vault writes, parked for the same reason as the SQL ones.
        if self.tools_state.note_create_req {
            self.tools_state.note_create_req = false;

            // An unsaved note would be lost when the new one takes focus.
            if self.tools_state.note_dirty {
                if let Some(cur) = self.tools_state.note_selected.clone() {
                    if let Some(existing) = self.vault.notes.iter_mut().find(|n| n.id == cur) {
                        existing.title = self.tools_state.note_title_buf.clone();
                        existing.body = self.tools_state.note_body_buf.clone();
                        existing.updated_at = chrono::Local::now().to_rfc3339();
                    }
                }
            }

            let note = NoteEntry {
                id: format!("note-{}", uuid::Uuid::new_v4()),
                title: "Untitled note".to_string(),
                body: String::new(),
                updated_at: chrono::Local::now().to_rfc3339(),
            };
            self.vault.notes.push(note.clone());
            self.tools_state.load_note(&note);
            self.tools_state.note_notice = None;
            let _ = save_vault(&self.vault);
        }
        if let Some(note) = self.tools_state.note_save_req.take() {
            match self.vault.notes.iter_mut().find(|n| n.id == note.id) {
                Some(existing) => *existing = note.clone(),
                None => self.vault.notes.push(note.clone()),
            }
            // Only clear the flag when the saved copy is the one on screen;
            // otherwise a pending switch would mark the new note clean.
            if self.tools_state.note_selected.as_deref() == Some(note.id.as_str()) {
                self.tools_state.note_dirty = false;
            }
            self.tools_state.note_notice = Some("Saved.".to_string());
            let _ = save_vault(&self.vault);
        }
        if let Some(id) = self.tools_state.note_delete_req.take() {
            self.vault.notes.retain(|n| n.id != id);
            if self.tools_state.note_selected.as_deref() == Some(id.as_str()) {
                self.tools_state.clear_note_editor();
            }
            self.tools_state.note_notice = Some("Note deleted.".to_string());
            let _ = save_vault(&self.vault);
        }

        let is_light_terminal = self.theme_name == "Daylight Clean" || self.theme_name == "Solarized Light";
        let terminal_bg = match self.theme_name.as_str() {
            "Tokyo Night" => Color32::from_rgb(26, 27, 38),
            "Nord" => Color32::from_rgb(46, 52, 64),
            "Monokai Pro" => Color32::from_rgb(39, 40, 34),
            "Cyberpunk" => Color32::from_rgb(10, 13, 20),
            "Daylight Clean" => Color32::from_rgb(248, 250, 252),
            "Solarized Light" => Color32::from_rgb(253, 246, 227),
            _ => Color32::from_rgb(13, 17, 23),
        };
        let terminal_default_fg = match self.theme_name.as_str() {
            "Daylight Clean" => Color32::from_rgb(15, 23, 42),
            "Solarized Light" => Color32::from_rgb(71, 85, 105),
            _ => Color32::from_rgb(230, 237, 243),
        };

        let (
            sidebar_bg,
            header_bg,
            card_bg,
            card_selected_bg,
            border_color,
            text_primary,
            text_secondary,
            btn_neutral_bg,
            google_card_bg,
            accent_purple,
            accent_green,
            accent_blue,
            accent_amber,
        ) = if effective_is_dark {
            (
                Color32::from_rgb(20, 24, 31),
                Color32::from_rgb(22, 27, 34),
                Color32::from_rgb(28, 33, 41),
                Color32::from_rgb(40, 34, 62),
                Color32::from_rgb(48, 54, 61),
                Color32::from_rgb(240, 246, 252),
                Color32::from_rgb(156, 163, 175),
                Color32::from_rgb(33, 38, 45),
                Color32::from_rgb(15, 19, 25),
                Color32::from_rgb(168, 85, 247),
                Color32::from_rgb(52, 211, 153),
                Color32::from_rgb(96, 165, 250),
                Color32::from_rgb(251, 191, 36),
            )
        } else {
            (
                Color32::from_rgb(241, 245, 249),
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(237, 233, 254),
                Color32::from_rgb(203, 213, 225),
                Color32::from_rgb(15, 23, 42),
                Color32::from_rgb(71, 85, 105),
                Color32::from_rgb(226, 232, 240),
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(124, 58, 237),
                Color32::from_rgb(5, 150, 105),
                Color32::from_rgb(37, 99, 235),
                Color32::from_rgb(217, 119, 6),
            )
        };
        let border_stroke = Stroke::new(1.0_f32, border_color);

        let selected_profile = self
            .vault
            .profiles
            .iter()
            .find(|p| p.id == self.selected_host_id)
            .cloned();
        let selected_name = selected_profile
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "No Host Selected".to_string());
        let active_ssh_key = match self.active_tab_kind() {
            Some(TabKind::Ssh { session_key, .. }) => Some(session_key),
            _ => None,
        };
        let is_selected_connected = active_ssh_key
            .as_ref()
            .map(|k| self.active_sessions.contains_key(k))
            .unwrap_or_else(|| self.is_host_connected(&self.selected_host_id));
        let is_selected_connecting = active_ssh_key
            .as_ref()
            .map(|k| self.connecting_sessions.contains(k))
            .unwrap_or_else(|| self.is_host_connecting(&self.selected_host_id));

        // =========================================================================
        // TOP NAVIGATION PANEL
        // =========================================================================
        egui::TopBottomPanel::top("top_header")
            .frame(
                egui::Frame::none()
                    .fill(header_bg)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::symmetric(14.0_f32, 9.0_f32)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if draw_sidebar_toggle_button(ui, self.show_left_sidebar, text_primary, btn_neutral_bg).clicked() {
                        self.show_left_sidebar = !self.show_left_sidebar;
                    }

                    egui::Frame::none()
                        .fill(btn_neutral_bg)
                        .stroke(Stroke::new(1.0_f32, accent_purple))
                        .rounding(Rounding::same(5.0_f32))
                        .inner_margin(egui::Margin::symmetric(10.0_f32, 4.0_f32))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot = if is_selected_connected {
                                    accent_green
                                } else if is_selected_connecting {
                                    accent_amber
                                } else {
                                    Color32::from_rgb(107, 114, 128)
                                };
                                draw_status_dot(ui, dot, 3.5_f32);
                                ui.label(
                                    RichText::new(&selected_name)
                                        .color(text_primary)
                                        .size(12.5_f32)
                                        .strong(),
                                );
                            });
                        });

                    ui.add_space(6.0_f32);
                    if is_selected_connected {
                        ui.label(
                            RichText::new("SSH Live Connected")
                                .color(accent_green)
                                .size(12.0_f32),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Disconnect").color(Color32::WHITE).size(11.5_f32),
                                )
                                .fill(Color32::from_rgb(185, 28, 28)),
                            )
                            .clicked()
                        {
                            if let Some(skey) = &active_ssh_key {
                                self.disconnect_session(skey);
                            } else {
                                let hid = self.selected_host_id.clone();
                                self.close_all_tabs_for_host(&hid);
                            }
                        }
                    } else if is_selected_connecting {
                        ui.label(
                            RichText::new("Connecting...")
                                .color(accent_amber)
                                .size(12.0_f32),
                        );
                    } else if selected_profile.is_some() {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Connect SSH")
                                        .color(Color32::WHITE)
                                        .size(11.5_f32)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(22, 163, 74)),
                            )
                            .clicked()
                        {
                            let hid = self.selected_host_id.clone();
                            self.initiate_connect(ctx, &hid, None, false);
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Settings").color(text_primary).size(12.0_f32),
                                )
                                .fill(btn_neutral_bg),
                            )
                            .clicked()
                        {
                            self.show_settings_modal = !self.show_settings_modal;
                        }

                        // Quick Appearance Mode Toggle (Night / Day / Auto)
                        let mode_label = match self.appearance_mode.as_str() {
                            "day" => "Mode: Day",
                            "auto" => "Mode: Auto",
                            _ => "Mode: Night",
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(mode_label).color(text_primary).size(12.0_f32),
                                )
                                .fill(btn_neutral_bg),
                            )
                            .on_hover_text("Click to cycle Night (Dark) / Day (Light) / Auto (System)")
                            .clicked()
                        {
                            self.appearance_mode = match self.appearance_mode.as_str() {
                                "night" => "day".to_string(),
                                "day" => "auto".to_string(),
                                _ => "night".to_string(),
                            };
                            self.vault.appearance_mode = self.appearance_mode.clone();
                            let _ = save_vault(&self.vault);
                        }

                        let active_tunnels_count = self.active_tunnels.len();
                        let tunnel_btn_text = format!("Tunnels ({} live)", active_tunnels_count);
                        let (tunnel_btn_fill, tunnel_btn_txt) = if self.show_tunnel_drawer {
                            (Color32::from_rgb(37, 99, 235), Color32::WHITE)
                        } else {
                            (btn_neutral_bg, text_primary)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(tunnel_btn_text)
                                        .color(tunnel_btn_txt)
                                        .size(12.0_f32),
                                )
                                .fill(tunnel_btn_fill),
                            )
                            .clicked()
                        {
                            self.show_tunnel_drawer = !self.show_tunnel_drawer;
                        }
                    });
                });
            });

        // =========================================================================
        // BOTTOM STATUS BAR
        // =========================================================================
        egui::TopBottomPanel::bottom("bottom_bar")
            .frame(
                egui::Frame::none()
                    .fill(header_bg)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::symmetric(14.0_f32, 6.0_f32)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Vault: Cross-Platform Portable")
                            .size(11.0_f32)
                            .color(accent_purple),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Active SSH Sessions: {} | Live Tunnels: {}",
                            self.active_sessions.len(),
                            self.active_tunnels.len()
                        ))
                        .size(11.0_f32)
                        .color(text_secondary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Theme: {} | Font: {}px | UTF-8",
                                self.theme_name, self.font_size
                            ))
                            .size(11.0_f32)
                            .color(text_secondary),
                        );
                    });
                });
            });

        // =========================================================================
        // FAR-LEFT ACTIVITY RAIL: HOSTS / TOOLS
        // =========================================================================
        egui::SidePanel::left("activity_rail")
            .frame(
                egui::Frame::none()
                    .fill(header_bg)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::symmetric(6.0_f32, 12.0_f32)),
            )
            .exact_width(64.0_f32)
            .resizable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for (tab, label, is_tools) in [
                        (SidebarTab::Hosts, "Hosts", false),
                        (SidebarTab::Tools, "Tools", true),
                    ] {
                        let is_active = self.sidebar_tab == tab && self.show_left_sidebar;
                        let fill = if is_active {
                            card_selected_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let stroke = if is_active {
                            Stroke::new(1.2_f32, accent_purple)
                        } else {
                            Stroke::NONE
                        };
                        let icon_col = if is_active {
                            accent_purple
                        } else {
                            text_secondary
                        };

                        let res = egui::Frame::none()
                            .fill(fill)
                            .stroke(stroke)
                            .rounding(Rounding::same(6.0_f32))
                            .inner_margin(egui::Margin::symmetric(6.0_f32, 8.0_f32))
                            .show(ui, |ui| {
                                ui.set_width(48.0_f32);
                                ui.vertical_centered(|ui| {
                                    draw_nav_icon(ui, is_tools, icon_col);
                                    ui.add_space(3.0_f32);
                                    ui.label(
                                        RichText::new(label)
                                            .size(10.5_f32)
                                            .color(if is_active { text_primary } else { text_secondary }),
                                    );
                                });
                            })
                            .response
                            .interact(egui::Sense::click());

                        if res.clicked() {
                            if self.sidebar_tab == tab {
                                self.show_left_sidebar = !self.show_left_sidebar;
                            } else {
                                self.sidebar_tab = tab;
                                self.show_left_sidebar = true;
                            }
                            if tab == SidebarTab::Hosts {
                                let hid = self.selected_host_id.clone();
                                if !hid.is_empty() {
                                    self.open_or_focus_ssh_tab(&hid);
                                }
                            } else if tab == SidebarTab::Tools && !matches!(self.active_tab_kind(), Some(TabKind::Tool(_))) {
                                self.open_or_focus_tool_tab(ActiveTool::Ping);
                            }
                        }
                        ui.add_space(6.0_f32);
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if draw_sidebar_toggle_button(ui, self.show_left_sidebar, text_primary, btn_neutral_bg).clicked() {
                        self.show_left_sidebar = !self.show_left_sidebar;
                    }
                });
            });

        // =========================================================================
        // SUB-SIDEBAR: HOSTS EXPLORER OR TOOLS LIST (COLLAPSIBLE)
        // =========================================================================
        if self.show_left_sidebar {
        egui::SidePanel::left("left_sidebar")
            .frame(
                egui::Frame::none()
                    .fill(sidebar_bg)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::same(12.0_f32)),
            )
            .default_width(290.0_f32)
            .width_range(240.0_f32..=400.0_f32)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Superterm")
                            .size(18.0_f32)
                            .strong()
                            .color(text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if draw_sidebar_toggle_button(ui, true, text_secondary, btn_neutral_bg).clicked() {
                            self.show_left_sidebar = false;
                        }
                    });
                });
                ui.label(
                    RichText::new(match self.sidebar_tab {
                        SidebarTab::Hosts => "Hosts  -  SSH & Tunneling Vault",
                        SidebarTab::Tools => "Tools  -  Network & Security Utilities",
                    })
                    .size(11.0_f32)
                    .color(text_secondary),
                );

                ui.add_space(10.0_f32);

                if self.sidebar_tab == SidebarTab::Tools {
                    // `focus` reuses an open tab; `fresh` always starts another.
                    let mut shell_focus_req: Option<ShellKind> = None;
                    let mut shell_new_req: Option<ShellKind> = None;
                    // Outer Some = a new SQL session was asked for; inner Some
                    // carries the saved connection to preload.
                    let mut sql_new_req: Option<Option<SqlConfig>> = None;

                    ScrollArea::vertical().show(ui, |ui| {
                        // ---- Local shells, above the network tools ----
                        for kind in ShellKind::all() {
                            let is_open = self
                                .open_tabs
                                .iter()
                                .any(|t| matches!(&t.kind, TabKind::Shell { kind: k, .. } if *k == kind));

                            let row = egui::Frame::none()
                                .fill(if is_open { card_selected_bg } else { card_bg })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if is_open { accent_green } else { border_color },
                                ))
                                .rounding(Rounding::same(6.0_f32))
                                .inner_margin(egui::Margin::symmetric(10.0_f32, 6.0_f32))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        draw_shell_icon(ui, kind, accent_green);
                                        ui.add_space(6.0_f32);
                                        ui.label(
                                            RichText::new(kind.title())
                                                .size(13.0_f32)
                                                .strong()
                                                .color(text_primary),
                                        );
                                    });
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Click to open  -  right-click for a new session");

                            if row.clicked() {
                                shell_focus_req = Some(kind);
                            }
                            row.context_menu(|ui| {
                                ui.set_min_width(150.0_f32);
                                if ui.button(format!("New {} session", kind.title())).clicked() {
                                    shell_new_req = Some(kind);
                                    ui.close_menu();
                                }
                            });
                            ui.add_space(5.0_f32);
                        }

                        ui.add_space(2.0_f32);
                        ui.separator();
                        ui.add_space(6.0_f32);

                        for tool in ActiveTool::all() {
                            // SQL Client is instanced, so "active" means any
                            // SQL tab is open rather than this one tool tab.
                            let is_active = if *tool == ActiveTool::Sql {
                                matches!(self.active_tab_kind(), Some(TabKind::Sql { .. }))
                            } else {
                                matches!(self.active_tab_kind(), Some(TabKind::Tool(ref t)) if t == tool)
                            };
                            let fill = if is_active { card_selected_bg } else { card_bg };
                            let stroke = if is_active {
                                Stroke::new(1.2_f32, accent_purple)
                            } else {
                                Stroke::new(1.0_f32, border_color)
                            };

                            let card_res = egui::Frame::none()
                                .fill(fill)
                                .stroke(stroke)
                                .rounding(Rounding::same(6.0_f32))
                                .inner_margin(egui::Margin::symmetric(10.0_f32, 6.0_f32))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        draw_tool_icon(ui, tool, accent_blue);
                                        ui.add_space(6.0_f32);
                                        ui.label(
                                            RichText::new(tool.title())
                                                .size(13.0_f32)
                                                .strong()
                                                .color(text_primary),
                                        );
                                    });
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);

                            // SQL Client can hold several sessions at once, so
                            // it gets the same click/right-click split as the
                            // local shells.
                            let card_res = if *tool == ActiveTool::Sql {
                                let r = card_res.on_hover_text(
                                    "Click to open  -  right-click for a new session",
                                );
                                r.context_menu(|ui| {
                                    ui.set_min_width(190.0_f32);
                                    if ui.button("New SQL session").clicked() {
                                        sql_new_req = Some(None);
                                        ui.close_menu();
                                    }
                                    if !self.vault.sql_connections.is_empty() {
                                        ui.separator();
                                        ui.label(
                                            RichText::new("Open saved connection")
                                                .size(10.5_f32)
                                                .color(text_secondary),
                                        );
                                        for cfg in &self.vault.sql_connections {
                                            if ui
                                                .button(format!("{}  [{}]", cfg.name, cfg.db_type))
                                                .clicked()
                                            {
                                                sql_new_req = Some(Some(cfg.clone()));
                                                ui.close_menu();
                                            }
                                        }
                                    }
                                });
                                r
                            } else {
                                card_res
                            };

                            if card_res.clicked() {
                                if *tool == ActiveTool::NetworkInterfaces
                                    && !self.tools_state.outputs.contains_key(tool.title())
                                {
                                    spawn_network_interfaces(
                                        &self.rt,
                                        self.tools_state.tx_events.clone(),
                                        ctx.clone(),
                                    );
                                }
                                if let Some(prof) = &selected_profile {
                                    if matches!(tool, ActiveTool::Ping | ActiveTool::Traceroute | ActiveTool::PortScanner) {
                                        // Pre-fill target with selected host if user hasn't customized it
                                        if self.tools_state.ping_target == "8.8.8.8" {
                                            self.tools_state.ping_target = prof.host.clone();
                                        }
                                        if self.tools_state.traceroute_target == "1.1.1.1" {
                                            self.tools_state.traceroute_target = prof.host.clone();
                                        }
                                        if self.tools_state.scan_target == "127.0.0.1" {
                                            self.tools_state.scan_target = prof.host.clone();
                                        }
                                    }
                                }
                                self.open_or_focus_tool_tab(tool.clone());
                            }
                            ui.add_space(5.0_f32);
                        }
                    });

                    // Handled after the scroll area so `self` is free again.
                    //
                    // A plain click reuses an open tab, and only asks about
                    // elevation when it would actually start something.
                    if let Some(kind) = shell_focus_req {
                        let already_open = self
                            .open_tabs
                            .iter()
                            .any(|t| matches!(&t.kind, TabKind::Shell { kind: k, .. } if *k == kind));
                        if already_open || self.running_elevated {
                            self.open_or_focus_shell_tab(ctx, kind);
                        } else {
                            self.pending_shell_elevation = Some((kind, false));
                        }
                    }
                    if let Some(kind) = shell_new_req {
                        if self.running_elevated {
                            self.open_shell_tab(ctx, kind);
                        } else {
                            self.pending_shell_elevation = Some((kind, true));
                        }
                    }
                    if let Some(preset) = sql_new_req {
                        self.open_sql_tab(preset);
                    }
                    return;
                }

                MenuEdit::single("search_query")
    .hint("Search host, IP, or tag...")
    .width(f32::INFINITY)
    .show(ui, &mut self.search_query);

                ui.add_space(6.0_f32);

                if ui
                    .add_sized(
                        [ui.available_width(), 28.0_f32],
                        egui::Button::new(
                            RichText::new("+ Add New Host")
                                .color(Color32::WHITE)
                                .size(12.5_f32)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(124, 58, 237)),
                    )
                    .clicked()
                {
                    self.reset_host_editor();
                }

                ui.add_space(10.0_f32);
                ui.separator();
                ui.add_space(4.0_f32);

                let scroll_height = (ui.available_height() - 68.0_f32).max(100.0_f32);
                let mut connect_request_id: Option<String> = None;
                let mut focus_host_tab_req: Option<String> = None;
                let mut menu_open_req: Option<(String, egui::Pos2)> = None;
                let mut delete_folder_req: Option<String> = None;
                let mut rename_folder_req: Option<String> = None;

                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(scroll_height)
                    .show(ui, |ui| {
                        let folders = self.get_existing_folders();

                        for folder in folders {
                            let query = self.search_query.to_lowercase();
                            let matching_hosts: Vec<HostProfile> = self
                                .vault
                                .profiles
                                .iter()
                                .filter(|p| {
                                    p.folder == folder
                                        && (p.name.to_lowercase().contains(&query)
                                            || p.host.to_lowercase().contains(&query)
                                            || p.tags.iter().any(|t| t.to_lowercase().contains(&query)))
                                })
                                .cloned()
                                .collect();

                            if matching_hosts.is_empty() && !query.is_empty() {
                                continue;
                            }

                            let folder_label =
                                RichText::new(format!("{} ({})", folder, matching_hosts.len()))
                                    .color(text_primary)
                                    .size(12.5_f32)
                                    .strong();

                            let collapsing = egui::CollapsingHeader::new(folder_label)
                                .default_open(true);
                            let header_res = collapsing.show(ui, |ui| {
                                if matching_hosts.is_empty() {
                                    ui.label(RichText::new("   (Empty folder)").color(text_secondary).size(11.0_f32));
                                }
                                for host in matching_hosts {
                                        let is_selected = self.selected_host_id == host.id;
                                        let is_connected = self.is_host_connected(&host.id);
                                        let is_connecting = self.is_host_connecting(&host.id);

                                        let row_fill = if is_selected {
                                            card_selected_bg
                                        } else {
                                            card_bg
                                        };
                                        let row_stroke = if is_selected {
                                            Stroke::new(1.0_f32, accent_purple)
                                        } else {
                                            Stroke::new(0.8_f32, border_color)
                                        };
                                        let led_color = if is_connected {
                                            accent_green
                                        } else if is_connecting {
                                            accent_amber
                                        } else {
                                            Color32::from_rgb(107, 114, 128)
                                        };

                                        // Full connection string, shown only on hover.
                                        let host_hint = format!(
                                            "{}@{}:{}",
                                            host.username, host.host, host.port
                                        );

                                        // One rect is allocated for the whole row and then split
                                        // geometrically, so the frame and the kebab button share
                                        // exactly the same top and bottom edge.
                                        //
                                        // The kebab must stay OUTSIDE the host frame: a row-wide
                                        // `interact()` overlay is registered after it and would
                                        // otherwise swallow every click it receives.
                                        const KEBAB_W: f32 = 28.0_f32;
                                        const ROW_H: f32 = 28.0_f32;

                                        let full_rect = ui
                                            .allocate_space(Vec2::new(ui.available_width(), ROW_H))
                                            .1;
                                        let split_x = full_rect.max.x - KEBAB_W;
                                        let frame_rect = egui::Rect::from_min_max(
                                            full_rect.min,
                                            egui::Pos2::new(split_x, full_rect.max.y),
                                        );
                                        let kebab_rect = egui::Rect::from_min_max(
                                            egui::Pos2::new(split_x, full_rect.min.y),
                                            full_rect.max,
                                        );

                                        // ---- Left part: host frame (click / double-click / right-click) ----
                                        //
                                        // Painted straight onto `frame_rect` rather than via
                                        // `egui::Frame`, which would shrink-wrap its content and
                                        // leave a gap before the kebab button.
                                        ui.painter().rect_filled(
                                            frame_rect,
                                            Rounding {
                                                nw: 5.0_f32,
                                                sw: 5.0_f32,
                                                ne: 0.0_f32,
                                                se: 0.0_f32,
                                            },
                                            row_fill,
                                        );
                                        ui.painter().rect_stroke(
                                            frame_rect,
                                            Rounding {
                                                nw: 5.0_f32,
                                                sw: 5.0_f32,
                                                ne: 0.0_f32,
                                                se: 0.0_f32,
                                            },
                                            row_stroke,
                                        );

                                        // Content sits inside the painted rect, inset horizontally.
                                        let content_rect = frame_rect.shrink2(Vec2::new(8.0_f32, 0.0_f32));
                                        ui.allocate_new_ui(
                                            egui::UiBuilder::new()
                                                .max_rect(content_rect)
                                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                            |ui| {
                                                draw_single_server_icon(
                                                    ui,
                                                    if is_connected {
                                                        accent_green
                                                    } else {
                                                        text_secondary
                                                    },
                                                    led_color,
                                                );
                                                ui.add_space(6.0_f32);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&host.name)
                                                            .color(text_primary)
                                                            .size(12.0_f32)
                                                            .strong(),
                                                    )
                                                    .truncate()
                                                    .selectable(false),
                                                );
                                            },
                                        );

                                        // Overlay covering only the frame half, never the kebab.
                                        let row_resp = ui
                                            .interact(
                                                frame_rect,
                                                egui::Id::new(("host_row", &host.id)),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(&host_hint);

                                        if row_resp.double_clicked() {
                                            // Double-click connects.
                                            connect_request_id = Some(host.id.clone());
                                        } else if row_resp.clicked() {
                                            focus_host_tab_req = Some(host.id.clone());
                                        }
                                        if row_resp.secondary_clicked() {
                                            let at = row_resp
                                                .interact_pointer_pos()
                                                .unwrap_or_else(|| row_resp.rect.left_top());
                                            menu_open_req = Some((host.id.clone(), at));
                                        }

                                        // ---- Right part: kebab button (left-click) ----
                                        let kebab = ui
                                            .interact(
                                                kebab_rect,
                                                egui::Id::new(("host_kebab", &host.id)),
                                                egui::Sense::click(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text("Host actions");

                                        paint_kebab(
                                            ui.painter(),
                                            kebab_rect,
                                            row_fill,
                                            row_stroke,
                                            text_secondary,
                                            kebab.hovered(),
                                            btn_neutral_bg,
                                        );

                                        if kebab.clicked() {
                                            menu_open_req = Some((
                                                host.id.clone(),
                                                kebab_rect.left_bottom()
                                                    + Vec2::new(-167.0_f32, 3.0_f32),
                                            ));
                                        }

                                        ui.add_space(3.0_f32);
                                    }
                                });

                            let has_connections = self.vault.profiles.iter().any(|p| p.folder == folder);
                            header_res.header_response.context_menu(|ui| {
                                ui.set_min_width(140.0_f32);
                                if ui.button("Rename folder...").clicked() {
                                    rename_folder_req = Some(folder.clone());
                                    ui.close_menu();
                                }
                                if !has_connections {
                                    if ui.button(RichText::new("Delete folder").color(Color32::from_rgb(248, 113, 113))).clicked() {
                                        delete_folder_req = Some(folder.clone());
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    });

                if let Some(folder_name) = rename_folder_req {
                    self.folder_rename_buffer = folder_name.clone();
                    self.renaming_folder = Some(folder_name);
                }
                if let Some(folder_name) = delete_folder_req {
                    self.delete_folder(&folder_name);
                }

                if let Some(hid) = focus_host_tab_req {
                    self.open_or_focus_ssh_tab(&hid);
                }
                if let Some((hid, pos)) = menu_open_req {
                    self.host_menu_open = Some((hid, pos));
                }
                if let Some(hid) = connect_request_id {
                    self.initiate_connect(ctx, &hid, None, false);
                }

                // Bottom Google Sync Card
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let has_email = !self.vault.sync_email.trim().is_empty();
                    let email_display = if has_email {
                        self.vault.sync_email.as_str()
                    } else {
                        "Set Sync Email..."
                    };
                    let dot_col = if has_email {
                        accent_green
                    } else {
                        Color32::from_rgb(156, 163, 175)
                    };

                    egui::Frame::none()
                        .fill(google_card_bg)
                        .stroke(Stroke::new(1.0_f32, border_color))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(8.0_f32))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                draw_status_dot(ui, dot_col, 4.5_f32);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(email_display)
                                            .size(11.5_f32)
                                            .strong()
                                            .color(text_primary),
                                    );
                                    ui.label(
                                        RichText::new(&self.sync_status_text)
                                            .size(10.0_f32)
                                            .color(if has_email { accent_green } else { text_secondary }),
                                    );
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Sync").color(text_primary).size(11.5_f32),
                                            )
                                            .fill(btn_neutral_bg),
                                        )
                                        .on_hover_text("Configure sync email & encrypted .stvault backup")
                                        .clicked()
                                    {
                                        self.sync_email_input = self.vault.sync_email.clone();
                                        self.sync_feedback = None;
                                        self.show_sync_modal = true;
                                    }
                                });
                            });
                        });
                });
            });
        }

        // =========================================================================
        // FLOATING HOST CONTEXT MENU (shared by '...' button and right-click)
        // =========================================================================
        if let Some((menu_host_id, menu_pos)) = self.host_menu_open.clone() {
            let host = self
                .vault
                .profiles
                .iter()
                .find(|p| p.id == menu_host_id)
                .cloned();

            match host {
                None => self.host_menu_open = None,
                Some(host) => {
                    let is_connected = self.is_host_connected(&host.id);
                    let mut close_menu = false;

                    // Each live (or connecting) tab of this host, so a single one
                    // can be disconnected without dropping the rest.
                    let live_tabs: Vec<(String, String)> = self
                        .open_tabs
                        .iter()
                        .filter_map(|t| match &t.kind {
                            TabKind::Ssh { session_key, host_id }
                                if *host_id == host.id
                                    && (self.active_sessions.contains_key(session_key)
                                        || self.connecting_sessions.contains(session_key)) =>
                            {
                                Some((t.id.clone(), t.title.clone()))
                            }
                            _ => None,
                        })
                        .collect();
                    // The tab a single "Disconnect" acts on: the one on screen if
                    // it belongs to this host, otherwise the most recently opened.
                    let single_target: Option<String> = live_tabs
                        .iter()
                        .find(|(id, _)| self.active_tab_id.as_deref() == Some(id.as_str()))
                        .or_else(|| live_tabs.last())
                        .map(|(id, _)| id.clone());
                    let has_live = !live_tabs.is_empty();

                    // Keep the popup fully on-screen regardless of where it was opened.
                    let screen = ctx.screen_rect();
                    // Rough bounds used only to keep the popup on screen;
                    // the real width is measured from the labels below.
                    // Each extra disconnect row is 24px + spacing.
                    let extra_rows = if has_live { 2 } else { 0 };
                    let menu_size = Vec2::new(185.0_f32, 265.0_f32 + 25.0_f32 * extra_rows as f32);
                    let anchor = egui::Pos2::new(
                        menu_pos
                            .x
                            .clamp(screen.min.x + 4.0_f32, (screen.max.x - menu_size.x).max(screen.min.x + 4.0_f32)),
                        menu_pos
                            .y
                            .clamp(screen.min.y + 4.0_f32, (screen.max.y - menu_size.y).max(screen.min.y + 4.0_f32)),
                    );

                    let area = egui::Area::new(egui::Id::new("host_context_menu"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(anchor)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(card_bg)
                                .stroke(Stroke::new(1.0_f32, border_color))
                                .show(ui, |ui| {
                                    // Size to the longest label actually shown,
                                    // not to the unbounded width an Area offers.
                                    let mut labels: Vec<&str> = vec![
                                        "Connect",
                                        "Connect in new tab",
                                        "Edit...",
                                        "Duplicate",
                                        "Rename...",
                                        "Export Connection",
                                        "Delete",
                                    ];
                                    if has_live {
                                        labels.push("Disconnect");
                                        labels.push("Disconnect All Tabs");
                                    }
                                    let row_w = menu_width_for(ui, &labels);

                                    ui.set_width(row_w);
                                    ui.set_max_width(row_w);
                                    ui.spacing_mut().item_spacing.y = 1.0_f32;

                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Connect,
                                        "Connect",
                                        accent_green,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_connect_req = Some(host.id.clone());
                                        close_menu = true;
                                    }
                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::NewTab,
                                        "Connect in new tab",
                                        accent_blue,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_connect_new_tab_req = Some(host.id.clone());
                                        close_menu = true;
                                    }
                                    // "Disconnect" drops one tab of this host;
                                    // "Disconnect All Tabs" drops every one.
                                    if let Some(tab_id) = &single_target {
                                        if menu_item(
                                            ui,
                                            row_w,
                                            MenuIcon::Disconnect,
                                            "Disconnect",
                                            accent_amber,
                                            btn_neutral_bg,
                                        )
                                        .clicked()
                                        {
                                            menu_disconnect_tab_req = Some(tab_id.clone());
                                            close_menu = true;
                                        }
                                    }
                                    if has_live
                                        && menu_item(
                                            ui,
                                            row_w,
                                            MenuIcon::Disconnect,
                                            "Disconnect All Tabs",
                                            accent_amber,
                                            btn_neutral_bg,
                                        )
                                        .clicked()
                                    {
                                        menu_disconnect_req = Some(host.id.clone());
                                        close_menu = true;
                                    }
                                    let _ = is_connected;

                                    ui.add_space(3.0_f32);
                                    ui.separator();
                                    ui.add_space(3.0_f32);

                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Edit,
                                        "Edit...",
                                        text_primary,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_edit_req = Some(host.clone());
                                        close_menu = true;
                                    }
                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Duplicate,
                                        "Duplicate",
                                        text_primary,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_duplicate_req = Some(host.id.clone());
                                        close_menu = true;
                                    }
                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Rename,
                                        "Rename...",
                                        text_primary,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_rename_req =
                                            Some((host.id.clone(), host.name.clone()));
                                        close_menu = true;
                                    }
                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Export,
                                        "Export Connection",
                                        text_primary,
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_export_req = Some(host.clone());
                                        close_menu = true;
                                    }

                                    ui.add_space(3.0_f32);
                                    ui.separator();
                                    ui.add_space(3.0_f32);

                                    if menu_item(
                                        ui,
                                        row_w,
                                        MenuIcon::Delete,
                                        "Delete",
                                        Color32::from_rgb(248, 113, 113),
                                        btn_neutral_bg,
                                    )
                                    .clicked()
                                    {
                                        menu_delete_req =
                                            Some((host.id.clone(), host.name.clone()));
                                        close_menu = true;
                                    }
                                });
                        });

                    // Close when clicking anywhere outside the popup, or on Escape.
                    let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
                        && !area.response.rect.contains(
                            ctx.input(|i| i.pointer.interact_pos())
                                .unwrap_or(egui::Pos2::new(-1.0, -1.0)),
                        );
                    if close_menu || clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.host_menu_open = None;
                    }
                }
            }
        }

        if let Some(hid) = menu_connect_req {
            self.initiate_connect(ctx, &hid, None, false);
        }
        if let Some(hid) = menu_connect_new_tab_req {
            self.initiate_connect(ctx, &hid, None, true);
        }
        if let Some(hid) = menu_disconnect_req {
            self.close_all_tabs_for_host(&hid);
        }
        if let Some(tid) = menu_disconnect_tab_req {
            // Same teardown as "Disconnect All Tabs", for one tab only. The
            // host's tunnels stop only once its last tab is gone.
            self.close_tab(&tid);
        }
        if let Some(host) = menu_edit_req {
            self.load_host_into_editor(&host);
        }
        if let Some(dup_id) = menu_duplicate_req {
            if let Some(orig) = self.vault.profiles.iter().find(|p| p.id == dup_id).cloned() {
                let new_id = uuid::Uuid::new_v4().to_string();
                let mut dup = orig;
                dup.id = new_id.clone();
                dup.name = format!("{} (Copy)", dup.name);
                for t in &mut dup.tunnels {
                    t.id = uuid::Uuid::new_v4().to_string();
                    t.is_active = false;
                }
                self.vault.profiles.push(dup);
                let _ = save_vault(&self.vault);
                self.selected_host_id = new_id;
            }
        }
        if let Some(req) = menu_rename_req {
            self.rename_host_modal = Some(req);
        }
        if let Some(prof) = menu_export_req {
            if let Ok(json_str) = serde_json::to_string_pretty(&prof) {
                self.export_host_json_modal = Some((prof.name, json_str));
            }
        }
        if let Some(req) = menu_delete_req {
            self.confirm_delete_host = Some(req);
        }

        // =========================================================================
        // RIGHT PANEL: LIVE SSH TUNNELS DRAWER (REAL PORT FORWARDING)
        // =========================================================================
        if self.show_tunnel_drawer {
            egui::SidePanel::right("tunnel_panel")
                .frame(
                    egui::Frame::none()
                        .fill(sidebar_bg)
                        .stroke(border_stroke)
                        .inner_margin(egui::Margin::same(12.0_f32)),
                )
                .default_width(325.0_f32)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Port Forwarding & Proxy")
                                .size(14.0_f32)
                                .strong()
                                .color(text_primary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("X").color(text_secondary)).clicked() {
                                self.show_tunnel_drawer = false;
                            }
                        });
                    });

                    ui.add_space(6.0_f32);

                    if ui
                        .add_sized(
                            [ui.available_width(), 26.0_f32],
                            egui::Button::new(
                                RichText::new("+ Add Tunnel to Selected Host")
                                    .color(Color32::WHITE)
                                    .size(12.0_f32)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(37, 99, 235)),
                        )
                        .clicked()
                    {
                        self.editing_tunnel_id = None;
                        self.new_tunnel_name.clear();
                        self.new_tunnel_type = "local".to_string();
                        self.new_tunnel_bind_host = "127.0.0.1".to_string();
                        self.new_tunnel_bind_port = "3307".to_string();
                        self.new_tunnel_remote_host = "127.0.0.1".to_string();
                        self.new_tunnel_remote_port = "3306".to_string();
                        self.new_tunnel_auto_start = true;
                        self.show_add_tunnel_modal = true;
                    }

                    ui.add_space(6.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.filter_tunnels_selected_host, "Selected Host")
                            .clicked()
                        {
                            self.filter_tunnels_selected_host = true;
                        }
                        if ui
                            .selectable_label(!self.filter_tunnels_selected_host, "All Hosts")
                            .clicked()
                        {
                            self.filter_tunnels_selected_host = false;
                        }
                    });
                    ui.separator();
                    ui.add_space(6.0_f32);

                    let mut start_tun_req: Option<(String, TunnelConfig)> = None;
                    let mut stop_tun_req: Option<(String, String)> = None;
                    let mut edit_tun_req: Option<(String, TunnelConfig)> = None;
                    let mut del_tun_req: Option<(String, String, String)> = None;

                    ScrollArea::vertical().show(ui, |ui| {
                        for profile in &self.vault.profiles {
                            if self.filter_tunnels_selected_host && profile.id != self.selected_host_id {
                                continue;
                            }
                            let host_connected = self.is_host_connected(&profile.id);
                            for tun in &profile.tunnels {
                                let is_running_live = self.active_tunnels.contains_key(&tun.id);
                                let live_bytes = self
                                    .active_tunnels
                                    .get(&tun.id)
                                    .map(|h| h.bytes_transferred())
                                    .unwrap_or(0);

                                egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(border_stroke)
                                    .rounding(Rounding::same(6.0_f32))
                                    .inner_margin(egui::Margin::same(10.0_f32))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let (badge, badge_col) = match tun.tunnel_type.as_str() {
                                                "dynamic" => ("SOCKS5", accent_amber),
                                                "remote" => ("REMOTE", Color32::from_rgb(192, 132, 252)),
                                                _ => ("LOCAL", accent_blue),
                                            };
                                            ui.label(
                                                RichText::new(format!("[{}]", badge))
                                                    .size(10.5_f32)
                                                    .strong()
                                                    .color(badge_col),
                                            );
                                            ui.label(
                                                RichText::new(&tun.name)
                                                    .color(text_primary)
                                                    .strong(),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.menu_button(
                                                        RichText::new("...")
                                                            .color(text_secondary)
                                                            .size(13.0_f32)
                                                            .strong(),
                                                        |ui| {
                                                            ui.set_min_width(120.0_f32);
                                                            if ui
                                                                .button(RichText::new("Edit Tunnel").color(text_primary))
                                                                .clicked()
                                                            {
                                                                edit_tun_req = Some((profile.id.clone(), tun.clone()));
                                                                ui.close_menu();
                                                            }
                                                            ui.separator();
                                                            if ui
                                                                .button(RichText::new("Delete Tunnel").color(Color32::from_rgb(248, 113, 113)))
                                                                .clicked()
                                                            {
                                                                del_tun_req = Some((
                                                                    profile.id.clone(),
                                                                    tun.id.clone(),
                                                                    tun.name.clone(),
                                                                ));
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    );
                                                }
                                            );
                                        });

                                        ui.label(
                                            RichText::new(format!(
                                                "Host: {} ({}){}",
                                                profile.name,
                                                if host_connected { "Online" } else { "Offline" },
                                                if tun.auto_start { "  -  Auto-ON" } else { "" }
                                            ))
                                            .size(10.0_f32)
                                            .color(if host_connected {
                                                accent_green
                                            } else {
                                                text_secondary
                                            }),
                                        );

                                        let route_desc = if tun.tunnel_type == "dynamic" {
                                            format!("SOCKS5 Proxy on {}:{}", tun.bind_host, tun.bind_port)
                                        } else {
                                            format!(
                                                "{}:{} -> {}:{}",
                                                tun.bind_host,
                                                tun.bind_port,
                                                tun.remote_host.as_deref().unwrap_or("?"),
                                                tun.remote_port.unwrap_or(0)
                                            )
                                        };
                                        ui.label(
                                            RichText::new(route_desc)
                                                .monospace()
                                                .size(11.0_f32)
                                                .color(text_secondary),
                                        );

                                        ui.add_space(4.0_f32);
                                        ui.horizontal(|ui| {
                                            let status_color = if is_running_live {
                                                accent_green
                                            } else {
                                                Color32::from_rgb(107, 114, 128)
                                            };
                                            draw_status_dot(ui, status_color, 3.5_f32);
                                            ui.label(
                                                RichText::new(if is_running_live {
                                                    format!("Live ({:.1} KB)", live_bytes as f64 / 1024.0)
                                                } else {
                                                    "Stopped".to_string()
                                                })
                                                .color(status_color)
                                                .size(11.0_f32),
                                            );

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let btn_fill = if is_running_live {
                                                        Color32::from_rgb(185, 28, 28)
                                                    } else if host_connected {
                                                        Color32::from_rgb(22, 163, 74)
                                                    } else {
                                                        Color32::from_rgb(55, 65, 81)
                                                    };
                                                    if ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new(if is_running_live { "Stop" } else { "Start" })
                                                                    .color(Color32::WHITE)
                                                                    .size(11.0_f32),
                                                            )
                                                            .fill(btn_fill),
                                                        )
                                                        .on_hover_text(if host_connected {
                                                            "Start / Stop port forwarding"
                                                        } else {
                                                            "Connect the SSH host first to start this tunnel"
                                                        })
                                                        .clicked()
                                                    {
                                                        if is_running_live {
                                                            stop_tun_req = Some((profile.id.clone(), tun.id.clone()));
                                                        } else {
                                                            start_tun_req = Some((profile.id.clone(), tun.clone()));
                                                        }
                                                    }
                                                }
                                            );
                                        });
                                    });
                                ui.add_space(6.0_f32);
                            }
                        }
                    });

                    if let Some((host_id, tun)) = start_tun_req {
                        let sess_opt = self.host_tunnel_sessions.get(&host_id).cloned().or_else(|| {
                            self.active_sessions
                                .values()
                                .find(|s| s.host_id == host_id)
                                .cloned()
                        });
                        if let Some(sess) = sess_opt {
                            self.host_tunnel_sessions.insert(host_id.clone(), sess.clone());
                            let handle = spawn_tunnel(
                                &self.rt,
                                sess,
                                tun.clone(),
                                self.tx_events.clone(),
                                ctx.clone(),
                            );
                            self.active_tunnels.insert(tun.id.clone(), handle);
                            if let Some(p) = self.vault.profiles.iter_mut().find(|p| p.id == host_id) {
                                if let Some(t) = p.tunnels.iter_mut().find(|t| t.id == tun.id) {
                                    t.is_active = true;
                                }
                            }
                            let _ = save_vault(&self.vault);
                        } else {
                            let target_hid = self.selected_host_id.clone();
                            self.push_log_for_host(
                                &target_hid,
                                format!("[!] Please connect SSH to the host for tunnel '{}' first.", tun.name),
                            );
                        }
                    }

                    if let Some((host_id, tun_id)) = stop_tun_req {
                        if let Some(handle) = self.active_tunnels.remove(&tun_id) {
                            handle.stop();
                        }
                        if let Some(p) = self.vault.profiles.iter_mut().find(|p| p.id == host_id) {
                            if let Some(t) = p.tunnels.iter_mut().find(|t| t.id == tun_id) {
                                t.is_active = false;
                            }
                        }
                        let _ = save_vault(&self.vault);
                    }

                    if let Some((host_id, tun)) = edit_tun_req {
                        self.editing_tunnel_id = Some((host_id, tun.id));
                        self.new_tunnel_name = tun.name;
                        self.new_tunnel_type = tun.tunnel_type;
                        self.new_tunnel_bind_host = tun.bind_host;
                        self.new_tunnel_bind_port = tun.bind_port.to_string();
                        self.new_tunnel_remote_host = tun.remote_host.unwrap_or_else(|| "127.0.0.1".to_string());
                        self.new_tunnel_remote_port = tun.remote_port.map(|p| p.to_string()).unwrap_or_else(|| "3306".to_string());
                        self.new_tunnel_auto_start = tun.auto_start;
                        self.show_add_tunnel_modal = true;
                    }

                    if let Some(del_info) = del_tun_req {
                        self.confirm_delete_tunnel = Some(del_info);
                    }
                });
        }

        // =========================================================================
        // WORKSPACE TAB BAR (MULTI-SESSION SSH & TOOL TABS)
        // =========================================================================
        let mut switch_tab_req: Option<String> = None;
        let mut close_tab_req: Option<String> = None;

        egui::TopBottomPanel::top("workspace_tabs")
            .frame(
                egui::Frame::none()
                    .fill(header_bg)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::symmetric(8.0_f32, 4.0_f32)),
            )
            .show(ctx, |ui| {
                ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for tab in &self.open_tabs {
                                let is_active = self.active_tab_id.as_deref() == Some(tab.id.as_str());
                                let tab_fill = if is_active {
                                    card_selected_bg
                                } else {
                                    card_bg
                                };
                                let tab_stroke = if is_active {
                                    Stroke::new(1.2_f32, accent_purple)
                                } else {
                                    Stroke::new(1.0_f32, border_color)
                                };

                                egui::Frame::none()
                                    .fill(tab_fill)
                                    .stroke(tab_stroke)
                                    .rounding(Rounding::same(5.0_f32))
                                    .inner_margin(egui::Margin::symmetric(8.0_f32, 3.0_f32))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            match &tab.kind {
                                                TabKind::Ssh { session_key, .. } => {
                                                    let dot = if self.active_sessions.contains_key(session_key) {
                                                        accent_green
                                                    } else if self.connecting_sessions.contains(session_key) {
                                                        accent_amber
                                                    } else {
                                                        Color32::from_rgb(107, 114, 128)
                                                    };
                                                    draw_status_dot(ui, dot, 3.0_f32);
                                                }
                                                TabKind::Shell { session_key, .. } => {
                                                    let dot = if self.shell_sessions.contains_key(session_key) {
                                                        accent_green
                                                    } else {
                                                        Color32::from_rgb(107, 114, 128)
                                                    };
                                                    draw_status_dot(ui, dot, 3.0_f32);
                                                }
                                                TabKind::Sql { .. } => {
                                                    draw_tool_icon(
                                                        ui,
                                                        &ActiveTool::Sql,
                                                        if is_active { accent_blue } else { text_secondary },
                                                    );
                                                }
                                                TabKind::Tool(t) => {
                                                    draw_tool_icon(ui, t, if is_active { accent_blue } else { text_secondary });
                                                }
                                            }

                                            let lbl = ui.add(
                                                egui::Label::new(
                                                    RichText::new(&tab.title)
                                                        .size(12.0_f32)
                                                        .color(if is_active { text_primary } else { text_secondary })
                                                        .strong(),
                                                )
                                                .selectable(false)
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if lbl.clicked() {
                                                switch_tab_req = Some(tab.id.clone());
                                            }
                                            if lbl.middle_clicked() {
                                                close_tab_req = Some(tab.id.clone());
                                            }

                                            ui.add_space(4.0_f32);
                                            let close_btn = ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("x")
                                                            .size(11.0_f32)
                                                            .color(text_secondary),
                                                    )
                                                    .frame(false)
                                                    .min_size(Vec2::new(16.0_f32, 16.0_f32)),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text("Close tab");
                                            if close_btn.clicked() {
                                                close_tab_req = Some(tab.id.clone());
                                            }
                                        });
                                    });
                            }

                            // '+' Quick New Tab Button
                            ui.menu_button(
                                RichText::new(" + ").size(13.0_f32).strong().color(text_primary),
                                |ui| {
                                    ui.set_min_width(170.0_f32);
                                    ui.label(RichText::new("Open SSH Host Tab").size(11.0_f32).color(text_secondary));
                                    for p in &self.vault.profiles {
                                        if ui.button(&p.name).clicked() {
                                            switch_tab_req = Some(format!("open_ssh:{}", p.id));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Open Local Shell").size(11.0_f32).color(text_secondary));
                                    for kind in ShellKind::all() {
                                        if ui.button(kind.title()).clicked() {
                                            switch_tab_req = Some(format!("open_shell:{}", kind.title()));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Open SQL Client").size(11.0_f32).color(text_secondary));
                                    if ui.button("New blank session").clicked() {
                                        switch_tab_req = Some("open_sql:".to_string());
                                        ui.close_menu();
                                    }
                                    for cfg in &self.vault.sql_connections {
                                        if ui
                                            .button(format!("{}  [{}]", cfg.name, cfg.db_type))
                                            .clicked()
                                        {
                                            switch_tab_req = Some(format!("open_sql:{}", cfg.id));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    ui.label(RichText::new("Open Tool Tab").size(11.0_f32).color(text_secondary));
                                    for tool in ActiveTool::all() {
                                        // SQL Client has its own section above.
                                        if *tool == ActiveTool::Sql {
                                            continue;
                                        }
                                        if ui.button(tool.title()).clicked() {
                                            switch_tab_req = Some(format!("open_tool:{}", tool.title()));
                                            ui.close_menu();
                                        }
                                    }
                                }
                            );
                        });
                    });
            });

        if let Some(req) = switch_tab_req {
            if let Some(hid) = req.strip_prefix("open_ssh:") {
                self.initiate_connect(ctx, hid, None, true);
            } else if let Some(tname) = req.strip_prefix("open_tool:") {
                if let Some(t) = ActiveTool::all().iter().find(|t| t.title() == tname) {
                    self.open_or_focus_tool_tab(t.clone());
                }
            } else if let Some(sname) = req.strip_prefix("open_shell:") {
                // The '+' menu explicitly means "give me another one".
                if let Some(kind) = ShellKind::all().into_iter().find(|k| k.title() == sname) {
                    if self.running_elevated {
                        self.open_shell_tab(ctx, kind);
                    } else {
                        self.pending_shell_elevation = Some((kind, true));
                    }
                }
            } else if let Some(cfg_id) = req.strip_prefix("open_sql:") {
                // Always a new session, matching the shell behaviour above.
                let preset = if cfg_id.is_empty() {
                    None
                } else {
                    self.vault
                        .sql_connections
                        .iter()
                        .find(|c| c.id == cfg_id)
                        .cloned()
                };
                self.open_sql_tab(preset);
            } else {
                self.active_tab_id = Some(req.clone());
                if let Some(TabKind::Ssh { host_id, .. }) = self.open_tabs.iter().find(|t| t.id == req).map(|t| t.kind.clone()) {
                    self.selected_host_id = host_id;
                }
            }
        }
        if let Some(cid) = close_tab_req {
            self.close_tab(&cid);
        }

        // =========================================================================
        // CENTRAL PANEL: ACTIVE TAB WORKSPACE (SSH TERMINAL OR TOOL)
        // =========================================================================
        let any_modal_open = self.password_prompt_host_id.is_some()
            || self.confirm_delete_host.is_some()
            || self.confirm_delete_tunnel.is_some()
            || self.rename_host_modal.is_some()
            || self.renaming_folder.is_some()
            || self.tools_state.note_confirm_delete.is_some()
            || self.export_host_json_modal.is_some()
            || self.pending_shell_elevation.is_some()
            || self.show_add_host_modal
            || self.show_add_tunnel_modal
            || self.show_settings_modal
            || self.show_sync_modal;

        match self.active_tab_kind() {
            Some(TabKind::Tool(active_tool)) => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(sidebar_bg)
                            .inner_margin(egui::Margin::same(16.0_f32)),
                    )
                    .show(ctx, |ui| {
                        let mut back_to_term = false;
                        render_tool_workspace(
                            ui,
                            ctx,
                            &self.rt,
                            &mut self.tools_state,
                            &active_tool,
                            &self.vault,
                            text_primary,
                            text_secondary,
                            card_bg,
                            border_color,
                            btn_neutral_bg,
                            accent_blue,
                            accent_green,
                            accent_purple,
                            &mut back_to_term,
                        );
                        if back_to_term {
                            let hid = self.selected_host_id.clone();
                            if !hid.is_empty() {
                                self.open_or_focus_ssh_tab(&hid);
                            }
                            self.sidebar_tab = SidebarTab::Hosts;
                        }
                    });
            }
            Some(TabKind::Sql { session_key }) => {
                // Tabs cannot be added while the panel borrows `self`.
                let mut sql_open_req = false;
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::none()
                            .fill(sidebar_bg)
                            .inner_margin(egui::Margin::same(16.0_f32)),
                    )
                    .show(ctx, |ui| {
                        // Take the session out so `self` stays free for the
                        // shared ToolsState and vault borrows below.
                        let Some(mut sess) = self.sql_sessions.remove(&session_key) else {
                            return;
                        };

                        let mut new_session_req = false;
                        render_sql_client(
                            ui,
                            ctx,
                            &self.rt,
                            &mut sess,
                            &mut self.tools_state,
                            &self.vault,
                            text_primary,
                            text_secondary,
                            card_bg,
                            Stroke::new(1.0_f32, border_color),
                            btn_neutral_bg,
                            accent_blue,
                            accent_green,
                            accent_purple,
                            &mut new_session_req,
                            &session_key,
                        );
                        if new_session_req {
                            sql_open_req = true;
                        }

                        // Keep the workspace tab caption in step with the
                        // connection the session now points at.
                        let want = format!("SQL - {}", sess.tab_title());
                        if let Some(tab) = self.open_tabs.iter_mut().find(|t| t.id == session_key) {
                            if tab.title != want {
                                tab.title = want;
                            }
                        }

                        self.sql_sessions.insert(session_key.clone(), sess);
                    });

                if sql_open_req {
                    self.open_sql_tab(None);
                }
            }
            Some(TabKind::Ssh { session_key, .. }) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(terminal_bg))
                    .show(ctx, |ui| {
                        let emu = self
                            .terminals
                            .entry(session_key.clone())
                            .or_insert_with(|| TerminalEmulator::new(36, 120));

                        let tab_connected = self.active_sessions.contains_key(&session_key);
                        let actions = render_terminal_viewport(
                            ui,
                            ctx,
                            emu,
                            self.font_size,
                            terminal_bg,
                            terminal_default_fg,
                            accent_purple,
                            is_light_terminal,
                            tab_connected,
                            any_modal_open,
                        );
                        self.last_terminal_size = (emu.cols, emu.rows);

                        if let Some(sess) = self.active_sessions.get(&session_key) {
                            if !actions.bytes_to_send.is_empty() {
                                sess.send_bytes(actions.bytes_to_send);
                            }
                            if let Some((new_cols, new_rows)) = actions.resized_to {
                                sess.send_resize(new_cols as u32, new_rows as u32);
                            }
                        }
                    });
            }
            Some(TabKind::Shell { session_key, .. }) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(terminal_bg))
                    .show(ctx, |ui| {
                        let emu = self
                            .terminals
                            .entry(session_key.clone())
                            .or_insert_with(|| TerminalEmulator::new(36, 120));

                        let shell_alive = self.shell_sessions.contains_key(&session_key);
                        let actions = render_terminal_viewport(
                            ui,
                            ctx,
                            emu,
                            self.font_size,
                            terminal_bg,
                            terminal_default_fg,
                            accent_purple,
                            is_light_terminal,
                            shell_alive,
                            any_modal_open,
                        );
                        self.last_terminal_size = (emu.cols, emu.rows);

                        if let Some(sess) = self.shell_sessions.get(&session_key) {
                            if !actions.bytes_to_send.is_empty() {
                                sess.send_bytes(actions.bytes_to_send);
                            }
                            if let Some((new_cols, new_rows)) = actions.resized_to {
                                sess.send_resize(new_cols, new_rows);
                            }
                        }
                    });
            }
            None => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(terminal_bg))
                    .show(ctx, |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("No open tab. Select a Host or Tool from the sidebar, or click '+' above.")
                                    .size(13.0_f32)
                                    .color(text_secondary),
                            );
                        });
                    });
            }
        }

        // =========================================================================
        // MODAL: PASSWORD PROMPT ON CONNECT
        // =========================================================================
        if let Some(prompt_host_id) = self.password_prompt_host_id.clone() {
            let host_info = self
                .vault
                .profiles
                .iter()
                .find(|p| p.id == prompt_host_id)
                .map(|p| format!("{}@{}:{}", p.username, p.host, p.port))
                .unwrap_or_default();

            egui::Window::new("SSH Authentication")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(360.0_f32, 180.0_f32))
                .show(ctx, |ui| {
                    ui.label(RichText::new(format!("Enter SSH password for {}", host_info)).color(text_primary));
                    ui.add_space(6.0_f32);

                    let (pwd_res, _) = MenuEdit::single("password_prompt_input")
                        .hint("SSH Password...")
                        .width(f32::INFINITY)
                        .password()
                        .show_response(ui, &mut self.password_prompt_input);
                    pwd_res.request_focus();

                    ui.checkbox(&mut self.password_prompt_save, "Save password in local vault");
                    ui.add_space(10.0_f32);

                    let submit_pressed = pwd_res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    let mut do_connect = submit_pressed;

                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Connect").color(Color32::WHITE))
                                    .fill(Color32::from_rgb(22, 163, 74)),
                            )
                            .clicked()
                        {
                            do_connect = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.password_prompt_host_id = None;
                            self.password_prompt_input.clear();
                        }
                    });

                    if do_connect {
                        let pwd = self.password_prompt_input.clone();
                        if self.password_prompt_save {
                            if let Some(p) = self.vault.profiles.iter_mut().find(|p| p.id == prompt_host_id) {
                                p.password = Some(pwd.clone());
                            }
                            let _ = save_vault(&self.vault);
                        }
                        self.password_prompt_host_id = None;
                        self.password_prompt_input.clear();
                        let new_tab = self.password_prompt_new_tab;
                        self.password_prompt_new_tab = false;
                        self.initiate_connect(ctx, &prompt_host_id, Some(pwd), new_tab);
                    }
                });
        }

        // =========================================================================
        // MODAL: RENAME HOST
        // =========================================================================
        if let Some((ren_id, mut ren_name)) = self.rename_host_modal.clone() {
            let mut keep_open = true;
            egui::Window::new("Rename Host")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(340.0_f32, 120.0_f32))
                .show(ctx, |ui| {
                    ui.label(RichText::new("New Host Name:").color(text_primary));
                    let (res, _) = MenuEdit::single("ren_name")
                        .width(f32::INFINITY)
                        .show_response(ui, &mut ren_name);
                    res.request_focus();
                    let enter_save = res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));

                    ui.add_space(10.0_f32);
                    ui.horizontal(|ui| {
                        if (ui
                            .add(egui::Button::new(RichText::new("Rename").color(Color32::WHITE)).fill(Color32::from_rgb(124, 58, 237)))
                            .clicked()
                            || enter_save)
                            && !ren_name.trim().is_empty()
                        {
                            let clean = ren_name.trim().to_string();
                            if let Some(p) = self.vault.profiles.iter_mut().find(|p| p.id == ren_id) {
                                p.name = clean.clone();
                            }
                            for tab in &mut self.open_tabs {
                                if let TabKind::Ssh { host_id, .. } = &tab.kind {
                                    if host_id == &ren_id {
                                        if let Some(suffix_idx) = tab.title.rfind(" #") {
                                            tab.title = format!("{}{}", clean, &tab.title[suffix_idx..]);
                                        } else {
                                            tab.title = clean.clone();
                                        }
                                    }
                                }
                            }
                            let _ = save_vault(&self.vault);
                            keep_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep_open = false;
                        }
                    });
                });
            if keep_open {
                self.rename_host_modal = Some((ren_id, ren_name));
            } else {
                self.rename_host_modal = None;
            }
        }

        // =========================================================================
        // MODAL: EXPORT HOST (JSON)
        // =========================================================================
        if let Some((exp_name, mut exp_json)) = self.export_host_json_modal.clone() {
            let mut keep_open = true;
            egui::Window::new(format!("Export Connection - {}", exp_name))
                .id(egui::Id::new("export_connection_window"))
                .collapsible(false)
                .resizable(true)
                .default_size(Vec2::new(460.0_f32, 340.0_f32))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Host Configuration & Tunnels JSON:").color(text_secondary));
                    ScrollArea::vertical()
                        .max_height(220.0_f32)
                        .show(ui, |ui| {
                            MenuEdit::multi("exp_json", 6)
    .width(f32::INFINITY)
    .monospace()
    .show(ui, &mut exp_json);
                        });
                    ui.add_space(8.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("Copy JSON").color(Color32::WHITE)).fill(Color32::from_rgb(37, 99, 235)))
                            .clicked()
                        {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(exp_json.clone());
                            }
                        }
                        if ui.button("Close").clicked() {
                            keep_open = false;
                        }
                    });
                });
            if !keep_open {
                self.export_host_json_modal = None;
            }
        }

        // =========================================================================
        // MODAL: RUN LOCAL SHELL ELEVATED?
        // =========================================================================
        if let Some((pending_kind, force_new)) = self.pending_shell_elevation {
            let (role, note) = if cfg!(windows) {
                (
                    "Administrator",
                    "Windows cannot grant Administrator rights to a running program, so Superterm must restart. Open sessions will be closed and Windows will show a UAC prompt.",
                )
            } else {
                (
                    "root",
                    "Superterm must restart to gain root rights. Open sessions will be closed and your system will ask for authentication.",
                )
            };

            let mut choice: Option<bool> = None; // Some(true) = elevated
            let mut dismissed = false;

            egui::Window::new(format!("Run {} as {}?", pending_kind.title(), role))
                .id(egui::Id::new("shell_elevation_window"))
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(430.0_f32, 190.0_f32))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Start {} with elevated privileges?",
                            pending_kind.title()
                        ))
                        .color(text_primary)
                        .size(13.0_f32),
                    );
                    ui.add_space(8.0_f32);
                    ui.label(RichText::new(note).color(text_secondary).size(11.5_f32));

                    ui.add_space(14.0_f32);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("Yes, restart as {}", role))
                                        .color(Color32::WHITE)
                                        .size(12.0_f32),
                                )
                                .fill(Color32::from_rgb(185, 28, 28))
                                .min_size(Vec2::new(150.0_f32, 26.0_f32)),
                            )
                            .clicked()
                        {
                            choice = Some(true);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("No, run normally")
                                        .color(Color32::WHITE)
                                        .size(12.0_f32),
                                )
                                .fill(Color32::from_rgb(37, 99, 235))
                                .min_size(Vec2::new(130.0_f32, 26.0_f32)),
                            )
                            .clicked()
                        {
                            choice = Some(false);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancel").color(text_primary).size(12.0_f32),
                                )
                                .fill(btn_neutral_bg)
                                .min_size(Vec2::new(70.0_f32, 26.0_f32)),
                            )
                            .clicked()
                        {
                            dismissed = true;
                        }
                    });
                });

            match choice {
                Some(true) => {
                    self.pending_shell_elevation = None;
                    match relaunch_elevated() {
                        Ok(()) => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(e) => {
                            // Elevation failed or was declined: fall back to a
                            // normal shell rather than leaving the user stuck.
                            self.open_shell_tab(ctx, pending_kind);
                            if let Some(emu) = self
                                .active_tab_id
                                .clone()
                                .and_then(|k| self.terminals.get_mut(&k))
                            {
                                emu.writeln(&format!(
                                    "\x1b[33m[!] {} Running without elevation.\x1b[0m",
                                    e
                                ));
                            }
                        }
                    }
                }
                Some(false) => {
                    self.pending_shell_elevation = None;
                    if force_new {
                        self.open_shell_tab(ctx, pending_kind);
                    } else {
                        self.open_or_focus_shell_tab(ctx, pending_kind);
                    }
                }
                None => {
                    if dismissed || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.pending_shell_elevation = None;
                    }
                }
            }
        }

        // =========================================================================
        // MODAL: CONFIRM DELETE NOTE
        // =========================================================================
        if let Some((note_id, note_title)) = self.tools_state.note_confirm_delete.clone() {
            let mut keep_open = true;
            egui::Window::new("Delete Note")
                .id(egui::Id::new("note_delete_window"))
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(340.0_f32, 130.0_f32))
                .show(ctx, |ui| {
                    let shown = if note_title.trim().is_empty() {
                        "(untitled)".to_string()
                    } else {
                        note_title.clone()
                    };
                    ui.label(
                        RichText::new(format!("Delete the note '{}'?", shown)).color(text_primary),
                    );
                    ui.label(
                        RichText::new("This cannot be undone.")
                            .size(11.0_f32)
                            .color(text_secondary),
                    );
                    ui.add_space(12.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                                    .fill(Color32::from_rgb(185, 28, 28)),
                            )
                            .clicked()
                        {
                            self.tools_state.note_delete_req = Some(note_id.clone());
                            keep_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep_open = false;
                        }
                    });
                });
            if !keep_open {
                self.tools_state.note_confirm_delete = None;
            }
        }

        // =========================================================================
        // MODAL: RENAME FOLDER
        // =========================================================================
        if let Some(folder_name) = self.renaming_folder.clone() {
            let mut keep_open = true;
            egui::Window::new("Rename Folder")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(340.0_f32, 120.0_f32))
                .show(ctx, |ui| {
                    ui.label(RichText::new("New Folder Name:").color(text_primary));
                    let (res, _) = MenuEdit::single("folder_rename_buffer")
                        .width(f32::INFINITY)
                        .show_response(ui, &mut self.folder_rename_buffer);
                    res.request_focus();
                    let enter_save = res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));

                    ui.add_space(10.0_f32);
                    ui.horizontal(|ui| {
                        if (ui
                            .add(egui::Button::new(RichText::new("Rename").color(Color32::WHITE)).fill(Color32::from_rgb(124, 58, 237)))
                            .clicked()
                            || enter_save)
                            && !self.folder_rename_buffer.trim().is_empty()
                        {
                            let clean = self.folder_rename_buffer.trim().to_string();
                            self.rename_folder(&folder_name, &clean);
                            keep_open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep_open = false;
                        }
                    });
                });
            if !keep_open {
                self.renaming_folder = None;
            }
        }

        // =========================================================================
        // MODAL: CONFIRM DELETE HOST
        // =========================================================================
        if let Some((del_id, del_name)) = self.confirm_delete_host.clone() {
            egui::Window::new("Confirm Delete Host")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(340.0_f32, 140.0_f32))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("Are you sure you want to delete host '{}' and all of its tunnels?", del_name))
                            .color(text_primary),
                    );
                    ui.add_space(12.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                                    .fill(Color32::from_rgb(185, 28, 28)),
                            )
                            .clicked()
                        {
                            self.close_all_tabs_for_host(&del_id);
                            self.vault.profiles.retain(|p| p.id != del_id);
                            if self.selected_host_id == del_id {
                                self.selected_host_id = self
                                    .vault
                                    .profiles
                                    .first()
                                    .map(|p| p.id.clone())
                                    .unwrap_or_default();
                            }
                            let _ = save_vault(&self.vault);
                            self.confirm_delete_host = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete_host = None;
                        }
                    });
                });
        }

        // =========================================================================
        // MODAL: CONFIRM DELETE TUNNEL
        // =========================================================================
        if let Some((host_id, tun_id, tun_name)) = self.confirm_delete_tunnel.clone() {
            egui::Window::new("Confirm Delete Tunnel")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(340.0_f32, 130.0_f32))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("Are you sure you want to delete tunnel '{}'?", tun_name))
                            .color(text_primary),
                    );
                    ui.add_space(12.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                                    .fill(Color32::from_rgb(185, 28, 28)),
                            )
                            .clicked()
                        {
                            if let Some(h) = self.active_tunnels.remove(&tun_id) {
                                h.stop();
                            }
                            if let Some(profile) = self.vault.profiles.iter_mut().find(|p| p.id == host_id) {
                                profile.tunnels.retain(|t| t.id != tun_id);
                                let _ = save_vault(&self.vault);
                            }
                            self.confirm_delete_tunnel = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete_tunnel = None;
                        }
                    });
                });
        }

        // =========================================================================
        // MODAL: ADD / EDIT HOST
        // =========================================================================
        if self.show_add_host_modal {
            let title = if self.editing_host_id.is_some() {
                format!("Edit session - {}", self.new_host_name)
            } else {
                "New session".to_string()
            };
            let mut save_clicked = false;
            let mut cancel_clicked = false;

            // The dialog is deliberately fixed-size. A resizable window measures
            // its content to decide its width, and this form asks for
            // `f32::INFINITY` widths, so the two would feed each other and the
            // window would creep wider on every frame.
            const DIALOG_W: f32 = 560.0_f32;
            const DIALOG_H: f32 = 560.0_f32;
            const CONTENT_W: f32 = DIALOG_W - 24.0_f32;

            egui::Window::new(title)
                // Stable id: the title contains the host name being edited,
                // and egui derives a window's id from its title by default.
                // Without this, every keystroke in "Name" created a "new"
                // window, which egui re-positioned - the form jumped around.
                .id(egui::Id::new("host_editor_window"))
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(DIALOG_W, DIALOG_H))
                .show(ctx, |ui| {
                    // Pin the content width so no child can widen the dialog.
                    ui.set_width(CONTENT_W);
                    ui.set_max_width(CONTENT_W);

                    // ---------------- Tab strip ----------------
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0_f32;
                        for tab in HostEditorTab::all() {
                            let selected = self.host_editor_tab == tab;
                            let color = if selected { accent_blue } else { text_secondary };

                            let resp = ui.add(
                                egui::Button::new(
                                    RichText::new(tab.title()).color(color).size(12.5_f32),
                                )
                                .frame(false)
                                .min_size(Vec2::new(0.0_f32, 22.0_f32)),
                            );

                            // Active-tab underline.
                            if selected {
                                let r = resp.rect;
                                ui.painter().line_segment(
                                    [
                                        egui::Pos2::new(r.min.x, r.max.y + 2.0_f32),
                                        egui::Pos2::new(r.max.x, r.max.y + 2.0_f32),
                                    ],
                                    Stroke::new(2.0_f32, accent_blue),
                                );
                            }
                            if resp.clicked() {
                                self.host_editor_tab = tab;
                            }
                        }
                    });
                    ui.add_space(6.0_f32);
                    ui.separator();
                    ui.add_space(8.0_f32);

                    // Constant, not measured: a width derived from the layout
                    // would reintroduce the feedback loop. Leave room for the
                    // scrollbar so rows never overflow the dialog.
                    let full_w = CONTENT_W - 16.0_f32;

                    egui::ScrollArea::vertical()
                        .max_height(400.0_f32)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(full_w);
                            ui.set_max_width(full_w);
                            match self.host_editor_tab {
                            // ================= CONNECTION =================
                            HostEditorTab::Connection => {
                                ui.label(
                                    RichText::new("Session type")
                                        .color(text_secondary)
                                        .size(11.5_f32),
                                );
                                egui::ComboBox::from_id_salt("session_type_combo")
                                    .width(full_w - 4.0_f32)
                                    .selected_text(
                                        RichText::new(session_type_label(&self.new_host_session_type))
                                            .color(text_primary),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (key, label) in SESSION_TYPES {
                                            ui.selectable_value(
                                                &mut self.new_host_session_type,
                                                key.to_string(),
                                                label,
                                            );
                                        }
                                    });

                                ui.add_space(8.0_f32);
                                ui.label(RichText::new("Name").color(text_secondary).size(11.5_f32));
                                MenuEdit::single("host_name_edit")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_name);

                                ui.add_space(8.0_f32);
                                ui.horizontal(|ui| {
                                    let port_w = 110.0_f32;
                                    let host_w = (full_w - port_w - 14.0_f32).max(120.0_f32);
                                    ui.vertical(|ui| {
                                        ui.set_width(host_w);
                                        ui.label(
                                            RichText::new("Host").color(text_secondary).size(11.5_f32),
                                        );
                                        MenuEdit::single("host_ip_edit")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_ip);
                                    });
                                    ui.vertical(|ui| {
                                        ui.set_width(port_w);
                                        ui.label(
                                            RichText::new("Port").color(text_secondary).size(11.5_f32),
                                        );
                                        MenuEdit::single("host_port_edit")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_port);
                                    });
                                });

                                ui.add_space(8.0_f32);
                                ui.label(RichText::new("Username").color(text_secondary).size(11.5_f32));
                                MenuEdit::single("host_user_edit")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_user);

                                ui.add_space(8.0_f32);
                                ui.label(RichText::new("Folder").color(text_secondary).size(11.5_f32));
                                let folders = self.get_existing_folders();
                                ui.horizontal(|ui| {
                                    let mut selected_text = self.new_host_folder.clone();
                                    if selected_text.is_empty() {
                                        selected_text = "Select folder...".to_string();
                                    }
                                    egui::ComboBox::from_id_salt("folder_dropdown_select")
                                        .selected_text(selected_text)
                                        .width(160.0_f32)
                                        .show_ui(ui, |ui| {
                                            for folder in folders {
                                                if ui.selectable_label(self.new_host_folder == folder, &folder).clicked() {
                                                    self.new_host_folder = folder;
                                                }
                                            }
                                        });
                                    MenuEdit::single("host_folder_edit")
    .hint("or type new folder name...")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_folder);
                                });

                                // ---- Authentication group ----
                                ui.add_space(10.0_f32);
                                egui::Frame::none()
                                    .stroke(Stroke::new(1.0_f32, border_color))
                                    .rounding(Rounding::same(6.0_f32))
                                    .inner_margin(egui::Margin::same(10.0_f32))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(
                                            RichText::new("AUTHENTICATION")
                                                .color(text_secondary)
                                                .size(10.5_f32)
                                                .strong(),
                                        );
                                        ui.add_space(6.0_f32);

                                        ui.horizontal_wrapped(|ui| {
                                            for (key, label) in [
                                                ("password", "Password"),
                                                ("vault", "Credential from vault"),
                                                ("private_key", "Private key"),
                                            ] {
                                                ui.radio_value(
                                                    &mut self.new_host_auth_type,
                                                    key.to_string(),
                                                    label,
                                                );
                                                ui.add_space(8.0_f32);
                                            }
                                        });
                                        ui.horizontal_wrapped(|ui| {
                                            for (key, label) in [
                                                ("agent", "SSH agent (Pageant / ssh-agent)"),
                                                ("keyboard_interactive", "Keyboard-interactive (ask every time)"),
                                            ] {
                                                ui.radio_value(
                                                    &mut self.new_host_auth_type,
                                                    key.to_string(),
                                                    label,
                                                );
                                                ui.add_space(8.0_f32);
                                            }
                                        });

                                        ui.add_space(8.0_f32);
                                        match self.new_host_auth_type.as_str() {
                                            "password" => {
                                                ui.label(
                                                    RichText::new("Password")
                                                        .color(text_secondary)
                                                        .size(11.5_f32),
                                                );
                                                ui.horizontal(|ui| {
                                                    // Constant width: measuring here would let the
                                                    // field grow the dialog frame after frame.
                                                    let eye_w = 44.0_f32;
                                                    // Revealing the password
                                                    // also restores Copy/Cut
                                                    // in the context menu.
                                                    let mut pw = MenuEdit::single(
                                                        "host_password_edit",
                                                    )
                                                    .width(full_w - eye_w - 28.0_f32);
                                                    if !self.new_host_reveal_password {
                                                        pw = pw.password();
                                                    }
                                                    pw.show(ui, &mut self.new_host_password);
                                                    if ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new(
                                                                    if self.new_host_reveal_password {
                                                                        "Hide"
                                                                    } else {
                                                                        "Show"
                                                                    },
                                                                )
                                                                .color(text_secondary)
                                                                .size(10.5_f32),
                                                            )
                                                            .frame(false),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.new_host_reveal_password =
                                                            !self.new_host_reveal_password;
                                                    }
                                                });
                                                ui.label(
                                                    RichText::new(
                                                        "Stored encrypted in the local vault. Leave empty to be asked on connect.",
                                                    )
                                                    .color(text_secondary)
                                                    .size(10.5_f32),
                                                );
                                            }
                                            "private_key" => {
                                                ui.label(
                                                    RichText::new("Private key path")
                                                        .color(text_secondary)
                                                        .size(11.5_f32),
                                                );
                                                MenuEdit::single("new_host_key_path_")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_key_path,);
                                                ui.add_space(4.0_f32);
                                                ui.label(
                                                    RichText::new("Key passphrase (if any)")
                                                        .color(text_secondary)
                                                        .size(11.5_f32),
                                                );
                                                MenuEdit::single("new_host_passphrase_")
    .width(f32::INFINITY)
    .password()
    .show(ui, &mut self.new_host_passphrase,);
                                            }
                                            "vault" => {
                                                ui.label(
                                                    RichText::new(
                                                        "The credential stored in the encrypted vault for this host will be used.",
                                                    )
                                                    .color(text_secondary)
                                                    .size(11.0_f32),
                                                );
                                            }
                                            "agent" => {
                                                ui.label(
                                                    RichText::new(
                                                        "Authentication is delegated to the running SSH agent.",
                                                    )
                                                    .color(text_secondary)
                                                    .size(11.0_f32),
                                                );
                                            }
                                            _ => {
                                                ui.label(
                                                    RichText::new(
                                                        "You will be prompted for every challenge the server sends.",
                                                    )
                                                    .color(text_secondary)
                                                    .size(11.0_f32),
                                                );
                                            }
                                        }
                                    });
                            }

                            // ================= ADVANCED =================
                            HostEditorTab::Advanced => {
                                // ---- Jump host group ----
                                egui::Frame::none()
                                    .stroke(Stroke::new(1.0_f32, border_color))
                                    .rounding(Rounding::same(6.0_f32))
                                    .inner_margin(egui::Margin::same(10.0_f32))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(
                                            RichText::new("JUMP HOST (PROXYJUMP)")
                                                .color(text_secondary)
                                                .size(10.5_f32)
                                                .strong(),
                                        );
                                        ui.add_space(6.0_f32);
                                        ui.checkbox(
                                            &mut self.new_host_use_jump,
                                            "Connect through a jump host",
                                        );

                                        if self.new_host_use_jump {
                                            ui.add_space(8.0_f32);
                                            ui.horizontal(|ui| {
                                                let port_w = 90.0_f32;
                                                let host_w =
                                                    (ui.available_width() - port_w - 10.0_f32).max(100.0_f32);
                                                ui.vertical(|ui| {
                                                    ui.set_width(host_w);
                                                    ui.label(
                                                        RichText::new("Jump host")
                                                            .color(text_secondary)
                                                            .size(11.5_f32),
                                                    );
                                                    MenuEdit::single("new_host_jump_host_")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_jump_host,);
                                                });
                                                ui.vertical(|ui| {
                                                    ui.set_width(port_w);
                                                    ui.label(
                                                        RichText::new("Port")
                                                            .color(text_secondary)
                                                            .size(11.5_f32),
                                                    );
                                                    MenuEdit::single("new_host_jump_port_")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_jump_port,);
                                                });
                                            });
                                            ui.add_space(6.0_f32);
                                            ui.label(
                                                RichText::new("Jump username")
                                                    .color(text_secondary)
                                                    .size(11.5_f32),
                                            );
                                            MenuEdit::single("new_host_jump_user_")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_host_jump_user,);
                                        }
                                    });

                                ui.add_space(12.0_f32);

                                // ---- Session group ----
                                egui::Frame::none()
                                    .stroke(Stroke::new(1.0_f32, border_color))
                                    .rounding(Rounding::same(6.0_f32))
                                    .inner_margin(egui::Margin::same(10.0_f32))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(
                                            RichText::new("SESSION")
                                                .color(text_secondary)
                                                .size(10.5_f32)
                                                .strong(),
                                        );
                                        ui.add_space(6.0_f32);

                                        ui.label(
                                            RichText::new("Startup command")
                                                .color(text_secondary)
                                                .size(11.5_f32),
                                        );
                                        MenuEdit::single("new_host_startup_cmd")
    .hint("e.g. tmux attach || tmux")
    .width(f32::INFINITY)
    .monospace()
    .show(ui, &mut self.new_host_startup_cmd);
                                        ui.label(
                                            RichText::new("Sent once the shell is ready.")
                                                .color(text_secondary)
                                                .size(10.5_f32),
                                        );

                                        ui.add_space(8.0_f32);
                                        ui.label(
                                            RichText::new("SFTP start directory")
                                                .color(text_secondary)
                                                .size(11.5_f32),
                                        );
                                        MenuEdit::single("new_host_sftp_dir")
    .hint("default: home directory")
    .width(f32::INFINITY)
    .monospace()
    .show(ui, &mut self.new_host_sftp_dir);

                                        ui.add_space(8.0_f32);
                                        ui.label(
                                            RichText::new("Keep-alive (seconds)")
                                                .color(text_secondary)
                                                .size(11.5_f32),
                                        );
                                        MenuEdit::single("new_host_keepalive")
    .width(120.0_f32)
    .show(ui, &mut self.new_host_keepalive);
                                        ui.label(
                                            RichText::new("0 disables")
                                                .color(text_secondary)
                                                .size(10.5_f32),
                                        );

                                        ui.add_space(10.0_f32);
                                        ui.checkbox(&mut self.new_host_compression, "Compression");
                                        ui.checkbox(
                                            &mut self.new_host_agent_forward,
                                            "Agent forwarding",
                                        );
                                        ui.checkbox(
                                            &mut self.new_host_auto_reconnect,
                                            "Auto-reconnect when the connection drops",
                                        );
                                        ui.checkbox(
                                            &mut self.new_host_open_sftp,
                                            "Open SFTP browser in the sidebar when connected",
                                        );
                                    });
                            }
                            }
                        });

                    ui.add_space(10.0_f32);
                    ui.separator();
                    ui.add_space(8.0_f32);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Save").color(Color32::WHITE).size(12.5_f32),
                                )
                                .fill(Color32::from_rgb(37, 99, 235))
                                .min_size(Vec2::new(70.0_f32, 26.0_f32)),
                            )
                            .clicked()
                        {
                            save_clicked = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancel").color(text_primary).size(12.5_f32),
                                )
                                .fill(btn_neutral_bg)
                                .min_size(Vec2::new(70.0_f32, 26.0_f32)),
                            )
                            .clicked()
                        {
                            cancel_clicked = true;
                        }
                    });
                });

            // Actions run outside the window closure so `self` is fully
            // borrowable again.
            if save_clicked && !self.new_host_ip.trim().is_empty() {
                let port: u16 = self.new_host_port.trim().parse().unwrap_or(22);
                let name = if self.new_host_name.trim().is_empty() {
                    self.new_host_ip.clone()
                } else {
                    self.new_host_name.clone()
                };
                let folder = if self.new_host_folder.trim().is_empty() {
                    "Production".to_string()
                } else {
                    self.new_host_folder.clone()
                };
                let saved_pwd = if self.new_host_password.is_empty() {
                    None
                } else {
                    Some(self.new_host_password.clone())
                };
                let saved_key = if self.new_host_auth_type == "private_key"
                    && !self.new_host_key_path.trim().is_empty()
                {
                    Some(self.new_host_key_path.clone())
                } else {
                    None
                };
                let saved_pass = if self.new_host_passphrase.is_empty() {
                    None
                } else {
                    Some(self.new_host_passphrase.clone())
                };
                let jump_port: u16 = self.new_host_jump_port.trim().parse().unwrap_or(22);
                let keepalive: u32 = self.new_host_keepalive.trim().parse().unwrap_or(30);

                if let Some(edit_id) = self.editing_host_id.clone() {
                    if let Some(existing) =
                        self.vault.profiles.iter_mut().find(|p| p.id == edit_id)
                    {
                        existing.name = name.clone();
                        existing.host = self.new_host_ip.clone();
                        existing.port = port;
                        existing.username = self.new_host_user.clone();
                        existing.folder = folder;
                        existing.auth_type = self.new_host_auth_type.clone();
                        existing.password = saved_pwd;
                        existing.key_path = saved_key;
                        existing.passphrase = saved_pass;

                        existing.session_type = self.new_host_session_type.clone();
                        existing.use_jump_host = self.new_host_use_jump;
                        existing.jump_host = self.new_host_jump_host.clone();
                        existing.jump_port = jump_port;
                        existing.jump_username = self.new_host_jump_user.clone();
                        existing.startup_command = self.new_host_startup_cmd.clone();
                        existing.sftp_start_dir = self.new_host_sftp_dir.clone();
                        existing.keepalive_secs = keepalive;
                        existing.compression = self.new_host_compression;
                        existing.agent_forwarding = self.new_host_agent_forward;
                        existing.auto_reconnect = self.new_host_auto_reconnect;
                        existing.open_sftp_on_connect = self.new_host_open_sftp;
                    }
                    // Keep every open tab for this host in sync with the new name.
                    for tab in &mut self.open_tabs {
                        if let TabKind::Ssh { host_id, .. } = &tab.kind {
                            if host_id == &edit_id {
                                if let Some(idx) = tab.title.rfind(" #") {
                                    tab.title = format!("{}{}", name, &tab.title[idx..]);
                                } else {
                                    tab.title = name.clone();
                                }
                            }
                        }
                    }
                } else {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    let new_profile = HostProfile {
                        id: new_id.clone(),
                        name: name.clone(),
                        folder,
                        tags: vec!["ssh".to_string()],
                        host: self.new_host_ip.clone(),
                        port,
                        username: self.new_host_user.clone(),
                        auth_type: self.new_host_auth_type.clone(),
                        password: saved_pwd,
                        key_path: saved_key,
                        passphrase: saved_pass,
                        tunnels: Vec::new(),
                        last_connected: None,
                        session_type: self.new_host_session_type.clone(),
                        use_jump_host: self.new_host_use_jump,
                        jump_host: self.new_host_jump_host.clone(),
                        jump_port,
                        jump_username: self.new_host_jump_user.clone(),
                        startup_command: self.new_host_startup_cmd.clone(),
                        sftp_start_dir: self.new_host_sftp_dir.clone(),
                        keepalive_secs: keepalive,
                        compression: self.new_host_compression,
                        agent_forwarding: self.new_host_agent_forward,
                        auto_reconnect: self.new_host_auto_reconnect,
                        open_sftp_on_connect: self.new_host_open_sftp,
                    };
                    self.vault.profiles.push(new_profile);
                    self.selected_host_id = new_id.clone();
                    self.open_or_focus_ssh_tab(&new_id);
                }

                let _ = save_vault(&self.vault);
                self.editing_host_id = None;
                self.show_add_host_modal = false;
            }

            if cancel_clicked {
                self.editing_host_id = None;
                self.show_add_host_modal = false;
            }
        }

        // =========================================================================
        // MODAL: ADD / EDIT TUNNEL
        // =========================================================================
        if self.show_add_tunnel_modal {
            let modal_title = if self.editing_tunnel_id.is_some() {
                "Edit Port Forward / SOCKS5 Tunnel"
            } else {
                "Add Port Forward / SOCKS5 Tunnel"
            };
            egui::Window::new(modal_title)
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(380.0_f32, 320.0_f32))
                .show(ctx, |ui| {
                    let target_host_id = self
                        .editing_tunnel_id
                        .as_ref()
                        .map(|(h, _)| h.clone())
                        .unwrap_or_else(|| self.selected_host_id.clone());

                    let target_name = self
                        .vault
                        .profiles
                        .iter()
                        .find(|p| p.id == target_host_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "None".to_string());

                    ui.label(RichText::new(format!("Target Host: {}", target_name)).color(accent_blue).strong());
                    ui.separator();

                    ui.label(RichText::new("Tunnel Name:").color(text_primary));
                    MenuEdit::single("new_tunnel_name")
    .hint("e.g. Postgres DB / Browser SOCKS5")
    .width(f32::INFINITY)
    .show(ui, &mut self.new_tunnel_name);

                    ui.label(RichText::new("Tunnel Type:").color(text_primary));
                    ui.horizontal(|ui| {
                        for (t_val, t_label) in [("local", "Local (-L)"), ("remote", "Remote (-R)"), ("dynamic", "SOCKS5 (-D)")] {
                            if ui.selectable_label(self.new_tunnel_type == t_val, t_label).clicked() {
                                self.new_tunnel_type = t_val.to_string();
                                if t_val == "dynamic" && self.editing_tunnel_id.is_none() {
                                    self.new_tunnel_bind_port = "1080".to_string();
                                }
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Bind IP:").color(text_primary));
                        MenuEdit::single("new_tunnel_bind_host")
    .width(110.0_f32)
    .show(ui, &mut self.new_tunnel_bind_host);
                        ui.label(RichText::new("Port:").color(text_primary));
                        MenuEdit::single("new_tunnel_bind_port")
    .width(60.0_f32)
    .show(ui, &mut self.new_tunnel_bind_port);
                    });

                    if self.new_tunnel_type != "dynamic" {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Remote IP:").color(text_primary));
                            MenuEdit::single("new_tunnel_remote_host")
    .width(100.0_f32)
    .show(ui, &mut self.new_tunnel_remote_host);
                            ui.label(RichText::new("Port:").color(text_primary));
                            MenuEdit::single("new_tunnel_remote_port")
    .width(60.0_f32)
    .show(ui, &mut self.new_tunnel_remote_port);
                        });
                    }

                    ui.add_space(6.0_f32);
                    ui.checkbox(&mut self.new_tunnel_auto_start, "Auto-start when host connects (like PuTTY)");

                    ui.add_space(12.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("Save Tunnel").color(Color32::WHITE)).fill(Color32::from_rgb(37, 99, 235)))
                            .clicked()
                        {
                            let bind_port: u16 = self.new_tunnel_bind_port.trim().parse().unwrap_or(1080);
                            let remote_port: u16 = self.new_tunnel_remote_port.trim().parse().unwrap_or(3306);
                            let tun_name = if self.new_tunnel_name.trim().is_empty() {
                                format!("{} :{}", self.new_tunnel_type.to_uppercase(), bind_port)
                            } else {
                                self.new_tunnel_name.clone()
                            };

                            if let Some((edit_host_id, edit_tun_id)) = &self.editing_tunnel_id {
                                if let Some(host) = self.vault.profiles.iter_mut().find(|p| &p.id == edit_host_id) {
                                    if let Some(existing_tun) = host.tunnels.iter_mut().find(|t| &t.id == edit_tun_id) {
                                        existing_tun.name = tun_name;
                                        existing_tun.tunnel_type = self.new_tunnel_type.clone();
                                        existing_tun.bind_host = self.new_tunnel_bind_host.clone();
                                        existing_tun.bind_port = bind_port;
                                        existing_tun.remote_host = if self.new_tunnel_type == "dynamic" {
                                            None
                                        } else {
                                            Some(self.new_tunnel_remote_host.clone())
                                        };
                                        existing_tun.remote_port = if self.new_tunnel_type == "dynamic" {
                                            None
                                        } else {
                                            Some(remote_port)
                                        };
                                        existing_tun.auto_start = self.new_tunnel_auto_start;
                                    }
                                    let _ = save_vault(&self.vault);
                                }
                            } else {
                                let new_tun = TunnelConfig {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    name: tun_name,
                                    tunnel_type: self.new_tunnel_type.clone(),
                                    bind_host: self.new_tunnel_bind_host.clone(),
                                    bind_port,
                                    remote_host: if self.new_tunnel_type == "dynamic" {
                                        None
                                    } else {
                                        Some(self.new_tunnel_remote_host.clone())
                                    },
                                    remote_port: if self.new_tunnel_type == "dynamic" {
                                        None
                                    } else {
                                        Some(remote_port)
                                    },
                                    auto_start: self.new_tunnel_auto_start,
                                    is_active: false,
                                    bytes_transferred: 0,
                                };

                                if let Some(host) = self.vault.profiles.iter_mut().find(|p| p.id == self.selected_host_id) {
                                    host.tunnels.push(new_tun);
                                    let _ = save_vault(&self.vault);
                                }
                            }

                            self.editing_tunnel_id = None;
                            self.show_add_tunnel_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.editing_tunnel_id = None;
                            self.show_add_tunnel_modal = false;
                        }
                    });
                });
        }

        // =========================================================================
        // MODAL: SETTINGS & TERMINAL APPEARANCE
        // =========================================================================
        if self.show_settings_modal {
            egui::Window::new("Terminal & Appearance Settings")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(400.0_f32, 420.0_f32))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Configure Appearance Mode, Font & Palette").color(text_secondary));
                    ui.separator();

                    ui.add_space(8.0_f32);
                    ui.label(RichText::new("Appearance Mode:").color(text_primary).strong());
                    ui.horizontal(|ui| {
                        for (mode_val, mode_txt) in [
                            ("night", "Night (Dark)"),
                            ("day", "Day (Light)"),
                            ("auto", "Auto (System)"),
                        ] {
                            let selected = self.appearance_mode == mode_val;
                            if ui
                                .selectable_label(
                                    selected,
                                    RichText::new(mode_txt).color(if selected {
                                        Color32::WHITE
                                    } else {
                                        text_primary
                                    }),
                                )
                                .clicked()
                            {
                                self.appearance_mode = mode_val.to_string();
                                self.vault.appearance_mode = mode_val.to_string();
                                let _ = save_vault(&self.vault);
                            }
                        }
                    });
                    if self.appearance_mode == "auto" {
                        let sys_str = match os_theme {
                            Some(egui::Theme::Light) => "Light",
                            Some(egui::Theme::Dark) => "Dark",
                            None => "Dark (Default)",
                        };
                        ui.label(
                            RichText::new(format!("Detected OS theme: {}", sys_str))
                                .size(11.0_f32)
                                .color(accent_blue),
                        );
                    }

                    ui.add_space(8.0_f32);
                    ui.label(RichText::new("Terminal Font Size:").color(text_primary).strong());
                    if ui
                        .add(egui::Slider::new(&mut self.font_size, 11.0_f32..=24.0_f32).text("px"))
                        .changed()
                    {
                        self.vault.font_size = self.font_size;
                    }

                    ui.add_space(6.0_f32);
                    ui.label(RichText::new("UI Zoom Scale:").color(text_primary).strong());
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Slider::new(&mut self.ui_zoom, 0.8_f32..=2.0_f32).text("x"))
                            .changed()
                        {
                            ctx.set_zoom_factor(self.ui_zoom);
                            self.vault.ui_zoom = self.ui_zoom;
                        }
                        if ui.small_button("Default (1.21x)").clicked() {
                            self.ui_zoom = 1.21_f32;
                            ctx.set_zoom_factor(self.ui_zoom);
                            self.vault.ui_zoom = self.ui_zoom;
                            let _ = save_vault(&self.vault);
                        }
                    });

                    ui.add_space(8.0_f32);
                    ui.label(RichText::new("Terminal Theme Palette:").color(text_primary).strong());
                    let themes = [
                        "Dracula",
                        "Tokyo Night",
                        "Nord",
                        "Monokai Pro",
                        "Cyberpunk",
                        "Daylight Clean",
                        "Solarized Light",
                    ];
                    for th in themes {
                        let selected = self.theme_name == th;
                        if ui
                            .selectable_label(
                                selected,
                                RichText::new(th).color(if selected {
                                    Color32::WHITE
                                } else {
                                    text_primary
                                }),
                            )
                            .clicked()
                        {
                            self.theme_name = th.to_string();
                            self.vault.terminal_theme = th.to_string();
                            let _ = save_vault(&self.vault);
                        }
                    }

                    ui.add_space(10.0_f32);
                    ui.separator();
                    ui.add_space(6.0_f32);
                    ui.label(RichText::new("Sync Account & Backup:").color(text_primary).strong());
                    ui.horizontal(|ui| {
                        let email_lbl = if self.vault.sync_email.trim().is_empty() {
                            "Not configured".to_string()
                        } else {
                            self.vault.sync_email.clone()
                        };
                        ui.label(RichText::new(email_lbl).size(11.5_f32).color(accent_blue));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Configure Sync Email...").clicked() {
                                self.sync_email_input = self.vault.sync_email.clone();
                                self.sync_feedback = None;
                                self.show_settings_modal = false;
                                self.show_sync_modal = true;
                            }
                        });
                    });

                    ui.add_space(12.0_f32);
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.vault.font_size = self.font_size;
                        self.vault.ui_zoom = self.ui_zoom;
                        self.vault.terminal_theme = self.theme_name.clone();
                        self.vault.appearance_mode = self.appearance_mode.clone();
                        let _ = save_vault(&self.vault);
                        self.show_settings_modal = false;
                    }
                });
        }

        // =========================================================================
        // MODAL: SYNC ACCOUNT & CROSS-PLATFORM ENCRYPTED VAULT (.stvault)
        // =========================================================================
        if self.show_sync_modal {
            egui::Window::new("Sync Account & Encrypted Vault")
                .collapsible(false)
                .resizable(false)
                .default_size(Vec2::new(430.0_f32, 330.0_f32))
                .show(ctx, |ui| {
                    ui.heading(RichText::new("Account & Cross-Platform E2EE Sync").color(text_primary));
                    ui.label(
                        RichText::new("Configure your Google sync email and export/import AES-256-GCM + Argon2id encrypted .stvault files across Windows, macOS, and Linux.")
                            .size(11.5_f32)
                            .color(text_secondary),
                    );
                    ui.add_space(8.0_f32);

                    ui.label(RichText::new("Sync Email (Google Account):").color(text_primary).strong());
                    ui.horizontal(|ui| {
                        MenuEdit::single("sync_email_input")
    .hint("you@gmail.com")
    .width(270.0_f32)
    .show(ui, &mut self.sync_email_input);
                        if ui
                            .add(egui::Button::new(RichText::new("Save Email").color(Color32::WHITE)).fill(Color32::from_rgb(124, 58, 237)))
                            .clicked()
                        {
                            self.vault.sync_email = self.sync_email_input.trim().to_string();
                            self.vault.sync_revision = self.vault.sync_revision.saturating_add(1);
                            let _ = save_vault(&self.vault);
                            self.sync_status_text = if self.vault.sync_email.is_empty() {
                                format!("Local vault only (Rev #{})", self.vault.sync_revision)
                            } else {
                                format!("Sync ready (Rev #{})", self.vault.sync_revision)
                            };
                            self.sync_feedback = Some("Email saved to vault configuration.".to_string());
                        }
                    });

                    ui.add_space(10.0_f32);
                    ui.separator();
                    ui.add_space(6.0_f32);

                    ui.label(RichText::new("Encrypted Portable Backup (.stvault):").color(text_primary).strong());
                    ui.label(RichText::new("Vault File Path (e.g. inside your Google Drive / OneDrive folder):").size(11.0_f32).color(text_secondary));
                    MenuEdit::single("sync_file_path")
    .width(f32::INFINITY)
    .show(ui, &mut self.sync_file_path);

                    ui.label(RichText::new("Master Encryption Password:").size(11.0_f32).color(text_secondary));
                    MenuEdit::single("sync_master_password")
    .hint("Enter master password for AES-256-GCM...")
    .width(f32::INFINITY)
    .password()
    .show(ui, &mut self.sync_master_password);

                    ui.add_space(8.0_f32);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("Export Encrypted .stvault").color(Color32::WHITE)).fill(Color32::from_rgb(37, 99, 235)))
                            .clicked()
                        {
                            let path = std::path::PathBuf::from(self.sync_file_path.trim());
                            match export_encrypted_stvault(&self.vault, &self.sync_master_password, &path) {
                                Ok(_) => {
                                    self.vault.sync_revision = self.vault.sync_revision.saturating_add(1);
                                    let _ = save_vault(&self.vault);
                                    self.sync_status_text = format!("Exported E2EE (Rev #{})", self.vault.sync_revision);
                                    self.sync_feedback = Some(format!("Encrypted vault saved to {}", path.display()));
                                }
                                Err(e) => {
                                    self.sync_feedback = Some(format!("Error: {}", e));
                                }
                            }
                        }

                        if ui.button("Import Encrypted .stvault").clicked() {
                            let path = std::path::PathBuf::from(self.sync_file_path.trim());
                            match import_encrypted_stvault(&self.sync_master_password, &path) {
                                Ok(imported) => {
                                    self.vault = imported;
                                    self.appearance_mode = self.vault.appearance_mode.clone();
                                    self.theme_name = self.vault.terminal_theme.clone();
                                    self.font_size = self.vault.font_size.clamp(10.0, 26.0);
                                    self.ui_zoom = self.vault.ui_zoom.clamp(0.8, 2.2);
                                    ctx.set_zoom_factor(self.ui_zoom);
                                    self.sync_email_input = self.vault.sync_email.clone();
                                    self.last_applied_dark = None;
                                    let _ = save_vault(&self.vault);
                                    self.sync_status_text = format!("Imported E2EE (Rev #{})", self.vault.sync_revision);
                                    self.sync_feedback = Some(format!("Imported {} host(s) successfully.", self.vault.profiles.len()));
                                }
                                Err(e) => {
                                    self.sync_feedback = Some(format!("Error: {}", e));
                                }
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                self.show_sync_modal = false;
                            }
                        });
                    });

                    if let Some(msg) = &self.sync_feedback {
                        ui.add_space(6.0_f32);
                        let col = if msg.starts_with("Error:") {
                            Color32::from_rgb(248, 113, 113)
                        } else {
                            accent_green
                        };
                        ui.label(RichText::new(msg).size(11.0_f32).color(col));
                    }
                });
        }
    }

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        // Stop SSH sessions
        for session in self.active_sessions.values() {
            session.disconnect();
        }
        for session in self.host_tunnel_sessions.values() {
            session.disconnect();
        }
        // Stop Shell sessions
        for session in self.shell_sessions.values() {
            session.kill();
        }
        // Wait briefly for cleanup
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}















