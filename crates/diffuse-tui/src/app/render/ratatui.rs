//! Terminal UI layout rendering via Ratatui widgets and frame buffers.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Paragraph,
};

use crate::{
    app::AppState,
    render::UiPalette,
    view::{
        bottom_ribbon_line,
        header::status_bar_line,
        modals::{render_finder_modal_widget, render_help_modal_widget, render_note_modal_widget},
        to_color,
    },
};

/// Blank secondary widget keeping the status background when empty.
pub(crate) fn blank_status_widget(ui: &UiPalette) -> Paragraph<'static> {
    Paragraph::new(ratatui::text::Line::default())
        .style(Style::default().bg(to_color(ui.status_bar_bg)))
}

impl AppState {
    /// Render visual frame to a Ratatui [`Frame`] with structured layout
    /// components.
    pub fn render_ratatui_frame(&mut self, f: &mut Frame) {
        let area = f.area();
        let width = area.width as usize;
        let cur_fi = self.current_file_idx();

        // Scrollbar pre-pass inputs that need no borrows held.
        let est_cw = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };
        let est_total = self.cached_geometry_tuple(est_cw).3;

        self.poll_status_expiry(std::time::Instant::now());

        // Owned up front so no borrow is held across the mutable viewport
        // call below.
        let status_msg_owned: Option<(String, bool)> = self
            .visible_status_message()
            .map(|(m, ok)| (m.to_string(), ok));
        let common_prefix = self.output.common_directory_prefix();

        let file = self.output.files.get(cur_fi);
        // Reserve the secondary slot even when empty so showing/hiding
        // never shifts diff rows. Emptiness is width-independent (see
        // `status_layout::segments_for`), so flags suffice for layout.
        // The empty state ("No diff to display.") lives in the bar itself
        // with no timeout, so zero files always shows the bar.
        let show_status = status_msg_owned.is_some()
            || self.is_streaming
            || self.raw_diff
            || self.col_offset > 0
            || !self.num_buffer.is_empty()
            || self.output.files.is_empty()
            || common_prefix.as_deref().is_some_and(|p| !p.is_empty());

