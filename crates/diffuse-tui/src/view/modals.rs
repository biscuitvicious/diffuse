use std::borrow::Cow;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use super::header::{truncate_pad, visible_width};
use crate::{
    core::wire::PipelineOutput,
    finder::FileFinderState,
    render::{DiffuseTheme, render_row_segment, styled_prefix},
};

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

/// Render the `?` key-bindings modal as full-screen overlay lines.
pub fn render_help_modal(width: usize, height: usize) -> Vec<String> {
    let key_w = if width < 48 {
        16
    } else if width < 58 {
        22
    } else {
        32
    };

    let entries: &[(&str, &str)] = &[
        ("j · ↓ / k · ↑", "next / prev line"),
        ("J · } / K · {", "next / prev hunk"),
        ("Space · PgDn · Ctrl+d · Ctrl+f", "page down"),
        ("PgUp · Ctrl+u · Ctrl+b", "page up"),
        ("G · End / g · Home", "bottom / top"),
        ("n · ] / p · [", "next / prev file"),
        ("@", "find file in diff"),
        ("l · → / h · ←", "right / left scroll"),
        ("w", "toggle word diff"),
        ("y", "toggle syntax highlight"),
        ("R", "toggle raw diff view"),
        ("a · +", "expand all context lines"),
        ("c / C", "reset hunk / all context"),
        ("m", "new note"),
        ("d", "delete note"),
        ("?", "toggle help"),
        ("q · Ctrl+c", "quit"),
    ];

    let mut lines = Vec::new();
    lines.push(format!(
        "\x1b[48;5;236m\x1b[1m\x1b[38;5;231m{}\x1b[0m",
        truncate_pad("  KEY BINDINGS", width)
    ));
    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;240m{}\x1b[0m",
        "─".repeat(width)
    ));

    for (key, desc) in entries {
        let desc_w = width.saturating_sub(key_w + 4);
        let wrapped_keys = wrap_words(key, key_w);
        let wrapped_descs = wrap_words(desc, desc_w);
        let num_rows = wrapped_keys.len().max(wrapped_descs.len());

        for i in 0..num_rows {
            let k = wrapped_keys.get(i).map(|s| s.as_str()).unwrap_or("");
            let d = wrapped_descs.get(i).map(|s| s.as_str()).unwrap_or("");
            let k_str = truncate_pad(k, key_w);
            let line_raw = format!("  {k_str} {d}");
            lines.push(format!(
                "\x1b[48;5;236m\x1b[38;5;255m{}\x1b[0m",
                truncate_pad(&line_raw, width)
            ));
        }
    }

    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;244m{}\x1b[0m",
        truncate_pad("  ? · Esc to close", width)
    ));

    while lines.len() < height {
        lines.push(format!("\x1b[48;5;236m{}\x1b[0m", " ".repeat(width)));
    }

    lines
}

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

/// Render the Help Modal directly as a full-screen Ratatui widget popup with
/// rounded borders.
pub fn render_help_modal_widget(f: &mut Frame, area: Rect) {
    let entries: &[(&str, &str)] = &[
        ("j · ↓ / k · ↑", "next / prev line"),
        ("J · } / K · {", "next / prev hunk"),
        ("Space · PgDn · Ctrl+d · Ctrl+f", "page down"),
        ("PgUp · Ctrl+u · Ctrl+b", "page up"),
        ("G · End / g · Home", "bottom / top"),
        ("n · ] / p · [", "next / prev file"),
        ("@", "find file in diff"),
        ("l · → / h · ←", "right / left scroll"),
        ("w", "toggle word diff"),
        ("y", "toggle syntax highlight"),
        ("R", "toggle raw diff view"),
        ("a · +", "expand all context lines"),
        ("c / C", "reset hunk / all context"),
        ("m", "new note"),
        ("d", "delete note"),
        ("?", "toggle help"),
        ("q · Ctrl+c", "quit"),
    ];

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" KEY BINDINGS · ? / Esc close ")
        .title_style(
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Indexed(240)))
        .style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Indexed(252)),
        );

    let inner_w = area.width.saturating_sub(2) as usize;
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
                        .fg(Color::Indexed(51))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.to_string(), Style::default().fg(Color::Indexed(252))),
            ]));
        }
    }

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

