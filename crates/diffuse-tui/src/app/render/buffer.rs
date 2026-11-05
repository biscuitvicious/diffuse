//! Serialization of Ratatui in-memory frame buffers to ANSI and plain text.

use std::fmt::Write;

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
};

use crate::app::AppState;

/// Render ANSI escape bytes as the literal text `\x1b` so diagnostic messages
/// stay readable instead of emitting raw control codes.
#[must_use]
pub fn escape_ansi_for_diagnostic(s: &str) -> String {
    s.replace('\x1b', "\\x1b")
}

fn write_fg_ansi<W: Write>(out: &mut W, c: Color) -> std::fmt::Result {
    match c {
        Color::Reset => write!(out, "\x1b[39m"),
        Color::Black => write!(out, "\x1b[30m"),
        Color::Red => write!(out, "\x1b[31m"),
        Color::Green => write!(out, "\x1b[32m"),
        Color::Yellow => write!(out, "\x1b[33m"),
        Color::Blue => write!(out, "\x1b[34m"),
        Color::Magenta => write!(out, "\x1b[35m"),
        Color::Cyan => write!(out, "\x1b[36m"),
        Color::Gray => write!(out, "\x1b[37m"),
        Color::DarkGray => write!(out, "\x1b[90m"),
        Color::LightRed => write!(out, "\x1b[91m"),
        Color::LightGreen => write!(out, "\x1b[92m"),
        Color::LightYellow => write!(out, "\x1b[93m"),
        Color::LightBlue => write!(out, "\x1b[94m"),
        Color::LightMagenta => write!(out, "\x1b[95m"),
        Color::LightCyan => write!(out, "\x1b[96m"),
        Color::White => write!(out, "\x1b[97m"),
        Color::Indexed(n) => write!(out, "\x1b[38;5;{n}m"),
        Color::Rgb(r, g, b) => write!(out, "\x1b[38;2;{r};{g};{b}m"),
    }
}

fn write_bg_ansi<W: Write>(out: &mut W, c: Color) -> std::fmt::Result {
    match c {
        Color::Reset => write!(out, "\x1b[49m"),
        Color::Black => write!(out, "\x1b[40m"),
        Color::Red => write!(out, "\x1b[41m"),
        Color::Green => write!(out, "\x1b[42m"),
        Color::Yellow => write!(out, "\x1b[43m"),
        Color::Blue => write!(out, "\x1b[44m"),
        Color::Magenta => write!(out, "\x1b[45m"),
        Color::Cyan => write!(out, "\x1b[46m"),
        Color::Gray => write!(out, "\x1b[47m"),
        Color::DarkGray => write!(out, "\x1b[100m"),
        Color::LightRed => write!(out, "\x1b[101m"),
        Color::LightGreen => write!(out, "\x1b[102m"),
        Color::LightYellow => write!(out, "\x1b[103m"),
        Color::LightBlue => write!(out, "\x1b[104m"),
        Color::LightMagenta => write!(out, "\x1b[105m"),
        Color::LightCyan => write!(out, "\x1b[106m"),
        Color::White => write!(out, "\x1b[107m"),
        Color::Indexed(n) => write!(out, "\x1b[48;5;{n}m"),
        Color::Rgb(r, g, b) => write!(out, "\x1b[48;2;{r};{g};{b}m"),
    }
}

fn write_mod_ansi<W: Write>(out: &mut W, m: Modifier) -> std::fmt::Result {
    if m.contains(Modifier::BOLD) {
        write!(out, "\x1b[1m")?;
    }
    if m.contains(Modifier::DIM) {
        write!(out, "\x1b[2m")?;
    }
    if m.contains(Modifier::ITALIC) {
        write!(out, "\x1b[3m")?;
    }
    if m.contains(Modifier::UNDERLINED) {
        write!(out, "\x1b[4m")?;
    }
    if m.contains(Modifier::REVERSED) {
        write!(out, "\x1b[7m")?;
    }
    if m.contains(Modifier::CROSSED_OUT) {
        write!(out, "\x1b[9m")?;
    }
    Ok(())
}