        let constraints = vec![
            Constraint::Length(1), // Header bar
            Constraint::Length(1), // Secondary status bar (reserved)
            Constraint::Min(1),    // Main diff area
            Constraint::Length(1), // Bottom ribbon
        ];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints.clone())
            .split(area);

        // Scrollbar pre-pass: the horizontal split needs to know up front
        // whether the bar column exists. Heights come from the vertical
        // chunks above, so this estimates with the scrollbar-width content.
        // Layout is Header/Status/Diff/Ribbon, so the diff is chunk 2.
        let diff_vh = chunks[2].height as usize;
        let show_bar = file.is_some()
            && !self.raw_diff
            && width > 1
            && est_total > diff_vh
            && diff_vh > 2
            && area.height > 0;

        // Root column layout: the app column (all rows) plus, when needed,
        // a 1-wide full-height scrollbar column.
        let (main_area, bar_area) = if show_bar {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            (cols[0], Some(cols[1]))
        } else {
            (area, None)
        };
        let main_width = main_area.width as usize;
        let track_h = area.height as usize;

        // Re-split the app column vertically: same row heights as `chunks`.
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(main_area);

        let mut chunk_idx = 0;

        // 1. Header Bar
        let header_line = crate::view::header_bar_line(
            &self.repo,
            &self.theme,
            Some(self.output.total_counts()),
            Some(&self.output.branch),
            self.commit_range.as_deref(),
            self.target.is_staged(),
            self.header_file_count(),
            main_width,
        );
        let header_bg = to_color(self.theme.palette.ui.header_bar_bg);
        f.render_widget(
            Paragraph::new(header_line).style(Style::default().bg(header_bg)),
            main_chunks[chunk_idx],
        );
        chunk_idx += 1;

        // 2. Secondary status bar (reserved with status bg when empty).
        let status_area = main_chunks[chunk_idx];
        chunk_idx += 1;

        // 3. Main diff area.
        let diff_area = main_chunks[chunk_idx];
        chunk_idx += 1;

        let ribbon_area = main_chunks[chunk_idx];

        if self.output.files.get(cur_fi).is_none() {
            let bg = to_color(self.theme.palette.ui.bg_base);
            // Main area stays blank: the empty-state notice lives in the
            // secondary bar (persistent, no timeout).
            let p = Paragraph::new(ratatui::text::Line::default()).style(Style::default().bg(bg));
            f.render_widget(p, diff_area);

            if show_status {
                let cur_hunk = self.current_hunk().unwrap_or(0);
                let none_file = None;
                let hunk_counts = None;
                let msg_ref = status_msg_owned.as_ref().map(|(m, ok)| (m.as_str(), *ok));
                let status_line = status_bar_line(crate::view::StatusBarContext {
                    cur_fi,
                    total_files: self.output.files.len(),
                    cur_hunk,
                    total_hunks: 0,
                    raw_diff: self.raw_diff,
                    side_by_side: self.side_by_side,
                    expanded_mods_count: if self.side_by_side {
                        0
                    } else {
                        self.expanded_mods.len()
                    },
                    total_mods: self.total_mod_rows(),
                    col_offset: self.col_offset,
                    num_buffer: &self.num_buffer,
                    common_prefix: common_prefix.as_deref(),
                    width: main_width,
                    is_streaming: self.is_streaming,
                    streaming_dot: self.streaming_dot(),
                    status_message: msg_ref,
                    file: none_file,
                    hunk_counts,
                    theme: self.theme,
                });
                let status_bg = to_color(self.theme.palette.ui.status_bar_bg);
                f.render_widget(
                    Paragraph::new(status_line).style(Style::default().bg(status_bg)),
                    status_area,
                );
            } else {
                f.render_widget(blank_status_widget(&self.theme.palette.ui), status_area);
            }

            let rib_line = bottom_ribbon_line(main_width, &self.theme.palette.ui);
            f.render_widget(
                Paragraph::new(rib_line).style(Style::default().bg(bg)),
                ribbon_area,
            );

            self.last_viewport_start = diff_area.y as usize;
            self.viewport_height = diff_area.height as usize;
            self.last_content_width = main_width;
            self.screen_height = track_h;

            if self.show_help {
                render_help_modal_widget(f, &self.theme, area);
            }
            if let Some(ref finder) = self.file_finder {
                render_finder_modal_widget(f, &self.output, finder, &self.theme, area);
            }
            return;
        }

        let file_path = self
            .output
            .files
            .get(cur_fi)
            .map(|fl| fl.path.clone())
            .unwrap_or_default();

        // 4. Diff Grid Viewport (no scrollbar chars here anymore; the bar
        // owns its own full-height column). Pass the full width: the
        // viewport fn deducts the bar column itself, so passing main_width
        // would subtract it twice and desync hit column from drawn column.
        let (grid_lines, has_scrollbar, thumb_offset, thumb_size, content_width) =
            self.render_diff_viewport_lines(width, diff_area.height as usize, track_h);

        if cfg!(debug_assertions) {
            for gline in grid_lines.iter() {
                debug_assert!(
                    gline.width() <= content_width,
                    "grid line visual width ({}) exceeds content_width ({})",
                    gline.width(),
                    content_width
                );
            }
        }
        let diff_bg = to_color(self.theme.palette.ui.bg_base);
        let diff_fg = to_color(self.theme.palette.ui.text);
        f.render_widget(
            Paragraph::new(grid_lines).style(Style::default().fg(diff_fg).bg(diff_bg)),
            diff_area,
        );

        // 5. Full-height scrollbar column.
        if let Some(bar) = bar_area
            && has_scrollbar
        {
            let thumb_color = to_color(self.theme.palette.ui.scrollbar_thumb);
            let track_color = to_color(self.theme.palette.ui.scrollbar_track);
            let (_, counts, _, total_vis_lines) = self.cached_geometry_tuple(content_width);
            let header_vis = counts.first().copied().unwrap_or(1);
            let subcell = Self::scrollbar_subcell_for(
                track_h,
                diff_area.height as usize,
                total_vis_lines,
                self.scroll,
                header_vis,
            );
            let bar_lines: Vec<ratatui::text::Line> = (0..track_h)
                .map(|r| {
                    if let Some(sub) = subcell {
                        sub.cell_at(r).to_ratatui_line(thumb_color, track_color)
                    } else if r >= thumb_offset && r < thumb_offset + thumb_size {
                        ratatui::text::Line::from(ratatui::text::Span::styled(
                            "█",
                            Style::default().fg(thumb_color).bg(thumb_color),
                        ))
                    } else {
                        ratatui::text::Line::from(ratatui::text::Span::styled(
                            "│",
                            Style::default().fg(track_color).bg(track_color),
                        ))
                    }
                })
                .collect();
            let track_bg = to_color(self.theme.palette.ui.scrollbar_track);
            f.render_widget(
                Paragraph::new(bar_lines).style(Style::default().bg(track_bg)),
                bar,
            );
        }
        if show_status {
            let cur_hunk = self.current_hunk().unwrap_or(0);
            let file_ref = self.output.files.get(cur_fi);
            let hunk_counts = file_ref.and_then(|fl| fl.hunks.get(cur_hunk).map(|h| h.counts()));
            let msg_ref = status_msg_owned.as_ref().map(|(m, ok)| (m.as_str(), *ok));
            let status_line = status_bar_line(crate::view::StatusBarContext {
                cur_fi,
                total_files: self.output.files.len(),
                cur_hunk,
                total_hunks: file_ref.map_or(0, |fl| fl.hunks.len().max(1)),
                raw_diff: self.raw_diff,
                side_by_side: self.side_by_side,
                expanded_mods_count: if self.side_by_side {
                    0
                } else {
                    self.expanded_mods.len()
                },
                total_mods: self.total_mod_rows(),
                col_offset: self.col_offset,
                num_buffer: &self.num_buffer,
                common_prefix: common_prefix.as_deref(),
                width: main_width,
                is_streaming: self.is_streaming,
                streaming_dot: self.streaming_dot(),
                status_message: msg_ref,
                file: file_ref,
                hunk_counts,
                theme: self.theme,
            });
            let status_bg = to_color(self.theme.palette.ui.status_bar_bg);
            f.render_widget(
                Paragraph::new(status_line).style(Style::default().bg(status_bg)),
                status_area,
            );
        } else {
            f.render_widget(blank_status_widget(&self.theme.palette.ui), status_area);
        }

        // 6. Bottom Ribbon
        let rib_line = bottom_ribbon_line(main_width, &self.theme.palette.ui);
        let rib_bg = to_color(self.theme.palette.ui.bg_base);
        f.render_widget(
            Paragraph::new(rib_line).style(Style::default().bg(rib_bg)),
            ribbon_area,
        );

        // Update viewport metrics for diff area
        self.last_viewport_start = diff_area.y as usize;
        self.viewport_height = diff_area.height as usize;
        self.last_content_width = content_width;
        self.screen_height = track_h;

        // 6. Help Modal Overlay
        if self.show_help {
            render_help_modal_widget(f, &self.theme, area);
        }

        // 7. File Finder Modal Overlay
        if let Some(ref finder) = self.file_finder {
            render_finder_modal_widget(f, &self.output, finder, &self.theme, area);
        }

        // 8. Note Draft Modal Overlay
        if self.note_draft.is_some() || self.note_editor.is_some() {
            let loc = if let Some(sel) = self.compute_selection() {
                sel.loc_label(file_path.as_str())
            } else if let Some(fr) = self.current_flat_row() {
                if let crate::types::FlatRowKind::FileHeader = fr.kind {
                    file_path.clone()
                } else {
                    let coord = self
                        .row_of(fr)
                        .and_then(crate::core::notes::DiffCoord::from_row)
                        .map_or_else(String::new, crate::core::notes::DiffCoord::label);
                    format!("{file_path}:{coord}")
                }
            } else {
                file_path.clone()
            };
            let note_area = if !self.raw_diff && has_scrollbar {
                main_area
            } else {
                area
            };
            let draft = self.note_draft.as_deref().unwrap_or("");
            render_note_modal_widget(
                f,
                &loc,
                draft,
                self.note_editor.as_mut(),
                &self.theme,
                note_area,
            );
        }
    }
}