/// Render the fuzzy file finder popup dialog covering the full screen with a
/// top finder list and bottom diff preview.
pub fn render_finder_modal(
    output: &PipelineOutput,
    finder: &FileFinderState,
    width: usize,
    height: usize,
) -> Vec<String> {
    let mut lines = Vec::new();

    let header_info = if width < 50 {
        format!(
            "  FIND FILE ({}/{}) · ↑/↓ · Enter · Esc",
            if finder.matches.is_empty() {
                0
            } else {
                finder.selected_idx + 1
            },
            finder.matches.len()
        )
    } else {
        format!(
            "  FIND FILE ({}/{}) · ↑/↓ nav · Enter jump · Esc close",
            if finder.matches.is_empty() {
                0
            } else {
                finder.selected_idx + 1
            },
            finder.matches.len()
        )
    };
    lines.push(format!(
        "\x1b[48;5;236m\x1b[1m\x1b[38;5;231m{}\x1b[0m",
        truncate_pad(&header_info, width)
    ));
    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;240m{}\x1b[0m",
        "─".repeat(width)
    ));

    let prompt = format!("  @ {}█", finder.query);
    lines.push(format!(
        "\x1b[48;5;236m\x1b[1m\x1b[38;5;231m{}\x1b[0m",
        truncate_pad(&prompt, width)
    ));
    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;240m{}\x1b[0m",
        "─".repeat(width)
    ));

    let max_visible = 6.min(height.saturating_sub(10).max(3));
    let total = finder.matches.len();
    if total == 0 {
        lines.push(format!(
            "\x1b[48;5;236m\x1b[38;5;240m{}\x1b[0m",
            truncate_pad("  No matching files in diff", width)
        ));
    } else {
        let start = if total <= max_visible || finder.selected_idx < max_visible {
            0
        } else {
            finder.selected_idx + 1 - max_visible
        };
        let end = (start + max_visible).min(total);

        for (i, m) in finder.matches[start..end].iter().enumerate() {
            let actual_idx = start + i;
            let is_sel = actual_idx == finder.selected_idx;
            let f = &output.files[m.file_idx];
            let prefix = if is_sel { "▶ " } else { "  " };
            let stats = format!("+{} -{} ~{}", f.counts.adds, f.counts.dels, f.counts.mods);

            let bg_code = if is_sel {
                "\x1b[48;5;239m"
            } else {
                "\x1b[48;5;236m"
            };
            let stats_color = if is_sel {
                "\x1b[38;5;248m"
            } else {
                "\x1b[38;5;242m"
            };
            let prefix_color = if is_sel {
                "\x1b[1m\x1b[38;5;120m"
            } else {
                "\x1b[38;5;244m"
            };

            let prefix_w = visible_width(prefix);
            let stats_w = stats.len();
            let right_margin = 2;
            let avail_path_w = width.saturating_sub(prefix_w + 2 + stats_w + right_margin);

            let (path_display, path_vis_w) =
                format_path_matches(&m.path, &m.match_indices, avail_path_w, is_sel);

            let used_w = prefix_w + path_vis_w + stats_w + right_margin;
            let pad_w = width.saturating_sub(used_w);
            let padding = " ".repeat(pad_w);

            let line_raw = format!(
                "{bg_code}{prefix_color}{prefix}{path_display}{bg_code}{padding}{stats_color}{stats}  \x1b[0m"
            );
            lines.push(line_raw);
        }
    }

    // Horizontal ruler dividing finder matches from the live diff preview
    lines.push(format!(
        "\x1b[48;5;236m\x1b[38;5;240m{}\x1b[0m",
        "─".repeat(width)
    ));

    // Bottom Preview Section (rendered exactly like a real diff, without the
    // filename bar)
    if let Some(selected_match) = finder.matches.get(finder.selected_idx) {
        let f = &output.files[selected_match.file_idx];

        let remaining = height.saturating_sub(lines.len());
        if f.rows.is_empty() {
            if remaining > 0 {
                lines.push(format!(
                    "\x1b[48;5;236m\x1b[38;5;242m{}\x1b[0m",
                    truncate_pad("      (No diff rows to display)", width)
                ));
            }
        } else {
            let text_cols = width.saturating_sub(6).max(1);
            for r in f.rows.iter().take(remaining) {
                let pfx = styled_prefix(r.kind, r.new_line, r.old_line, r.moved, &r.runs, true);
                let rendered_row = render_row_segment(
                    &r.text,
                    r.del_text.as_deref(),
                    &r.spans,
                    r.kind,
                    r.moved,
                    &r.runs,
                    0,
                    text_cols,
                    &pfx,
                    false,
                    false,
                    0,
                    true,
                    true,
                    &DiffuseTheme::MAGIC,
                );
                let vis_w = visible_width(&rendered_row);
                let pad_str = if vis_w < width {
                    " ".repeat(width - vis_w)
                } else {
                    String::new()
                };
                lines.push(format!("{rendered_row}{pad_str}\x1b[0m"));
            }
        }
    }

    while lines.len() < height {
        lines.push(format!("\x1b[48;5;236m{}\x1b[0m", " ".repeat(width)));
    }

    lines
}

