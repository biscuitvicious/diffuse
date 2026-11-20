//! File finder modal popup and diff preview rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{geometry::standard_modal_area, path::format_path_matches_spans};
use crate::{
    core::wire::{OutputHunk, PipelineOutput},
    finder::FileFinderState,
    render::{Badge, DiffuseTheme, row_segment_spans, styled_prefix_spans, width::display_width},
    view::{diff_grid::hunk_divider_line, header::format_counts_plain, span::fit_line},
};

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

    let p = &theme.palette.ui;
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
                .bg(crate::view::to_color(p.bg_modal))
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
                crate::view::to_color(p.bg_modal)
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
                            .bg(crate::view::to_color(p.bg_modal))
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
                .bg(crate::view::to_color(p.bg_modal))
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
                    total: Some(hunks.len()),
                };
                let mut div_line = hunk_divider_line(badge, h, 0, false, inner_w, theme);
                for span in &mut div_line.spans {
                    if span.style.bg.is_none() {
                        span.style = span.style.bg(crate::view::to_color(p.bg_modal));
                    }
                }
                preview_lines.push(div_line);

                for r in &h.rows {
                    if preview_lines.len() >= max_preview_rows {
                        break 'outer;
                    }
                    let prefix = styled_prefix_spans(
                        r.kind, r.mov_side, None, None, r.group, r.new_line, r.old_line, &r.runs,
                        true, None, theme,
                    );
                    let text = r.text();
                    let row = row_segment_spans(
                        &text,
                        r.is_peer(),
                        r.kind,
                        r.mov_side,
                        None,
                        &r.runs,
                        0,
                        text_cols,
                        prefix,
                        false,
                        false,
                        0,
                        true,
                        true,
                        theme,
                    );
                    preview_lines.push(fit_line(row.spans, inner_w, row.line_bg));
                }
            }
        }
    }

    let preview_p = Paragraph::new(preview_lines).block(preview_block);
    f.render_widget(preview_p, preview_area);
}
