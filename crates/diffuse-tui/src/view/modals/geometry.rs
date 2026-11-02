//! Modal viewport geometry calculations and ANSI style parsing.

use std::mem;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::width::char_width;

/// Compute a centered rectangle within an area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Compute the centered area for standard dialog modals (file finder, key
/// bindings).
///
/// On large screens, the dialog occupies 50% of width and 60% of height,
/// centered horizontally and vertically. On small screens, it expands up to the
/// full screen dimensions.
#[must_use]
pub fn standard_modal_area(area: Rect) -> Rect {
    let modal_w = ((area.width as u32 * 50) / 100)
        .max(50)
        .min(area.width as u32) as u16;
    let modal_h = ((area.height as u32 * 60) / 100)
        .max(16)
        .min(area.height as u32) as u16;
    centered_rect(modal_w, modal_h, area)
}

/// Compute the centered area for the file finder modal.
#[must_use]
pub fn finder_modal_area(area: Rect) -> Rect {
    standard_modal_area(area)
}

/// Compute the centered area for the key-bindings help modal.
#[must_use]
pub fn help_modal_area(area: Rect) -> Rect {
    standard_modal_area(area)
}

/// Convert an ANSI SGR styled string into a Ratatui `Line`.
pub fn ansi_to_ratatui_line(s: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut cur_style = Style::default();
    let mut text_buf = String::new();
    let mut col = 0usize;

    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            let mut code = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_alphabetic() {
                    let cmd = chars.next().unwrap();
                    if cmd == 'm' {
                        if !text_buf.is_empty() {
                            spans.push(Span::styled(mem::take(&mut text_buf), cur_style));
                        }
                        cur_style = parse_sgr_to_style(&code, cur_style);
                    }
                    break;
                } else {
                    code.push(chars.next().unwrap());
                }
            }
        } else if c == '\t' {
            let spaces = 4 - (col % 4);
            for _ in 0..spaces {
                text_buf.push(' ');
            }
            col += spaces;
        } else {
            col += char_width(c);
            text_buf.push(c);
        }
    }
    if !text_buf.is_empty() {
        spans.push(Span::styled(text_buf, cur_style));
    }
    Line::from(spans)
}

fn parse_sgr_to_style(code: &str, mut style: Style) -> Style {
    if code.is_empty() || code == "0" {
        return Style::default();
    }
    let parts: Vec<u8> = code
        .split(';')
        .filter_map(|p| p.parse::<u8>().ok())
        .collect();
    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(Color::Indexed(parts[i] - 30)),
            38 => {
                if i + 2 < parts.len() && parts[i + 1] == 5 {
                    style = style.fg(Color::Indexed(parts[i + 2]));
                    i += 2;
                } else if i + 4 < parts.len() && parts[i + 1] == 2 {
                    style = style.fg(Color::Rgb(parts[i + 2], parts[i + 3], parts[i + 4]));
                    i += 4;
                }
            }
            39 => style.fg = None,
            40..=47 => style = style.bg(Color::Indexed(parts[i] - 40)),
            48 => {
                if i + 2 < parts.len() && parts[i + 1] == 5 {
                    style = style.bg(Color::Indexed(parts[i + 2]));
                    i += 2;
                } else if i + 4 < parts.len() && parts[i + 1] == 2 {
                    style = style.bg(Color::Rgb(parts[i + 2], parts[i + 3], parts[i + 4]));
                    i += 4;
                }
            }
            49 => style.bg = None,
            90..=97 => style = style.fg(Color::Indexed(parts[i] - 90 + 8)),
            100..=107 => style = style.bg(Color::Indexed(parts[i] - 100 + 8)),
            _ => {}
        }
        i += 1;
    }
    style
}