/// Render the File Finder directly as a Ratatui widget popup dialog with top
/// finder and bottom preview.
pub fn render_finder_modal_widget(
    f: &mut Frame,
    output: &PipelineOutput,
    finder: &FileFinderState,
    area: Rect,
) {
    f.render_widget(Clear, area);

    let file_list_h = (area.height / 5).max(3);
    let finder_h = (file_list_h + 4).min(area.height.saturating_sub(4)).max(4);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(finder_h), Constraint::Min(4)])
        .split(area);

    let finder_area = chunks[0];
    let preview_area = chunks[1];

    let title = format!(
        " FIND FILE ({}/{}) · ↑/↓ nav · Enter jump · Esc close ",
        if finder.matches.is_empty() {
            0
        } else {
            finder.selected_idx + 1
        },
        finder.matches.len()
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Indexed(252)),
        );

    let inner_w = finder_area.width.saturating_sub(2) as usize;

    let mut finder_lines = Vec::new();
    finder_lines.push(Line::from(vec![
        Span::styled(
            "@ ",
            Style::default()
                .fg(Color::Indexed(220))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            finder.query.clone(),
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(Color::Indexed(51))),
    ]));
    finder_lines.push(Line::from(Span::styled(
        "─".repeat(inner_w),
        Style::default().fg(Color::Indexed(240)),
    )));

    let max_visible = (finder_area.height.saturating_sub(4) as usize).max(1);
    let total = finder.matches.len();
    if total == 0 {
        finder_lines.push(Line::from(Span::styled(
            "  No matching files in diff",
            Style::default().fg(Color::Indexed(240)),
        )));
    } else {
        let start = if total <= max_visible || finder.selected_idx < max_visible {
            0
        } else {
            finder.selected_idx + 1 - max_visible
        };
        let end = (start + max_visible).min(total);

        for (i, m) in finder.matches[start..end].iter().enumerate() {
            let actual_idx = start + i;
            let is_sel = actual_idx == finder.selected_idx;
            let file = &output.files[m.file_idx];
            let prefix = if is_sel { "▶ " } else { "  " };
            let stats = format!(
                "+{} -{} ~{}",
                file.counts.adds, file.counts.dels, file.counts.mods
            );

            let mut spans = vec![Span::styled(
                prefix,
                if is_sel {
                    Style::default()
                        .fg(Color::Indexed(120))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Indexed(244))
                },
            )];

            let prefix_w = unicode_width::UnicodeWidthStr::width(prefix);
            let stats_w = stats.len();
            let avail_path_w = inner_w.saturating_sub(prefix_w + 2 + stats_w);
            let path_spans =
                format_path_matches_spans(&m.path, &m.match_indices, avail_path_w, is_sel);
            let path_len: usize = path_spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            spans.extend(path_spans);

            let bg_color = if is_sel {
                Color::Indexed(239)
            } else {
                Color::Indexed(236)
            };
            let pad_style = Style::default().bg(bg_color);

            let pad_w = inner_w.saturating_sub(prefix_w + path_len + stats_w);
            spans.push(Span::styled(" ".repeat(pad_w), pad_style));
            spans.push(Span::styled(
                stats,
                if is_sel {
                    Style::default()
                        .fg(Color::Indexed(248))
                        .bg(Color::Indexed(239))
                } else {
                    Style::default()
                        .fg(Color::Indexed(242))
                        .bg(Color::Indexed(236))
                },
            ));

            let mut line = Line::from(spans);
            if is_sel {
                line = line.style(Style::default().bg(Color::Indexed(239)));
            }
            finder_lines.push(line);
        }
    }

    let finder_p = Paragraph::new(finder_lines).block(block);
    f.render_widget(finder_p, finder_area);

    // 2. Preview Box
    let preview_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" PREVIEW ")
        .style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Indexed(252)),
        );

    let mut preview_lines = Vec::new();
    if let Some(m) = finder.matches.get(finder.selected_idx) {
        let file = &output.files[m.file_idx];
        let max_preview_rows = preview_area.height.saturating_sub(2) as usize;
        let text_cols = (preview_area.width.saturating_sub(2) as usize)
            .saturating_sub(6)
            .max(1);
        if file.rows.is_empty() {
            preview_lines.push(Line::from(Span::styled(
                "  (No diff lines to preview)",
                Style::default().fg(Color::Indexed(242)),
            )));
        } else {
            for r in file.rows.iter().take(max_preview_rows) {
                let pfx = styled_prefix(r.kind, r.new_line, r.old_line, r.moved, &r.runs, true);
                let rendered_row = render_row_segment(
                    &r.text,
                    r.del_text.as_deref(),
                    &r.spans,
                    r.kind,
                    r.moved,
                    &r.runs,
                    0,
                    text_cols,
                    &pfx,
                    false,
                    false,
                    0,
                    true,
                    true,
                    &DiffuseTheme::MAGIC,
                );
                preview_lines.push(ansi_to_ratatui_line(&rendered_row));
            }
        }
    }

    let preview_p = Paragraph::new(preview_lines).block(preview_block);
    f.render_widget(preview_p, preview_area);
}

