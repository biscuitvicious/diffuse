use std::{borrow::Cow, mem};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{
    diff_grid::{hunk_divider_line, hunk_divider_text},
    header::{clamp_or_pad_ansi, format_counts_plain, truncate_pad, visible_width},
};
use crate::{
    core::wire::{OutputHunk, PipelineOutput},
    finder::FileFinderState,
    render::{
        Badge, ColorExt, DiffuseTheme, render_row_segment, slice_by_visual_width, styled_prefix,
        width::{char_width, display_width},
    },
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

/// Canonical key bindings reference displayed in the `?` help dialog.
pub const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j · ↓ / k · ↑", "next / prev line"),
    ("J · } / K · {", "next / prev hunk"),
    ("Space · PgDn · Ctrl+d · Ctrl+f", "page down"),
    ("PgUp · Ctrl+u · Ctrl+b", "page up"),
    ("G · End / g · Home", "bottom / top"),
    ("<num>g · <num>G", "jump to line number"),
    ("n · ] · Tab / p · [ · S-Tab", "next / prev file"),
    ("@", "find file in diff"),
    ("Enter", "toggle file collapse / expand"),
    ("l · → / h · ←", "right / left scroll"),
    ("^ · 0 / $", "start / end of line"),
    ("w", "toggle word diff"),
    ("y", "toggle syntax highlight"),
    ("R", "toggle raw diff view"),
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

/// Render the `?` key-bindings modal as full-screen overlay lines.
pub fn render_help_modal(width: usize, height: usize, theme: &DiffuseTheme) -> Vec<String> {
    let key_w = if width < 48 {
        16
    } else if width < 58 {
        22
    } else {
        32
    };

    let entries = HELP_ENTRIES;

    let p = &theme.palette;
    let bg_code = p.modal_bg.bg_code();
    let border_code = p.modal_border.fg_code();
    let title_code = p.modal_title.fg_code();
    let text_code = p.modal_text.fg_code();
    let dim_code = p.modal_dim.fg_code();

    let mut lines = Vec::new();
    lines.push(format!(
        "{bg_code}\x1b[1m{title_code}{}\x1b[0m",
        truncate_pad("  KEY BINDINGS", width)
    ));
    lines.push(format!(
        "{bg_code}{border_code}{}\x1b[0m",
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
                "{bg_code}{text_code}{}\x1b[0m",
                truncate_pad(&line_raw, width)
            ));
        }
    }

    lines.push(format!(
        "{bg_code}{dim_code}{}\x1b[0m",
        truncate_pad("  ? · Esc to close", width)
    ));

    while lines.len() < height {
        lines.push(format!("{bg_code}{}\x1b[0m", " ".repeat(width)));
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

/// Render the Help Modal as a centered Ratatui widget popup with rounded
/// borders.
pub fn render_help_modal_widget(f: &mut Frame, theme: &DiffuseTheme, area: Rect) {
    let entries = HELP_ENTRIES;

    let modal_area = standard_modal_area(area);
    f.render_widget(Clear, modal_area);

    let p = &theme.palette;
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
                .bg(crate::view::to_color(p.modal_bg))
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

/// Render the fuzzy file finder popup dialog covering the full screen with a
/// top finder list and bottom diff preview.
pub fn render_finder_modal(
    output: &PipelineOutput,
    finder: &FileFinderState,
    width: usize,
    height: usize,
    theme: &DiffuseTheme,
) -> Vec<String> {
    let p = &theme.palette;
    let bg_code = p.modal_bg.bg_code();
    let border_code = p.modal_border.fg_code();
    let title_code = p.modal_title.fg_code();

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
        "{bg_code}\x1b[1m{title_code}{}\x1b[0m",
        truncate_pad(&header_info, width)
    ));
    lines.push(format!(
        "{bg_code}{border_code}{}\x1b[0m",
        "─".repeat(width)
    ));

    let prompt = format!("  @ {}█", finder.query);
    lines.push(format!(
        "{bg_code}\x1b[1m{title_code}{}\x1b[0m",
        truncate_pad(&prompt, width)
    ));
    lines.push(format!(
        "{bg_code}{border_code}{}\x1b[0m",
        "─".repeat(width)
    ));

    let max_visible = 6.min(height.saturating_sub(10).max(3));
    let total = finder.matches.len();
    if total == 0 {
        lines.push(format!(
            "{bg_code}{border_code}{}\x1b[0m",
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
            let stats = format_counts_plain(&f.counts);

            let row_bg_code = if is_sel {
                p.modal_selected_bg.bg_code()
            } else {
                p.modal_bg.bg_code()
            };
            let stats_color = if is_sel {
                p.modal_selected_fg.fg_code()
            } else {
                p.modal_dim.fg_code()
            };
            let prefix_color = if is_sel {
                format!("\x1b[1m{}", p.count_adds.fg_code())
            } else {
                p.modal_dim.fg_code()
            };

            let prefix_w = visible_width(prefix);
            let stats_w = display_width(&stats);
            let right_margin = 2;
            let avail_path_w = width.saturating_sub(prefix_w + 2 + stats_w + right_margin);

            let (path_display, path_vis_w) =
                format_path_matches(&m.path, &m.match_indices, avail_path_w, is_sel, p);

            let used_w = prefix_w + path_vis_w + stats_w + right_margin;
            let pad_w = width.saturating_sub(used_w);
            let padding = " ".repeat(pad_w);

            let line_raw = format!(
                "{row_bg_code}{prefix_color}{prefix}{path_display}{row_bg_code}{padding}{stats_color}{stats}  \x1b[0m"
            );
            lines.push(line_raw);
        }
    }

    // Horizontal ruler dividing finder matches from the live diff preview
    lines.push(format!(
        "{bg_code}{border_code}{}\x1b[0m",
        "─".repeat(width)
    ));

    // Bottom Preview Section (rendered exactly like a real diff, without the
    // filename bar)
    if let Some(selected_match) = finder.matches.get(finder.selected_idx) {
        let f = &output.files[selected_match.file_idx];

        let remaining = height.saturating_sub(lines.len());
        let synthetic_hunk;
        let has_hunk_rows = f.hunks.iter().any(|h| !h.rows.is_empty())
            || f.raw_hunks.iter().any(|h| !h.rows.is_empty());
        let hunks: &[OutputHunk] = if !f.hunks.is_empty() && has_hunk_rows {
            &f.hunks
        } else if !f.raw_hunks.is_empty() && has_hunk_rows {
            &f.raw_hunks
        } else if !f.rows.is_empty() {
            synthetic_hunk = [OutputHunk {
                old_start: 1,
                old_count: 0,
                new_start: 1,
                new_count: f.rows.len() as u32,
                rows: f.rows.clone(),
            }];
            &synthetic_hunk
        } else {
            &f.hunks
        };

        if hunks.is_empty() {
            if remaining > 0 {
                let dim_code = p.modal_dim.fg_code();
                lines.push(format!(
                    "{bg_code}{dim_code}{}\x1b[0m",
                    truncate_pad("      (No diff rows to display)", width)
                ));
            }
        } else {
            let (bg, fg) = theme.palette.hunk_divider_colors(false, false);
            let div_bg_code = match bg {
                crate::render::Color::Primary => p.modal_bg.bg_code(),
                color => color.bg_code(),
            };
            let fg_code = fg.fg_code();
            let text_cols = width.saturating_sub(6).max(1);

            'outer: for (hi, h) in hunks.iter().enumerate() {
                if lines.len() >= height {
                    break;
                }
                let badge = Badge {
                    idx: hi,
                    total: hunks.len(),
                };
                let txt = hunk_divider_text(badge, h, 0);
                let piece = slice_by_visual_width(&txt, 0, width);
                let piece_w = display_width(&piece);
                let bar_str = if piece_w < width {
                    format!("{piece}{}", "─".repeat(width - piece_w))
                } else {
                    piece
                };
                lines.push(format!("{div_bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m"));

                for r in &h.rows {
                    if lines.len() >= height {
                        break 'outer;
                    }
                    let pfx = styled_prefix(
                        r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, true, theme,
                    );
                    let text = r.text();
                    let rendered_row = render_row_segment(
                        &text,
                        r.is_aux(),
                        r.kind,
                        r.mov_side,
                        &r.runs,
                        0,
                        text_cols,
                        &pfx,
                        false,
                        false,
                        0,
                        true,
                        true,
                        theme,
                    );
                    lines.push(clamp_or_pad_ansi(&rendered_row, width));
                }
            }
        }
    }

    while lines.len() < height {
        lines.push(format!("{bg_code}{}\x1b[0m", " ".repeat(width)));
    }

    lines
}

