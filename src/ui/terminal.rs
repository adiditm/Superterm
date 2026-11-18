use eframe::egui::{self, Align2, Color32, CursorIcon, EventFilter, FontId, Key, PointerButton, Pos2, Rect, Stroke, Vec2};
use std::num::NonZeroU16;
use std::time::{Duration, Instant};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellPos {
    /// Document-relative row: 0..rows when scrollback == 0, negative when in scrollback history
    pub row: isize,
    pub col: u16,
}

pub struct TerminalEmulator {
    pub parser: vt100::Parser,
    pub cols: u16,
    pub rows: u16,
    pub selection_start: Option<CellPos>,
    pub selection_end: Option<CellPos>,
    pub is_dragging: bool,
    pub last_autoscroll: Option<Instant>,
}

impl TerminalEmulator {
    pub fn new(rows: u16, cols: u16) -> Self {
        let r = NonZeroU16::new(rows.max(8)).unwrap();
        let c = NonZeroU16::new(cols.max(20)).unwrap();
        Self {
            parser: vt100::Parser::new(r, c, 3000),
            cols: cols.max(20),
            rows: rows.max(8),
            selection_start: None,
            selection_end: None,
            is_dragging: false,
            last_autoscroll: None,
        }
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) {
        // Keep user's scroll position if they are scrolled up reading/selecting history
        let prev_sb = self.parser.screen().scrollback();
        let prev_len = self.parser.screen().scrollback_len();
        self.parser.process(bytes);
        if prev_sb > 0 {
            let new_len = self.parser.screen().scrollback_len();
            let added = new_len.saturating_sub(prev_len);
            self.parser.screen_mut().set_scrollback(prev_sb + added);
        }
    }

    pub fn writeln(&mut self, line: &str) {
        self.process_bytes(line.as_bytes());
        self.process_bytes(b"\r\n");
    }