/// Render the Note Draft modal overlay with multi-line support and exact range
/// coordinates.
pub fn render_note_modal_widget(f: &mut Frame, loc: &str, draft: &str, area: Rect) {
    let margin_w = if area.width <= 60 { 2 } else { 4 };
    let modal_w = area.width.saturating_sub(margin_w).min(88);

    let min_h = 7.min(area.height);
    let max_h = area.height.saturating_sub(2).max(min_h);

    let inner_w = modal_w.saturating_sub(2).max(1) as usize;
    let split_lines: Vec<&str> = if draft.is_empty() {
        vec![""]
    } else {
        draft.split('\n').collect()
    };
    let total_lines = split_lines.len();

    let mut visual_rows_count = 0;
    for (idx, line_text) in split_lines.iter().enumerate() {
        let is_last = idx + 1 == total_lines;
        let char_count = line_text.chars().count() + if is_last { 1 } else { 0 };
        let rows = (char_count / inner_w)
            + if char_count % inner_w == 0 && char_count > 0 {
                0
            } else {
                1
            };
        visual_rows_count += rows.max(1);
    }
    let modal_h = (visual_rows_count as u16 + 6).clamp(min_h, max_h);
    let modal_area = centered_rect(modal_w, modal_h, area);

    f.render_widget(Clear, modal_area);

    let title = if modal_w < 50 {
        format!(" NOTE on {loc} · Ctrl+S · Esc ")
    } else {
        format!(" NOTE on {loc} · Ctrl+S Save · Enter ↵ · Esc Cancel ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Indexed(220)),
        );

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    let mut lines = Vec::new();
    for (idx, &line_text) in split_lines.iter().enumerate() {
        let is_last = idx + 1 == total_lines;
        if is_last {
            lines.push(Line::from(vec![
                Span::styled(
                    line_text.to_string(),
                    Style::default().fg(Color::Indexed(231)),
                ),
                Span::styled("█", Style::default().fg(Color::Indexed(51))),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                line_text.to_string(),
                Style::default().fg(Color::Indexed(231)),
            )));
        }
    }

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(Color::Indexed(236)));
    f.render_widget(p, inner);
}

/// Convert an ANSI SGR styled string into a Ratatui `Line`.
pub fn ansi_to_ratatui_line(s: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut cur_style = Style::default();
    let mut text_buf = String::new();

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
                            spans.push(Span::styled(std::mem::take(&mut text_buf), cur_style));
                        }
                        cur_style = parse_sgr_to_style(&code, cur_style);
                    }
                    break;
                } else {
                    code.push(chars.next().unwrap());
                }
            }
        } else {
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

fn format_path_matches(
    path: &str,
    indices: &[usize],
    max_w: usize,
    selected: bool,
) -> (String, usize) {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        (
            highlight_path_matches(&clean, indices, selected),
            char_count,
        )
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        let len = truncated.chars().count();
        (highlight_path_matches(&truncated, indices, selected), len)
    } else {
        let keep_chars = max_w - 1;
        let skip_chars = char_count - keep_chars;
        let mut actual_skip = skip_chars;
        if let Some(slash_offset) = clean
            .chars()
            .skip(skip_chars)
            .take(8)
            .position(|c| c == '/')
        {
            let candidate_skip = skip_chars + slash_offset;
            if char_count - candidate_skip < max_w {
                actual_skip = candidate_skip;
            }
        }

        let tail: String = clean.chars().skip(actual_skip).collect();
        let shifted_indices: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&idx| idx >= actual_skip)
            .map(|idx| idx - actual_skip)
            .collect();
        let default_color = if selected {
            "\x1b[1m\x1b[38;5;231m"
        } else {
            "\x1b[38;5;250m"
        };
        let tail_highlighted = highlight_path_matches(&tail, &shifted_indices, selected);
        (
            format!("{default_color}…{tail_highlighted}"),
            1 + tail.chars().count(),
        )
    }
}

fn format_path_matches_spans(
    path: &str,
    indices: &[usize],
    max_w: usize,
    selected: bool,
) -> Vec<Span<'static>> {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        highlight_path_matches_spans(&clean, indices, selected)
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        highlight_path_matches_spans(&truncated, indices, selected)
    } else {
        let keep_chars = max_w - 1;
        let skip_chars = char_count - keep_chars;
        let mut actual_skip = skip_chars;
        if let Some(slash_offset) = clean
            .chars()
            .skip(skip_chars)
            .take(8)
            .position(|c| c == '/')
        {
            let candidate_skip = skip_chars + slash_offset;
            if char_count - candidate_skip < max_w {
                actual_skip = candidate_skip;
            }
        }

        let tail: String = clean.chars().skip(actual_skip).collect();
        let shifted_indices: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&idx| idx >= actual_skip)
            .map(|idx| idx - actual_skip)
            .collect();
        let default_style = if selected {
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Indexed(250))
        };
        let mut spans = vec![Span::styled("…", default_style)];
        spans.extend(highlight_path_matches_spans(
            &tail,
            &shifted_indices,
            selected,
        ));
        spans
    }
}

