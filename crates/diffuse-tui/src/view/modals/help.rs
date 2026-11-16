//! Key bindings help dialog popup and full-screen overlay rendering.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::geometry::standard_modal_area;
use crate::render::DiffuseTheme;

/// Canonical key bindings reference displayed in the `?` help dialog.
pub const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j · ↓ / k · ↑", "next / prev line"),
    ("J · } / K · {", "next / prev hunk"),
    ("Space · PgDn · Ctrl+d · Ctrl+f", "page down"),
    ("PgUp · Ctrl+u · Ctrl+b", "page up"),
    ("G · End / g · Home", "bottom / top"),
    ("<num>g · <num>G", "jump to line number"),
    ("M · %", "jump to opposing moved line"),
    ("n · ] · Tab / p · [ · S-Tab", "next / prev file"),
    ("@", "find file in diff"),
    ("Enter", "toggle file collapse / jump opposing"),
    ("l · → / h · ←", "right / left scroll"),
    ("^ · 0 / $", "start / end of line"),
    ("w", "toggle word diff"),
    ("y", "toggle syntax highlight"),
    ("R", "toggle raw diff view"),
    ("#", "toggle old-line gutter"),
    (r"\ · | · b · F3", "toggle side-by-side diff"),
    ("r · Ctrl+r", "force reload diff"),
    ("t", "toggle staged / unstaged"),
    ("s / u", "stage / unstage hunk"),
    ("U", "unstage entire file"),
    ("x", "cycle note status / discard hunk"),
    ("X", "discard entire file"),
    ("v · V", "select lines / anchor band"),
    ("o · O", "open in editor"),
    ("T · Ctrl+t · F2", "toggle light / dark theme"),
    ("a · +", "expand all context lines"),
    ("c / C", "reset hunk / all context"),
    ("m", "new note"),
    ("d", "delete note"),
    ("z", "toggle resolved notes"),
    ("S", "toggle note original source"),
    ("?", "toggle help"),
    ("q · Ctrl+c", "quit"),
];

/// Wrap a text string into multiple lines at word boundaries up to `max_width`.
pub fn wrap_words(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut cur_line = String::new();

    for word in text.split_whitespace() {
        if cur_line.is_empty() {
            if word.len() <= max_width {
                cur_line.push_str(word);
            } else {
                let mut w = word;
                while w.len() > max_width {
                    lines.push(w[..max_width].to_string());
                    w = &w[max_width..];
                }
                cur_line.push_str(w);
            }
        } else if cur_line.len() + 1 + word.len() <= max_width {
            cur_line.push(' ');
            cur_line.push_str(word);
        } else {
            lines.push(cur_line);
            cur_line = String::new();
            if word.len() <= max_width {
                cur_line.push_str(word);
            } else {
                let mut w = word;
                while w.len() > max_width {
                    lines.push(w[..max_width].to_string());
                    w = &w[max_width..];
                }
                cur_line.push_str(w);
            }
        }
    }

    if !cur_line.is_empty() {
        lines.push(cur_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render the Help Modal as a centered Ratatui widget popup with rounded
/// borders.
pub fn render_help_modal_widget(f: &mut Frame, theme: &DiffuseTheme, area: Rect) {
    let entries = HELP_ENTRIES;

    let modal_area = standard_modal_area(area);
    f.render_widget(Clear, modal_area);

    let p = &theme.palette.ui;
    let title = if modal_area.width < 38 {
        " KEYS · ?/Esc "
    } else {
        " KEY BINDINGS · ? / Esc close "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(
            Style::default()
                .fg(crate::view::to_color(p.modal_title))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(crate::view::to_color(p.modal_border)))
        .style(
            Style::default()
                .bg(crate::view::to_color(p.bg_modal))
                .fg(crate::view::to_color(p.modal_text)),
        );

    let inner_w = modal_area.width.saturating_sub(2) as usize;
    let key_w = if inner_w < 44 {
        16
    } else if inner_w < 54 {
        22
    } else {
        32
    };

    let mut lines = Vec::new();
    for (key, desc) in entries {
        let desc_w = inner_w.saturating_sub(key_w + 2);
        let wrapped_keys = wrap_words(key, key_w);
        let wrapped_descs = wrap_words(desc, desc_w);
        let num_rows = wrapped_keys.len().max(wrapped_descs.len());

        for i in 0..num_rows {
            let k = wrapped_keys.get(i).map(|s| s.as_str()).unwrap_or("");
            let d = wrapped_descs.get(i).map(|s| s.as_str()).unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<key_w$} ", k),
                    Style::default()
                        .fg(crate::view::to_color(p.accent))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    d.to_string(),
                    Style::default().fg(crate::view::to_color(p.modal_text)),
                ),
            ]));
        }
    }

    let p_widget = Paragraph::new(lines).block(block);
    f.render_widget(p_widget, modal_area);
}