#[allow(deprecated)]
fn is_cell_skipped(cell: &ratatui::buffer::Cell) -> bool {
    cell.skip || cell.diff_option == ratatui::buffer::CellDiffOption::Skip
}

/// Serialize a Ratatui [`Buffer`] into an ANSI-escaped string with newline
/// delimiters.
#[must_use]
pub fn buffer_to_ansi(buffer: &Buffer) -> String {
    let mut out =
        String::with_capacity(buffer.area.width as usize * buffer.area.height as usize * 4);
    let mut cur_fg = Color::Reset;
    let mut cur_bg = Color::Reset;
    let mut cur_mod = Modifier::empty();

    for y in 0..buffer.area.height {
        if y > 0 {
            out.push_str("\r\n");
            cur_fg = Color::Reset;
            cur_bg = Color::Reset;
            cur_mod = Modifier::empty();
        }
        let mut skip_cols = 0;
        for x in 0..buffer.area.width {
            if skip_cols > 0 {
                skip_cols -= 1;
                continue;
            }
            let cell = &buffer[(x, y)];
            if is_cell_skipped(cell) || cell.symbol().is_empty() {
                continue;
            }
            let sym_width = crate::render::width::display_width(cell.symbol());
            if sym_width > 1 {
                skip_cols = sym_width - 1;
            }
            if cell.fg != cur_fg || cell.bg != cur_bg || cell.modifier != cur_mod {
                if (cur_mod - cell.modifier) != Modifier::empty() {
                    out.push_str("\x1b[0m");
                    cur_fg = Color::Reset;
                    cur_bg = Color::Reset;
                    cur_mod = Modifier::empty();
                }
                if cell.bg != cur_bg {
                    write_bg_ansi(&mut out, cell.bg).unwrap();
                    cur_bg = cell.bg;
                }
                if cell.fg != cur_fg {
                    write_fg_ansi(&mut out, cell.fg).unwrap();
                    cur_fg = cell.fg;
                }
                let added_mods = cell.modifier - cur_mod;
                if !added_mods.is_empty() {
                    write_mod_ansi(&mut out, added_mods).unwrap();
                    cur_mod |= added_mods;
                }
            }
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m");
    }
    crate::render::ansi::optimize_ansi(&out).into_owned()
}

/// Serialize a Ratatui [`Buffer`] into plain text without ANSI escape codes.
#[must_use]
pub fn buffer_to_plain(buffer: &Buffer) -> String {
    let mut out = String::with_capacity(buffer.area.width as usize * buffer.area.height as usize);
    for y in 0..buffer.area.height {
        if y > 0 {
            out.push('\n');
        }
        let mut skip_cols = 0;
        for x in 0..buffer.area.width {
            if skip_cols > 0 {
                skip_cols -= 1;
                continue;
            }
            let cell = &buffer[(x, y)];
            if is_cell_skipped(cell) {
                continue;
            }
            let sym_width = crate::render::width::display_width(cell.symbol());
            if sym_width > 1 {
                skip_cols = sym_width - 1;
            }
            out.push_str(cell.symbol());
        }
    }
    out
}

impl AppState {
    /// Render the visual frame to an in-memory Ratatui [`Buffer`].
    pub fn render_to_buffer(&mut self, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("in-memory test terminal must succeed");
        terminal
            .draw(|f| {
                self.render_ratatui_frame(f);
            })
            .expect("render_ratatui_frame must succeed");
        terminal.backend().buffer().clone()
    }

    /// Render visual screen frame as an ANSI string by drawing through the
    /// canonical Ratatui pipeline.
    pub fn render_frame(&mut self, width: usize, height: usize) -> String {
        let buf = self.render_to_buffer(width as u16, height as u16);
        buffer_to_ansi(&buf)
    }

    /// Render visual screen frame as plain text (no ANSI escape codes).
    pub fn render_plain_frame(&mut self, width: usize, height: usize) -> String {
        let buf = self.render_to_buffer(width as u16, height as u16);
        buffer_to_plain(&buf)
    }

    /// Send a key press event into the application.
    pub fn send_key(&mut self, code: crossterm::event::KeyCode) {
        self.send_key_with_modifiers(code, crossterm::event::KeyModifiers::NONE);
    }

    /// Send a key press event with modifiers into the application.
    pub fn send_key_with_modifiers(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) {
        crate::event::handle_key_event(self, crossterm::event::KeyEvent::new(code, modifiers));
    }

    /// Send a character key press into the application.
    pub fn send_char(&mut self, c: char) {
        let (code, mods) = if c.is_uppercase() {
            (
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::SHIFT,
            )
        } else {
            (
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            )
        };
        self.send_key_with_modifiers(code, mods);
    }

    /// Send a sequence of keystrokes into the application.
    pub fn send_keys(&mut self, keys: &str) {
        for c in keys.chars() {
            match c {
                '\n' | '\r' => self.send_key(crossterm::event::KeyCode::Enter),
                '\t' => self.send_key(crossterm::event::KeyCode::Tab),
                '\x08' => self.send_key(crossterm::event::KeyCode::Backspace),
                '\x1b' => self.send_key(crossterm::event::KeyCode::Esc),
                ch => self.send_char(ch),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{layout::Rect, style::Style};

    #[test]
    fn test_buffer_to_plain_and_ansi_multi_width() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        buf.set_stringn(
            0,
            0,
            "💬 hi",
            10,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        buf.set_stringn(0, 1, "row 2", 10, Style::default().bg(Color::Blue));

        let plain = buffer_to_plain(&buf);
        let plain_lines: Vec<&str> = plain.lines().collect();
        assert_eq!(plain_lines.len(), 2);
        assert_eq!(plain_lines[0].trim_end(), "💬 hi");
        assert_eq!(plain_lines[1].trim_end(), "row 2");

        let ansi = buffer_to_ansi(&buf);
        assert!(ansi.contains("\x1b[33m"));
        assert!(ansi.contains("\x1b[1m"));
        assert!(ansi.contains("💬 hi"));
        assert!(ansi.contains("\x1b[44m"));
        assert!(ansi.contains("row 2"));
    }

    #[test]
    fn test_buffer_interactive_keystroke_helpers() {
        let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,2 +1,3 @@
 line 1
+added line
 line 2
"#;
        let output = crate::core::pipeline::run_pipeline_full(
            diff,
            None,
            None,
            crate::core::PipelineOptions::default(),
        );
        let mut state = AppState::new(
            output,
            "demo_repo".into(),
            std::path::PathBuf::from("/tmp/diffuse_test_notes.json"),
            crate::render::DiffuseTheme::dark(),
        );

        // Verify initial screen via buffer and plain frame
        let initial_plain = state.render_plain_frame(80, 24);
        assert!(initial_plain.contains("demo_repo"));
        assert!(!initial_plain.contains("KEY BINDINGS"));

        // Toggle help modal via simulated '?' key press
        state.send_char('?');
        let help_plain = state.render_plain_frame(80, 24);
        assert!(help_plain.contains("KEY BINDINGS"));

        // Close help modal via Esc key press
        state.send_key(crossterm::event::KeyCode::Esc);
        let closed_plain = state.render_plain_frame(80, 24);
        assert!(!closed_plain.contains("KEY BINDINGS"));

        // Verify buffer dimensions and structured cell queries
        let buf = state.render_to_buffer(80, 24);
        assert_eq!(buf.area.width, 80);
        assert_eq!(buf.area.height, 24);
        // Header bar background color on row 0
        assert_ne!(buf[(0, 0)].bg, Color::Reset);
    }
}