    /// True when anything has ever been drawn on this screen.
    ///
    /// Used to tell a shell that printed a prompt from one that died silently.
    pub fn has_content(&self) -> bool {
        let screen = self.parser.screen();
        if screen.scrollback_len() > 0 {
            return true;
        }
        !screen.contents().trim().is_empty()
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> bool {
        let r = rows.max(8);
        let c = cols.max(20);
        if r != self.rows || c != self.cols {
            self.rows = r;
            self.cols = c;
            self.clear_selection();
            if let (Some(nr), Some(nc)) = (NonZeroU16::new(r), NonZeroU16::new(c)) {
                self.parser.set_size(nr, nc);
            }
            return true;
        }
        false
    }

    pub fn clear(&mut self) {
        self.clear_selection();
        self.parser.screen_mut().set_scrollback(0);
        self.process_bytes(b"\x1b[2J\x1b[H");
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_dragging = false;
    }

    pub fn has_selection(&self) -> bool {
        matches!((self.selection_start, self.selection_end), (Some(a), Some(b)) if a != b)
    }

    pub fn normalized_selection(&self) -> Option<(CellPos, CellPos)> {
        match (self.selection_start, self.selection_end) {
            (Some(a), Some(b)) => {
                if a <= b {
                    Some((a, b))
                } else {
                    Some((b, a))
                }
            }
            _ => None,
        }
    }

    pub fn is_cell_selected(&self, doc_row: isize, col: u16) -> bool {
        if let Some((start, end)) = self.normalized_selection() {
            if start == end {
                return false;
            }
            let pos = CellPos { row: doc_row, col };
            pos >= start && pos <= end
        } else {
            false
        }
    }

    pub fn extract_selected_text(&mut self) -> String {
        let Some((start, end)) = self.normalized_selection() else {
            return String::new();
        };
        if start == end {
            return String::new();
        }

        let orig_sb = self.parser.screen().scrollback();
        let max_sb = self.parser.screen().scrollback_len();
        let (s_rows, s_cols) = self.parser.screen().size();
        let max_vis_row = s_rows.get().saturating_sub(1);
        let max_col = s_cols.get().saturating_sub(1);
        let min_doc_row = -(max_sb as isize);
        let max_doc_row = max_vis_row as isize;

        let r_from = start.row.max(min_doc_row);
        let r_to = end.row.min(max_doc_row);

        let mut lines = Vec::new();
        for doc_row in r_from..=r_to {
            let (needed_sb, vis_row) = if doc_row < 0 {
                ((-doc_row) as usize, 0u16)
            } else {
                (0usize, doc_row as u16)
            };

            self.parser.screen_mut().set_scrollback(needed_sb);
            let screen = self.parser.screen();

            let col_from = if doc_row == start.row { start.col } else { 0 };
            let col_to = if doc_row == end.row { end.col.min(max_col) } else { max_col };

            let mut line = String::new();
            for col in col_from..=col_to {
                if let Some(cell) = screen.cell(vis_row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    if cell.has_contents() {
                        line.push_str(cell.contents());
                    } else {
                        line.push(' ');
                    }
                }
            }
            lines.push(line.trim_end_matches(' ').to_string());
        }

        // Restore original scrollback view
        self.parser.screen_mut().set_scrollback(orig_sb);
        lines.join("\n")
    }

    pub fn select_word_at(&mut self, vis_row: u16, col: u16) {
        let sb = self.parser.screen().scrollback() as isize;
        let screen = self.parser.screen();
        let (_, s_cols) = screen.size();
        let max_col = s_cols.get().saturating_sub(1);

        let is_word_char = |c: u16| -> bool {
            if let Some(cell) = screen.cell(vis_row, c) {
                if cell.has_contents() {
                    let s = cell.contents();
                    return !s.trim().is_empty();
                }
            }
            false
        };

        let doc_row = (vis_row as isize) - sb;
        if !is_word_char(col) {
            self.selection_start = Some(CellPos { row: doc_row, col });
            self.selection_end = Some(CellPos { row: doc_row, col });
            return;
        }

        let mut lo = col;
        while lo > 0 && is_word_char(lo - 1) {
            lo -= 1;
        }
        let mut hi = col;
        while hi < max_col && is_word_char(hi + 1) {
            hi += 1;
        }
        self.selection_start = Some(CellPos { row: doc_row, col: lo });
        self.selection_end = Some(CellPos { row: doc_row, col: hi });
    }
}

pub fn vt100_color_to_egui(color: vt100::Color, default_color: Color32, is_light: bool) -> Color32 {
    match color {
        vt100::Color::Default => default_color,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(idx) => ansi_idx_to_color(idx, is_light),
    }
}

fn ansi_idx_to_color(idx: u8, is_light: bool) -> Color32 {
    match idx {
        0 => Color32::from_rgb(24, 26, 34),
        1 => Color32::from_rgb(200, 48, 48),
        2 => if is_light { Color32::from_rgb(21, 128, 61) } else { Color32::from_rgb(56, 201, 108) },
        3 => if is_light { Color32::from_rgb(161, 98, 7) } else { Color32::from_rgb(234, 179, 8) },
        4 => Color32::from_rgb(29, 78, 216),
        5 => if is_light { Color32::from_rgb(147, 51, 234) } else { Color32::from_rgb(192, 86, 224) },
        6 => if is_light { Color32::from_rgb(14, 116, 144) } else { Color32::from_rgb(14, 140, 168) },
        7 => if is_light { Color32::from_rgb(71, 85, 105) } else { Color32::from_rgb(229, 231, 235) },
        8 => Color32::from_rgb(107, 114, 128),
        9 => if is_light { Color32::from_rgb(220, 38, 38) } else { Color32::from_rgb(248, 113, 113) },
        10 => if is_light { Color32::from_rgb(22, 163, 74) } else { Color32::from_rgb(74, 222, 128) },
        11 => if is_light { Color32::from_rgb(202, 138, 4) } else { Color32::from_rgb(250, 204, 21) },
        12 => if is_light { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(96, 165, 250) },
        13 => if is_light { Color32::from_rgb(192, 38, 211) } else { Color32::from_rgb(232, 121, 249) },
        14 => if is_light { Color32::from_rgb(8, 145, 178) } else { Color32::from_rgb(34, 211, 238) },
        15 => if is_light { Color32::from_rgb(15, 23, 42) } else { Color32::from_rgb(255, 255, 255) },
        16..=231 => {
            let i = idx - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(val(r), val(g), val(b))
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

pub struct TerminalInputActions {
    pub bytes_to_send: Vec<u8>,
    pub resized_to: Option<(u16, u16)>,
}

fn encode_mouse_event(
    encoding: MouseProtocolEncoding,
    btn_code: u8,
    col_1: u16,
    row_1: u16,
    is_release: bool,
) -> Vec<u8> {
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let suffix = if is_release { 'm' } else { 'M' };
            format!("\x1b[<{};{};{}{}", btn_code, col_1, row_1, suffix).into_bytes()
        }
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            let cb = if is_release { 3 + 32 } else { btn_code.saturating_add(32) };
            let cx = (col_1.min(223) as u8).saturating_add(32);
            let cy = (row_1.min(223) as u8).saturating_add(32);
            vec![0x1b, b'[', b'M', cb, cx, cy]
        }
    }
}

fn copy_text_to_clipboard(text: &str) {
    if !text.is_empty() {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text.to_string());
        }
    }
}