fn highlight_path_matches_spans(
    path: &str,
    indices: &[usize],
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let default_style = if selected {
        Style::default()
            .fg(Color::Indexed(231))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Indexed(250))
    };
    let match_style = Style::default()
        .fg(Color::Indexed(51))
        .add_modifier(Modifier::BOLD);

    let mut cur_text = String::new();
    let mut cur_matched = None;

    for (i, ch) in path.char_indices() {
        let is_m = indices.contains(&i);
        if Some(is_m) != cur_matched && !cur_text.is_empty() {
            let style = if cur_matched.unwrap_or(false) {
                match_style
            } else {
                default_style
            };
            spans.push(Span::styled(std::mem::take(&mut cur_text), style));
        }
        cur_matched = Some(is_m);
        cur_text.push(ch);
    }
    if !cur_text.is_empty() {
        let style = if cur_matched.unwrap_or(false) {
            match_style
        } else {
            default_style
        };
        spans.push(Span::styled(cur_text, style));
    }
    spans
}

fn highlight_path_matches(path: &str, indices: &[usize], selected: bool) -> String {
    let mut out = String::new();
    let default_color = if selected {
        "\x1b[1m\x1b[38;5;231m"
    } else {
        "\x1b[38;5;250m"
    };
    let match_color = "\x1b[1m\x1b[38;5;51m";

    let mut cur_matched = None;
    for (i, ch) in path.char_indices() {
        let is_m = indices.contains(&i);
        if Some(is_m) != cur_matched {
            if is_m {
                out.push_str(match_color);
            } else {
                out.push_str(default_color);
            }
            cur_matched = Some(is_m);
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parse::RowKind;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn test_ratatui_help_modal_widget() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_help_modal_widget(f, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..24 {
            for x in 0..80 {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("KEY BINDINGS"));
        assert!(text.contains("next / prev hunk"));
        assert!(text.contains("? / Esc close"));
    }

    #[test]
    fn test_ratatui_help_modal_visual_layout() {
        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_help_modal_widget(f, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..30 {
            let mut line = String::new();
            for x in 0..50 {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let full_text = lines.join("\n");
        assert!(full_text.contains("KEY BINDINGS"));
        assert!(full_text.contains("j · ↓ / k · ↑"));
        assert!(full_text.contains("next / prev line"));
        assert!(full_text.contains("J · } / K · {"));
        assert!(full_text.contains("next / prev hunk"));
        assert!(full_text.contains("Space · PgDn ·"));
        assert!(full_text.contains("page down"));
        assert!(full_text.contains("Ctrl+d · Ctrl+f"));
        assert!(full_text.contains("PgUp · Ctrl+u ·"));
        assert!(full_text.contains("page up"));
        assert!(full_text.contains("Ctrl+b"));
        assert!(full_text.contains("G · End / g · Home"));
        assert!(full_text.contains("bottom / top"));
        assert!(full_text.contains("n · ] / p · ["));
        assert!(full_text.contains("next / prev file"));
        assert!(full_text.contains("l · → / h · ←"));
        assert!(full_text.contains("right / left scroll"));
        assert!(full_text.contains("a · +"));
        assert!(full_text.contains("expand all context"));
        assert!(full_text.contains("lines"));
        assert!(full_text.contains("c / C"));
        assert!(full_text.contains("reset hunk / all"));
        assert!(full_text.contains("context"));
        assert!(full_text.contains("m"));
        assert!(full_text.contains("new note"));
        assert!(full_text.contains("d"));
        assert!(full_text.contains("delete note"));
        assert!(full_text.contains("q · Ctrl+c"));
        assert!(full_text.contains("quit"));
        assert!(full_text.contains("? / Esc close"));
    }

    #[test]
    fn test_ratatui_finder_modal_widget() {
        let output = PipelineOutput {
            files: vec![],
            branch: "main".to_string(),
            untracked_skipped: vec![],
        };
        let mut finder = FileFinderState::new(&output.files, 0, 0);
        finder.query = "main".to_string();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..24 {
            for x in 0..80 {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("FIND FILE"));
        assert!(text.contains("@ main█"));
        assert!(text.contains("No matching files in diff"));
    }

    fn dummy_row(kind: RowKind, line: u32, text: &str) -> crate::core::wire::OutputRow {
        crate::core::wire::OutputRow {
            kind,
            new_line: Some(line),
            old_line: Some(line),
            text: text.to_string(),
            del_text: None,
            spans: crate::core::parse::Spans::default(),
            runs: vec![],
            wholesale: false,
            moved: false,
        }
    }

    fn sample_finder_output() -> PipelineOutput {
        use crate::core::{counts::FileCounts, wire::OutputFile};
        let file1 = OutputFile {
            path: "TODO.md".to_string(),
            status: "modified".to_string(),
            new_line_count: 2,
            old_line_count: 0,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 2,
                dels: 0,
                mods: 0,
                movs: 0,
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![
                dummy_row(RowKind::Add, 1, "+ First todo item"),
                dummy_row(RowKind::Add, 2, "+ Second todo item"),
            ],
        };
        let file2 = OutputFile {
            path: "crates/diffuse-tui/src/app/render.rs".to_string(),
            status: "modified".to_string(),
            new_line_count: 127,
            old_line_count: 295,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 127,
                dels: 295,
                mods: 2,
                movs: 0,
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![
                dummy_row(RowKind::Del, 10, "- old render line"),
                dummy_row(RowKind::Add, 10, "+ new render line"),
            ],
        };
        PipelineOutput {
            files: vec![file1, file2],
            branch: "main".to_string(),
            untracked_skipped: vec![],
        }
    }

    #[test]
    fn test_ratatui_finder_modal_visual_layout() {
        let output = sample_finder_output();
        let finder = FileFinderState::new(&output.files, 0, 0);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let full_text = lines.join("\n");

        // Top Finder Box
        assert!(full_text.contains("FIND FILE (1/2)"));
        assert!(full_text.contains("@ █"));
        assert!(full_text.contains("▶ TODO.md"));
        assert!(full_text.contains("+2 -0 ~0"));
        assert!(
            full_text.contains("crates/diffuse-tui/src/app/render.rs")
                || full_text.contains("…/diffuse-tui/src/app/render.rs")
        );
        assert!(full_text.contains("+127 -295 ~2"));

        // Bottom Preview Box
        assert!(full_text.contains("PREVIEW"));
        assert!(full_text.contains("+ First todo item"));
        assert!(full_text.contains("+ Second todo item"));
    }

    #[test]
    fn test_finder_modal_navigation_updates_preview_visual_layout() {
        let output = sample_finder_output();
        let mut finder = FileFinderState::new(&output.files, 0, 0);
        // Move down to select the second file
        finder.move_down();
        assert_eq!(finder.selected_idx, 1);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let full_text = lines.join("\n");

        // Top Finder Box reflects selection on second file
        assert!(full_text.contains("FIND FILE (2/2)"));
        assert!(full_text.contains("TODO.md"));

        // Bottom Preview Box reflects preview of the second file
        assert!(full_text.contains("PREVIEW"));
        assert!(full_text.contains("- old render line"));
        assert!(full_text.contains("+ new render line"));
    }

    #[test]
    fn test_ansi_finder_modal_visual_layout() {
        let output = sample_finder_output();
        let finder = FileFinderState::new(&output.files, 0, 0);

        let lines = render_finder_modal(&output, &finder, 80, 24);
        assert_eq!(lines.len(), 24);

        let joined = lines.join("\n");
        assert!(joined.contains("FIND FILE (1/2)"));
        assert!(joined.contains("@ █"));
        assert!(joined.contains("TODO.md"));
        assert!(joined.contains("+2 -0 ~0"));
        assert!(joined.contains("First todo item"));
        assert!(joined.contains("Second todo item"));
    }

    #[test]
    fn test_help_modal_narrow_screen_no_wrapping() {
        use crate::view::header::visible_width;

        // Narrow phone width (44 columns, 24 rows)
        let lines = render_help_modal(44, 24);
        assert!(lines.len() >= 24);
        for (i, line) in lines.iter().enumerate() {
            let vw = visible_width(line);
            assert!(
                vw <= 44,
                "line #{i} visual width {vw} exceeds target width 44 on narrow screens: {line}"
            );
        }

        // Test with Ratatui backend on 44x32 terminal
        let backend = TestBackend::new(44, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_help_modal_widget(f, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..32 {
            for x in 0..44 {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("KEY BINDINGS"));
        assert!(text.contains("? / Esc"));
    }

    #[test]
    fn test_finder_modal_narrow_screen_no_wrapping() {
        use crate::{
            core::{counts::FileCounts, wire::OutputFile},
            view::header::visible_width,
        };

        let file = OutputFile {
            path: "crates/diffuse-tui/src/view/diff_grid.rs".to_string(),
            status: "modified".to_string(),
            new_line_count: 10,
            old_line_count: 10,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 116,
                dels: 0,
                mods: 0,
                movs: 0,
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![
                dummy_row(
                    RowKind::Add,
                    10,
                    "+ very long addition line exceeding narrow screen width",
                ),
                dummy_row(
                    RowKind::Del,
                    11,
                    "- another long deletion line exceeding width",
                ),
            ],
        };
        let output = PipelineOutput {
            files: vec![file],
            branch: "main".to_string(),
            untracked_skipped: vec![],
        };
        let finder = FileFinderState::new(&output.files, 0, 0);

        // Narrow phone width (44 columns, 24 rows)
        let lines = render_finder_modal(&output, &finder, 44, 24);
        for (i, line) in lines.iter().enumerate() {
            let vw = visible_width(line);
            assert!(
                vw <= 44,
                "finder line #{i} visual width {vw} exceeds target width 44 on narrow screens: {line}"
            );
        }

        // Test with Ratatui backend on 44x24 terminal
        let backend = TestBackend::new(44, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..24 {
            for x in 0..44 {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("FIND FILE"));
        assert!(text.contains("diff_grid.rs") || text.contains("…"));
    }

    #[test]
    fn test_finder_modal_lines_exact_width_wide_and_narrow() {
        use crate::{
            core::{counts::FileCounts, wire::OutputFile},
            view::header::visible_width,
        };

        let file_specs = [
            ("crates/diffuse-tui/src/app/tests.rs", 73, 0, 0),
            ("crates/diffuse-tui/src/event.rs", 3, 7, 3),
            ("crates/diffuse-tui/src/view/diff_grid.rs", 116, 0, 0),
            ("crates/diffuse-tui/src/view/mod.rs", 3, 0, 2),
            ("crates/diffuse-tui/src/view/modals.rs", 1010, 17, 35),
            ("plan/Ratatui Migration.md", 0, 0, 2),
        ];

        let files: Vec<OutputFile> = file_specs
            .iter()
            .map(|(path, adds, dels, mods)| OutputFile {
                path: path.to_string(),
                status: "modified".to_string(),
                new_line_count: 10,
                old_line_count: 10,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts {
                    adds: *adds,
                    dels: *dels,
                    mods: *mods,
                    movs: 0,
                },
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![dummy_row(RowKind::Add, 1, "+ line content")],
            })
            .collect();

        let output = PipelineOutput {
            files,
            branch: "main".to_string(),
            untracked_skipped: vec![],
        };

        // Test across widths: mobile (44), narrow (60), standard (80), laptop (120),
        // ultrawide (322)
        for &w in &[44, 60, 80, 120, 322] {
            for sel in 0..file_specs.len() {
                let mut finder = FileFinderState::new(&output.files, 0, 0);
                finder.selected_idx = sel;

                let lines = render_finder_modal(&output, &finder, w, 24);
                assert_eq!(
                    lines.len(),
                    24,
                    "must produce exactly 24 lines for w={w}, sel={sel}"
                );
                for (i, line) in lines.iter().enumerate() {
                    let vw = visible_width(line);
                    assert!(
                        vw <= w,
                        "line #{i} visual width {vw} exceeds target width {w} (sel={sel}): {line}"
                    );
                    assert_eq!(
                        vw, w,
                        "line #{i} visual width {vw} must be exactly padded to width {w} (sel={sel})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_wrap_words() {
        assert_eq!(wrap_words("", 10), vec![""]);
        assert_eq!(
            wrap_words("quick brown fox jumps", 11),
            vec!["quick brown", "fox jumps"]
        );
        assert_eq!(
            wrap_words("new note (ctrl+enter save, esc cancel)", 20),
            vec!["new note (ctrl+enter", "save, esc cancel)"]
        );
        assert_eq!(
            wrap_words("superlongwordexceedingmax", 8),
            vec!["superlon", "gwordexc", "eedingma", "x"]
        );
    }

    #[test]
    fn test_ansi_to_ratatui_line_strikethrough_reset() {
        let ansi = "prefix \x1b[9mstruck text\x1b[29m normal text";
        let line = ansi_to_ratatui_line(ansi);
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content, "prefix ");
        assert!(
            !line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
        assert_eq!(line.spans[1].content, "struck text");
        assert!(
            line.spans[1]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
        assert_eq!(line.spans[2].content, " normal text");
        assert!(
            !line.spans[2]
                .style
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
    }

    #[test]
    fn test_finder_modal_proportional_height_scaling() {
        let output = PipelineOutput {
            files: (0..30)
                .map(|i| crate::core::wire::OutputFile {
                    path: format!("file_{i}.rs"),
                    status: "modified".to_string(),
                    new_line_count: 1,
                    old_line_count: 1,
                    new_content: vec![],
                    old_content: vec![],
                    hunks: vec![],
                    raw_hunks: vec![],
                    counts: Default::default(),
                    fences: vec![],
                    notes: vec![],
                    content_runs: vec![],
                    rows: vec![],
                })
                .collect(),
            branch: "main".to_string(),
            untracked_skipped: vec![],
        };
        let finder = FileFinderState::new(&output.files, 0, 0);
        let backend = TestBackend::new(100, 60);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..60 {
            for x in 0..100 {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("file_0.rs"));
        assert!(text.contains("file_11.rs"));

        // Constrained terminal (height 14): exactly 3 visible files (file_0.rs through
        // file_2.rs)
        let backend_constrained = TestBackend::new(80, 14);
        let mut term_constrained = Terminal::new(backend_constrained).unwrap();
        term_constrained
            .draw(|f| {
                render_finder_modal_widget(f, &output, &finder, f.area());
            })
            .unwrap();
        let buf_constrained = term_constrained.backend().buffer();
        let mut text_constrained = String::new();
        for y in 0..14 {
            for x in 0..80 {
                text_constrained.push_str(buf_constrained[(x, y)].symbol());
            }
            text_constrained.push('\n');
        }
        assert!(text_constrained.contains("file_0.rs"));
        assert!(text_constrained.contains("file_2.rs"));
        assert!(!text_constrained.contains("file_3.rs"));
    }

    #[test]
    fn test_ratatui_note_modal_visual_layout() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_note_modal_widget(
                    f,
                    "src/lib.rs:12-18",
                    "Fix potential off-by-one error\nEnsure bounds are checked",
                    f.area(),
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let full_text = lines.join("\n");

        // Verify title components
        assert!(full_text.contains("NOTE on src/lib.rs:12-18"));
        assert!(full_text.contains("Ctrl+S Save"));
        assert!(full_text.contains("Enter ↵"));
        assert!(full_text.contains("Esc Cancel"));

        // Verify multiline content and cursor
        assert!(full_text.contains("Fix potential off-by-one error"));
        assert!(full_text.contains("Ensure bounds are checked█"));

        // Verify rounded borders rendered
        assert!(full_text.contains('╭') || full_text.contains('┌'));
        assert!(full_text.contains('╰') || full_text.contains('└'));
    }

    #[test]
    fn test_ratatui_note_modal_empty_draft_visual() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_note_modal_widget(f, "Cargo.toml:5", "", f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut full_text = String::new();
        for y in 0..24 {
            for x in 0..80 {
                full_text.push_str(buffer[(x, y)].symbol());
            }
            full_text.push('\n');
        }

        assert!(full_text.contains("NOTE on Cargo.toml:5"));
        assert!(
            full_text.contains('█'),
            "Must render cursor for empty draft"
        );
    }

    #[test]
    fn test_ratatui_note_modal_constrained_terminal() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_note_modal_widget(f, "main.rs:1", "short note", f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut full_text = String::new();
        for y in 0..10 {
            for x in 0..40 {
                full_text.push_str(buffer[(x, y)].symbol());
            }
            full_text.push('\n');
        }

        assert!(full_text.contains("NOTE on main.rs:1"));
        assert!(full_text.contains("short note█"));
    }

    #[test]
    fn test_ratatui_note_modal_long_line_word_wrapping() {
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let long_line = "This is a very long note text that exceeds the width of the modal and should wrap across multiple visual rows cleanly.";
        terminal
            .draw(|f| {
                render_note_modal_widget(f, "long.rs:10", long_line, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..20 {
            let mut line = String::new();
            for x in 0..60 {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        let full_text = lines.join("\n");
        assert!(full_text.contains("NOTE on long.rs:10"));
        assert!(full_text.contains("This is a very long note text"));
        assert!(full_text.contains("cleanly.█"));
    }

    #[test]
    fn test_ratatui_note_modal_equal_margins_on_small_screens() {
        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_note_modal_widget(f, "Cargo.lock:81", "hello", f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        // Check top border row (where the rounded corners ╭ and ╮ live)
        // Y coordinate of centered modal in 20-height area:
        // Modal height is approx 7..10, centered vertically
        let mut found_border_y = None;
        for y in 0..20 {
            if buffer[(1, y)].symbol() == "╭" {
                found_border_y = Some(y);
                break;
            }
        }

        let y = found_border_y.expect("must find top border ╭ at col 1");
        assert_eq!(
            buffer[(0, y)].symbol(),
            " ",
            "Col 0 must be 1 char left margin"
        );
        assert_eq!(buffer[(1, y)].symbol(), "╭", "Col 1 must be left border ╭");
        assert_eq!(
            buffer[(48, y)].symbol(),
            "╮",
            "Col 48 must be right border ╮"
        );
        assert_eq!(
            buffer[(49, y)].symbol(),
            " ",
            "Col 49 must be 1 char right margin (symmetric!)"
        );
    }
}