/// Render the File Finder directly as a Ratatui widget popup dialog with top
/// finder and bottom preview.
pub fn render_finder_modal_widget(
    f: &mut Frame,
    output: &PipelineOutput,
    finder: &FileFinderState,
    theme: &DiffuseTheme,
    area: Rect,
) {
    let modal_area = standard_modal_area(area);
    f.render_widget(Clear, modal_area);

    let p = &theme.palette;
    let file_list_h = (modal_area.height / 5).max(3);
    let finder_h = (file_list_h + 4)
        .min(modal_area.height.saturating_sub(4))
        .max(4);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(finder_h), Constraint::Min(4)])
        .split(modal_area);

    let finder_area = chunks[0];
    let preview_area = chunks[1];

    let title = if modal_area.width < 55 {
        format!(
            " FIND FILE ({}/{}) · ↑/↓ · ↵ · Esc ",
            if finder.matches.is_empty() {
                0
            } else {
                finder.selected_idx + 1
            },
            finder.matches.len()
        )
    } else {
        format!(
            " FIND FILE ({}/{}) · ↑/↓ nav · Enter jump · Esc close ",
            if finder.matches.is_empty() {
                0
            } else {
                finder.selected_idx + 1
            },
            finder.matches.len()
        )
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
                .bg(crate::view::to_color(p.modal_bg))
                .fg(crate::view::to_color(p.modal_text)),
        );

    let inner_w = finder_area.width.saturating_sub(2) as usize;

    let mut finder_lines = Vec::new();
    finder_lines.push(Line::from(vec![
        Span::styled(
            "@ ",
            Style::default()
                .fg(crate::view::to_color(p.status_bar_accent))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            finder.query.clone(),
            Style::default()
                .fg(crate::view::to_color(p.modal_title))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(crate::view::to_color(p.accent))),
    ]));
    finder_lines.push(Line::from(Span::styled(
        "─".repeat(inner_w),
        Style::default().fg(crate::view::to_color(p.modal_border)),
    )));

    let max_visible = (finder_area.height.saturating_sub(4) as usize).max(1);
    let total = finder.matches.len();
    if total == 0 {
        finder_lines.push(Line::from(Span::styled(
            "  No matching files in diff",
            Style::default().fg(crate::view::to_color(p.modal_dim)),
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
            let stats = format_counts_plain(&file.counts);

            let mut spans = vec![Span::styled(
                prefix,
                if is_sel {
                    Style::default()
                        .fg(crate::view::to_color(p.count_adds))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(crate::view::to_color(p.modal_dim))
                },
            )];

            let prefix_w = display_width(prefix);
            let stats_w = display_width(&stats);
            let avail_path_w = inner_w.saturating_sub(prefix_w + 2 + stats_w);
            let path_spans =
                format_path_matches_spans(&m.path, &m.match_indices, avail_path_w, is_sel, p);
            let path_len: usize = path_spans.iter().map(|s| display_width(&s.content)).sum();
            spans.extend(path_spans);

            let bg_color = if is_sel {
                crate::view::to_color(p.modal_selected_bg)
            } else {
                crate::view::to_color(p.modal_bg)
            };
            let pad_style = Style::default().bg(bg_color);

            let pad_w = inner_w.saturating_sub(prefix_w + path_len + stats_w);
            spans.push(Span::styled(" ".repeat(pad_w), pad_style));
            if !stats.is_empty() {
                spans.push(Span::styled(
                    stats,
                    if is_sel {
                        Style::default()
                            .fg(crate::view::to_color(p.modal_selected_fg))
                            .bg(crate::view::to_color(p.modal_selected_bg))
                    } else {
                        Style::default()
                            .fg(crate::view::to_color(p.modal_dim))
                            .bg(crate::view::to_color(p.modal_bg))
                    },
                ));
            }

            let mut line = Line::from(spans);
            if is_sel {
                line = line.style(Style::default().bg(crate::view::to_color(p.modal_selected_bg)));
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
        .title_style(
            Style::default()
                .fg(crate::view::to_color(p.modal_title))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(crate::view::to_color(p.modal_border)))
        .style(
            Style::default()
                .bg(crate::view::to_color(p.modal_bg))
                .fg(crate::view::to_color(p.modal_text)),
        );

    let mut preview_lines = Vec::new();
    if let Some(m) = finder.matches.get(finder.selected_idx) {
        let file = &output.files[m.file_idx];
        let max_preview_rows = preview_area.height.saturating_sub(2) as usize;
        let inner_w = preview_area.width.saturating_sub(2) as usize;
        let text_cols = inner_w.saturating_sub(6).max(1);

        let synthetic_hunk;
        let has_hunk_rows = file.hunks.iter().any(|h| !h.rows.is_empty())
            || file.raw_hunks.iter().any(|h| !h.rows.is_empty());
        let hunks: &[OutputHunk] = if !file.hunks.is_empty() && has_hunk_rows {
            &file.hunks
        } else if !file.raw_hunks.is_empty() && has_hunk_rows {
            &file.raw_hunks
        } else if !file.rows.is_empty() {
            synthetic_hunk = [OutputHunk {
                old_start: 1,
                old_count: 0,
                new_start: 1,
                new_count: file.rows.len() as u32,
                rows: file.rows.clone(),
            }];
            &synthetic_hunk
        } else {
            &file.hunks
        };

        if hunks.is_empty() {
            preview_lines.push(Line::from(Span::styled(
                "  (No diff lines to preview)",
                Style::default().fg(crate::view::to_color(p.modal_dim)),
            )));
        } else {
            'outer: for (hi, h) in hunks.iter().enumerate() {
                if preview_lines.len() >= max_preview_rows {
                    break;
                }
                let badge = Badge {
                    idx: hi,
                    total: hunks.len(),
                };
                let mut div_line = hunk_divider_line(badge, h, 0, false, inner_w, theme);
                if let Some(span) = div_line.spans.first_mut()
                    && span.style.bg.is_none()
                {
                    span.style = span.style.bg(crate::view::to_color(p.modal_bg));
                }
                preview_lines.push(div_line);

                for r in &h.rows {
                    if preview_lines.len() >= max_preview_rows {
                        break 'outer;
                    }
                    let pfx = styled_prefix(
                        r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, true, theme,
                    );
                    let text = r.text();
                    let rendered_row = render_row_segment(
                        &text,
                        r.is_aux(),
                        r.kind,
                        r.mov_side,
                        &r.runs,
                        0,
                        text_cols,
                        &pfx,
                        false,
                        false,
                        0,
                        true,
                        true,
                        theme,
                    );
                    let padded = clamp_or_pad_ansi(&rendered_row, inner_w);
                    preview_lines.push(ansi_to_ratatui_line(&padded));
                }
            }
        }
    }

    let preview_p = Paragraph::new(preview_lines).block(preview_block);
    f.render_widget(preview_p, preview_area);
}

/// Render the Note Draft modal overlay with multi-line support and exact range
/// coordinates.
pub fn render_note_modal_widget(
    f: &mut Frame,
    loc: &str,
    draft: &str,
    editor: Option<&mut crate::editor::Editor>,
    theme: &DiffuseTheme,
    area: Rect,
) {
    let p = &theme.palette;
    let margin_w = if area.width <= 60 { 2 } else { 4 };
    let modal_w = area.width.saturating_sub(margin_w).min(88);

    let min_h = 7.min(area.height);
    let max_h = area.height.saturating_sub(2).max(min_h);

    let inner_w = modal_w.saturating_sub(2).max(1) as usize;

    let mut fallback_editor;
    let ed = match editor {
        Some(e) => {
            e.set_width(inner_w);
            e
        }
        None => {
            fallback_editor = crate::editor::Editor::new(draft);
            fallback_editor.set_width(inner_w);
            let r = fallback_editor.lines().len().saturating_sub(1);
            let c = fallback_editor.lines()[r].chars().count();
            fallback_editor.set_cursor(r, c);
            &mut fallback_editor
        }
    };

    let total_visual_rows: usize = ed
        .lines()
        .iter()
        .map(|l| crate::editor::Editor::line_seg_count(l, inner_w))
        .sum();
    let modal_h = (total_visual_rows as u16 + 6).clamp(min_h, max_h);
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
        .title_style(
            Style::default()
                .fg(crate::view::to_color(p.warning))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(crate::view::to_color(p.modal_border)))
        .style(
            Style::default()
                .bg(crate::view::to_color(p.modal_bg))
                .fg(crate::view::to_color(p.modal_text)),
        );

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    ed.render(f, inner);
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

fn format_path_matches(
    path: &str,
    indices: &[usize],
    max_w: usize,
    selected: bool,
    palette: &crate::render::ThemePalette,
) -> (String, usize) {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        (
            highlight_path_matches(&clean, indices, selected, palette),
            char_count,
        )
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        let len = truncated.chars().count();
        (
            highlight_path_matches(&truncated, indices, selected, palette),
            len,
        )
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
            format!("\x1b[1m{}", palette.modal_title.fg_code())
        } else {
            palette.modal_text.fg_code()
        };
        let tail_highlighted = highlight_path_matches(&tail, &shifted_indices, selected, palette);
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
    palette: &crate::render::ThemePalette,
) -> Vec<Span<'static>> {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        highlight_path_matches_spans(&clean, indices, selected, palette)
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        highlight_path_matches_spans(&truncated, indices, selected, palette)
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
                .fg(crate::view::to_color(palette.modal_title))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::view::to_color(palette.modal_text))
        };
        let mut spans = vec![Span::styled("…", default_style)];
        spans.extend(highlight_path_matches_spans(
            &tail,
            &shifted_indices,
            selected,
            palette,
        ));
        spans
    }
}

fn highlight_path_matches_spans(
    path: &str,
    indices: &[usize],
    selected: bool,
    palette: &crate::render::ThemePalette,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let default_style = if selected {
        Style::default()
            .fg(crate::view::to_color(palette.modal_title))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(crate::view::to_color(palette.modal_text))
    };
    let match_style = Style::default()
        .fg(crate::view::to_color(palette.accent))
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
            spans.push(Span::styled(mem::take(&mut cur_text), style));
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

fn highlight_path_matches(
    path: &str,
    indices: &[usize],
    selected: bool,
    palette: &crate::render::ThemePalette,
) -> String {
    let mut out = String::new();
    let default_color = if selected {
        format!("\x1b[1m{}", palette.modal_title.fg_code())
    } else {
        palette.modal_text.fg_code()
    };
    let match_color = format!("\x1b[1m{}", palette.accent.fg_code());

    let mut cur_matched = None;
    for (i, ch) in path.char_indices() {
        let is_m = indices.contains(&i);
        if Some(is_m) != cur_matched {
            if is_m {
                out.push_str(&match_color);
            } else {
                out.push_str(&default_color);
            }
            cur_matched = Some(is_m);
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests;