fn read_clipboard_for_paste() -> Option<Vec<u8>> {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
            return Some(normalized.into_bytes());
        }
    }
    None
}

pub fn render_terminal_viewport(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    emu: &mut TerminalEmulator,
    font_size: f32,
    bg_color: Color32,
    default_fg: Color32,
    cursor_color: Color32,
    is_light_terminal: bool,
    is_connected: bool,
    modal_open: bool,
) -> TerminalInputActions {
    let mut actions = TerminalInputActions {
        bytes_to_send: Vec::new(),
        resized_to: None,
    };

    let font_id = FontId::monospace(font_size);
    let (char_width, line_height) = ctx.fonts(|f| {
        let cw = f.glyph_width(&font_id, 'M').max(5.0_f32);
        let lh = f.row_height(&font_id).max(font_size * 1.15_f32);
        (cw, lh)
    });

    let avail = ui.available_size();
    let pad = 6.0_f32;
    let usable_w = (avail.x - pad * 2.0_f32 - 6.0_f32).max(100.0_f32);
    let usable_h = (avail.y - pad * 2.0_f32).max(80.0_f32);

    let target_cols = (usable_w / char_width).floor().clamp(20.0_f32, 300.0_f32) as u16;
    let target_rows = (usable_h / line_height).floor().clamp(8.0_f32, 160.0_f32) as u16;

    if emu.resize(target_rows, target_cols) {
        actions.resized_to = Some((target_cols, target_rows));
    }

    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    let response = response.on_hover_cursor(CursorIcon::Text);
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0_f32, bg_color);

    let origin = Pos2::new(rect.min.x + pad, rect.min.y + pad);

    let pos_to_vis_cell = |pos: Pos2| -> (u16, u16) {
        let c = (((pos.x - origin.x) / char_width).floor().max(0.0_f32) as u16)
            .min(target_cols.saturating_sub(1));
        let r = (((pos.y - origin.y) / line_height).floor().max(0.0_f32) as u16)
            .min(target_rows.saturating_sub(1));
        (r, c)
    };

    // Focus handling
    let wants_other_text = ctx.wants_keyboard_input() && !response.has_focus();
    if response.clicked() || response.drag_started() {
        response.request_focus();
    } else if is_connected && !modal_open && !wants_other_text {
        response.request_focus();
    }

    let is_focused = response.has_focus();
    if is_focused {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    let app_cursor = emu.parser.screen().application_cursor();
    let mouse_mode = emu.parser.screen().mouse_protocol_mode();
    let mouse_encoding = emu.parser.screen().mouse_protocol_encoding();
    let shift_held = ctx.input(|i| i.modifiers.shift);

    // Mouse handling
    if mouse_mode != MouseProtocolMode::None && !shift_held {
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if rect.contains(pos) {
                let col_1 = (((pos.x - origin.x) / char_width).floor().max(0.0_f32) as u16)
                    .saturating_add(1)
                    .min(target_cols);
                let row_1 = (((pos.y - origin.y) / line_height).floor().max(0.0_f32) as u16)
                    .saturating_add(1)
                    .min(target_rows);

                if ctx.input(|i| i.pointer.button_pressed(PointerButton::Primary)) {
                    actions.bytes_to_send.extend_from_slice(&encode_mouse_event(
                        mouse_encoding,
                        0,
                        col_1,
                        row_1,
                        false,
                    ));
                }
                if ctx.input(|i| i.pointer.button_released(PointerButton::Primary))
                    && mouse_mode != MouseProtocolMode::Press
                {
                    actions.bytes_to_send.extend_from_slice(&encode_mouse_event(
                        mouse_encoding,
                        0,
                        col_1,
                        row_1,
                        true,
                    ));
                }
                if ctx.input(|i| i.pointer.button_pressed(PointerButton::Secondary)) {
                    actions.bytes_to_send.extend_from_slice(&encode_mouse_event(
                        mouse_encoding,
                        2,
                        col_1,
                        row_1,
                        false,
                    ));
                }
                if ctx.input(|i| i.pointer.button_released(PointerButton::Secondary))
                    && mouse_mode != MouseProtocolMode::Press
                {
                    actions.bytes_to_send.extend_from_slice(&encode_mouse_event(
                        mouse_encoding,
                        2,
                        col_1,
                        row_1,
                        true,
                    ));
                }

                let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll_delta.abs() > 1.0_f32 {
                    let btn = if scroll_delta > 0.0_f32 { 64 } else { 65 };
                    actions.bytes_to_send.extend_from_slice(&encode_mouse_event(
                        mouse_encoding,
                        btn,
                        col_1,
                        row_1,
                        false,
                    ));
                }
            }
        }
    } else {
        // Drag selection + edge auto-scrolling!
        let primary_down = ctx.input(|i| i.pointer.button_down(PointerButton::Primary));
        if !primary_down {
            emu.is_dragging = false;
        }

        if response.drag_started_by(PointerButton::Primary) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let (vis_r, c) = pos_to_vis_cell(pos);
                let sb = emu.parser.screen().scrollback() as isize;
                let cell = CellPos {
                    row: (vis_r as isize) - sb,
                    col: c,
                };
                emu.selection_start = Some(cell);
                emu.selection_end = Some(cell);
                emu.is_dragging = true;
                emu.last_autoscroll = None;
            }
        } else if emu.is_dragging && primary_down {
            if let Some(pos) = ctx.input(|i| i.pointer.latest_pos().or_else(|| i.pointer.interact_pos())) {
                // Edge auto-scroll when dragging above top or below bottom of terminal viewport!
                let now = Instant::now();
                let can_step = emu
                    .last_autoscroll
                    .map(|t| now.duration_since(t) >= Duration::from_millis(35))
                    .unwrap_or(true);

                if pos.y > rect.max.y - 6.0_f32 {
                    let over = (pos.y - (rect.max.y - 6.0_f32)).max(1.0_f32);
                    let step = ((over / 24.0_f32).ceil() as usize).clamp(1, 5);
                    let cur_sb = emu.parser.screen().scrollback();
                    if cur_sb > 0 {
                        if can_step {
                            emu.parser.screen_mut().set_scrollback(cur_sb.saturating_sub(step));
                            emu.last_autoscroll = Some(now);
                        }
                        ctx.request_repaint_after(Duration::from_millis(30));
                    }
                } else if pos.y < rect.min.y + 6.0_f32 {
                    let over = ((rect.min.y + 6.0_f32) - pos.y).max(1.0_f32);
                    let step = ((over / 24.0_f32).ceil() as usize).clamp(1, 5);
                    let cur_sb = emu.parser.screen().scrollback();
                    let max_sb = emu.parser.screen().scrollback_len();
                    if cur_sb < max_sb {
                        if can_step {
                            emu.parser.screen_mut().set_scrollback((cur_sb + step).min(max_sb));
                            emu.last_autoscroll = Some(now);
                        }
                        ctx.request_repaint_after(Duration::from_millis(30));
                    }
                }

                // Also support mouse scroll wheel while holding left button to select!
                let wheel = ctx.input(|i| i.raw_scroll_delta.y);
                if wheel.abs() > 1.0_f32 {
                    let lines_delta = (wheel / line_height).round() as isize;
                    if lines_delta != 0 {
                        let cur_sb = emu.parser.screen().scrollback();
                        let max_sb = emu.parser.screen().scrollback_len();
                        let next_sb = if lines_delta > 0 {
                            (cur_sb + (lines_delta as usize)).min(max_sb)
                        } else {
                            cur_sb.saturating_sub((-lines_delta) as usize)
                        };
                        emu.parser.screen_mut().set_scrollback(next_sb);
                    }
                }

                let (vis_r, c) = pos_to_vis_cell(pos);
                let sb = emu.parser.screen().scrollback() as isize;
                emu.selection_end = Some(CellPos {
                    row: (vis_r as isize) - sb,
                    col: c,
                });
            }
        } else if response.double_clicked() {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let (vis_r, c) = pos_to_vis_cell(pos);
                emu.select_word_at(vis_r, c);
            }
        } else if response.clicked_by(PointerButton::Primary) {
            emu.clear_selection();
        }

        // Scrollback wheel when hovering (not dragging)
        if response.hovered() && !emu.is_dragging {
            let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
            if scroll_delta.abs() > 1.0_f32 {
                let lines_delta = (scroll_delta / line_height).round() as isize;
                if lines_delta != 0 {
                    let current_sb = emu.parser.screen().scrollback();
                    let max_sb = emu.parser.screen().scrollback_len();
                    let next_sb = if lines_delta > 0 {
                        (current_sb + (lines_delta as usize)).min(max_sb)
                    } else {
                        current_sb.saturating_sub((-lines_delta) as usize)
                    };
                    emu.parser.screen_mut().set_scrollback(next_sb);
                }
            }
        }

        // Right-click context menu
        response.context_menu(|ui| {
            let has_sel = emu.has_selection();
            if ui
                .add_enabled(has_sel, egui::Button::new("Copy"))
                .clicked()
            {
                let text = emu.extract_selected_text();
                copy_text_to_clipboard(&text);
                emu.clear_selection();
                ui.close_menu();
            }
            if ui.button("Paste").clicked() {
                if let Some(bytes) = read_clipboard_for_paste() {
                    actions.bytes_to_send.extend_from_slice(&bytes);
                }
                ui.close_menu();
            }
            if ui.button("Select All (Visible + History)").clicked() {
                let max_sb = emu.parser.screen().scrollback_len() as isize;
                emu.selection_start = Some(CellPos { row: -max_sb, col: 0 });
                emu.selection_end = Some(CellPos {
                    row: (target_rows.saturating_sub(1)) as isize,
                    col: target_cols.saturating_sub(1),
                });
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Send Ctrl+C (Interrupt)").clicked() {
                actions.bytes_to_send.push(0x03);
                ui.close_menu();
            }
            if ui.button("Clear Screen").clicked() {
                emu.clear();
                ui.close_menu();
            }
        });
    }

    // Keyboard & Copy/Paste event capture
    if is_focused && !modal_open {
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Copy => {
                    if emu.has_selection() {
                        let text = emu.extract_selected_text();
                        copy_text_to_clipboard(&text);
                        emu.clear_selection();
                    } else {
                        emu.clear_selection();
                        emu.parser.screen_mut().set_scrollback(0);
                        actions.bytes_to_send.push(0x03);
                    }
                }
                egui::Event::Text(text) => {
                    emu.clear_selection();
                    emu.parser.screen_mut().set_scrollback(0);
                    actions.bytes_to_send.extend_from_slice(text.as_bytes());
                }
                egui::Event::Paste(text) => {
                    emu.clear_selection();
                    emu.parser.screen_mut().set_scrollback(0);
                    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
                    actions.bytes_to_send.extend_from_slice(normalized.as_bytes());
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if modifiers.ctrl && key == Key::C && (modifiers.shift || emu.has_selection()) {
                        let text = emu.extract_selected_text();
                        copy_text_to_clipboard(&text);
                        emu.clear_selection();
                        continue;
                    }

                    if modifiers.ctrl && modifiers.shift && key == Key::V {
                        if let Some(bytes) = read_clipboard_for_paste() {
                            emu.clear_selection();
                            emu.parser.screen_mut().set_scrollback(0);
                            actions.bytes_to_send.extend_from_slice(&bytes);
                        }
                        continue;
                    }

                    if modifiers.ctrl && !modifiers.alt {
                        if let Some(ctrl_byte) = key_to_ctrl_byte(key) {
                            emu.clear_selection();
                            emu.parser.screen_mut().set_scrollback(0);
                            actions.bytes_to_send.push(ctrl_byte);
                            continue;
                        }
                    }

                    let seq: Option<&[u8]> = match key {
                        Key::Enter => Some(b"\r"),
                        Key::Backspace => Some(b"\x7f"),
                        Key::Tab => Some(&[0x09]),
                        Key::Escape => Some(b"\x1b"),
                        Key::ArrowUp => Some(if app_cursor { b"\x1bOA" } else { b"\x1b[A" }),
                        Key::ArrowDown => Some(if app_cursor { b"\x1bOB" } else { b"\x1b[B" }),
                        Key::ArrowRight => Some(if app_cursor { b"\x1bOC" } else { b"\x1b[C" }),
                        Key::ArrowLeft => Some(if app_cursor { b"\x1bOD" } else { b"\x1b[D" }),
                        Key::Home => Some(if app_cursor { b"\x1bOH" } else { b"\x1b[H" }),
                        Key::End => Some(if app_cursor { b"\x1bOF" } else { b"\x1b[F" }),
                        Key::PageUp => Some(b"\x1b[5~"),
                        Key::PageDown => Some(b"\x1b[6~"),
                        Key::Delete => Some(b"\x1b[3~"),
                        Key::Insert => Some(b"\x1b[2~"),
                        Key::F1 => Some(b"\x1bOP"),
                        Key::F2 => Some(b"\x1bOQ"),
                        Key::F3 => Some(b"\x1bOR"),
                        Key::F4 => Some(b"\x1bOS"),
                        Key::F5 => Some(b"\x1b[15~"),
                        Key::F6 => Some(b"\x1b[17~"),
                        Key::F7 => Some(b"\x1b[18~"),
                        Key::F8 => Some(b"\x1b[19~"),
                        Key::F9 => Some(b"\x1b[20~"),
                        Key::F10 => Some(b"\x1b[21~"),
                        Key::F11 => Some(b"\x1b[23~"),
                        Key::F12 => Some(b"\x1b[24~"),
                        _ => None,
                    };
                    if let Some(bytes) = seq {
                        emu.clear_selection();
                        emu.parser.screen_mut().set_scrollback(0);
                        actions.bytes_to_send.extend_from_slice(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    // Render VT100 Screen Grid
    let sel_color = if is_light_terminal {
        Color32::from_rgba_unmultiplied(59, 130, 246, 85)
    } else {
        Color32::from_rgba_unmultiplied(124, 58, 237, 115)
    };

    let current_sb = emu.parser.screen().scrollback() as isize;
    let total_sb_len = emu.parser.screen().scrollback_len();
    let screen = emu.parser.screen();
    let (screen_rows, screen_cols) = screen.size();
    let r_count = screen_rows.get();
    let c_count = screen_cols.get();

    for row in 0..r_count {
        let row_y = origin.y + (row as f32) * line_height;
        let doc_row = (row as isize) - current_sb;

        // Pass 1: Contiguous background rects
        let mut span_start: Option<(u16, Color32)> = None;
        for col in 0..c_count {
            let cell_bg = if let Some(cell) = screen.cell(row, col) {
                let mut fg = vt100_color_to_egui(cell.fgcolor(), default_fg, is_light_terminal);
                let mut bg = vt100_color_to_egui(cell.bgcolor(), bg_color, is_light_terminal);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                bg
            } else {
                bg_color
            };

            match span_start {
                Some((_s_col, s_bg)) if s_bg == cell_bg => {}
                Some((s_col, s_bg)) => {
                    if s_bg != bg_color {
                        let r = Rect::from_min_max(
                            Pos2::new(origin.x + (s_col as f32) * char_width, row_y),
                            Pos2::new(origin.x + (col as f32) * char_width + 0.5_f32, row_y + line_height + 0.5_f32),
                        );
                        painter.rect_filled(r, 0.0_f32, s_bg);
                    }
                    span_start = if cell_bg != bg_color {
                        Some((col, cell_bg))
                    } else {
                        None
                    };
                }
                None => {
                    if cell_bg != bg_color {
                        span_start = Some((col, cell_bg));
                    }
                }
            }
        }
        if let Some((s_col, s_bg)) = span_start {
            if s_bg != bg_color {
                let r = Rect::from_min_max(
                    Pos2::new(origin.x + (s_col as f32) * char_width, row_y),
                    Pos2::new(origin.x + (c_count as f32) * char_width + 0.5_f32, row_y + line_height + 0.5_f32),
                );
                painter.rect_filled(r, 0.0_f32, s_bg);
            }
        }

        // Pass 1.5: Selection highlight overlay (doc_row aware!)
        if emu.has_selection() {
            let mut sel_start_col: Option<u16> = None;
            for col in 0..c_count {
                let selected = emu.is_cell_selected(doc_row, col);
                match (sel_start_col, selected) {
                    (None, true) => sel_start_col = Some(col),
                    (Some(sc), false) => {
                        let r = Rect::from_min_max(
                            Pos2::new(origin.x + (sc as f32) * char_width, row_y),
                            Pos2::new(origin.x + (col as f32) * char_width, row_y + line_height),
                        );
                        painter.rect_filled(r, 0.0_f32, sel_color);
                        sel_start_col = None;
                    }
                    _ => {}
                }
            }
            if let Some(sc) = sel_start_col {
                let r = Rect::from_min_max(
                    Pos2::new(origin.x + (sc as f32) * char_width, row_y),
                    Pos2::new(origin.x + (c_count as f32) * char_width, row_y + line_height),
                );
                painter.rect_filled(r, 0.0_f32, sel_color);
            }
        }

        // Pass 2: Exact cell-locked character rendering
        for col in 0..c_count {
            if let Some(cell) = screen.cell(row, col) {
                if cell.is_wide_continuation() || !cell.has_contents() {
                    continue;
                }
                let ch_str = cell.contents();
                if ch_str == " " {
                    continue;
                }

                let mut fg = vt100_color_to_egui(cell.fgcolor(), default_fg, is_light_terminal);
                let mut bg = vt100_color_to_egui(cell.bgcolor(), bg_color, is_light_terminal);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.bold() && cell.fgcolor() == vt100::Color::Default {
                    fg = if is_light_terminal {
                        Color32::from_rgb(15, 23, 42)
                    } else {
                        Color32::WHITE
                    };
                }

                let cell_x = origin.x + (col as f32) * char_width;
                painter.text(
                    Pos2::new(cell_x, row_y),
                    Align2::LEFT_TOP,
                    ch_str,
                    font_id.clone(),
                    fg,
                );
            }
        }
    }

    // Subtle scrollback indicator bar on the right edge when history exists
    if total_sb_len > 0 {
        let total_lines = (total_sb_len + r_count as usize) as f32;
        let visible_frac = ((r_count as f32) / total_lines).clamp(0.05_f32, 1.0_f32);
        let track_h = rect.height() - 8.0_f32;
        let thumb_h = (track_h * visible_frac).max(20.0_f32);
        let top_frac = ((total_sb_len as f32 - current_sb as f32) / (total_sb_len as f32)).clamp(0.0_f32, 1.0_f32);
        let thumb_y = rect.min.y + 4.0_f32 + (track_h - thumb_h) * top_frac;
        let thumb_rect = Rect::from_min_size(
            Pos2::new(rect.max.x - 6.0_f32, thumb_y),
            Vec2::new(4.0_f32, thumb_h),
        );
        let thumb_col = if is_light_terminal {
            Color32::from_rgba_unmultiplied(100, 116, 139, 130)
        } else {
            Color32::from_rgba_unmultiplied(168, 85, 247, 130)
        };
        painter.rect_filled(thumb_rect, 2.0_f32, thumb_col);
    }

    // Draw Terminal Cursor when not scrolled back and cursor not hidden
    if current_sb == 0 && !screen.hide_cursor() {
        let (cur_row, cur_col) = screen.cursor_position();
        let cur_rect = Rect::from_min_size(
            Pos2::new(
                origin.x + (cur_col as f32) * char_width,
                origin.y + (cur_row as f32) * line_height,
            ),
            Vec2::new(char_width, line_height),
        );
        if is_focused {
            painter.rect_filled(
                cur_rect,
                1.0_f32,
                Color32::from_rgba_unmultiplied(cursor_color.r(), cursor_color.g(), cursor_color.b(), 140),
            );
            painter.rect_stroke(cur_rect, 1.0_f32, Stroke::new(1.0_f32, cursor_color));
        } else {
            painter.rect_stroke(cur_rect, 1.0_f32, Stroke::new(1.0_f32, cursor_color));
        }
    }

    actions
}

fn key_to_ctrl_byte(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(1),
        Key::B => Some(2),
        Key::C => Some(3),
        Key::D => Some(4),
        Key::E => Some(5),
        Key::F => Some(6),
        Key::G => Some(7),
        Key::H => Some(8),
        Key::I => Some(9),
        Key::J => Some(10),
        Key::K => Some(11),
        Key::L => Some(12),
        Key::M => Some(13),
        Key::N => Some(14),
        Key::O => Some(15),
        Key::P => Some(16),
        Key::Q => Some(17),
        Key::R => Some(18),
        Key::S => Some(19),
        Key::T => Some(20),
        Key::U => Some(21),
        Key::V => None,
        Key::W => Some(23),
        Key::X => Some(24),
        Key::Y => Some(25),
        Key::Z => Some(26),
        _ => None,
    }
}
