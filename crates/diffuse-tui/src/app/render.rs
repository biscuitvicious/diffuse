use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Paragraph,
};

use crate::{
    core::parse::RowKind,
    render::{ColorExt, render_raw_row, render_row_segment, slice_by_visual_width},
    view::{
        bottom_ribbon_lines, clamp_or_pad_ansi,
        header::{render_bottom_ribbon, render_header_bar, render_status_bar, status_bar_line},
        modals::{render_finder_modal, render_help_modal},
        visible_width,
    },
};

use super::AppState;

/// Render ANSI escape bytes as the literal text `\x1b` so diagnostic
/// messages stay readable instead of emitting raw control codes.
pub(crate) fn escape_ansi_for_diagnostic(s: &str) -> String {
    s.replace('\x1b', "\\x1b")
}

impl AppState {
    /// Render full visual screen frame as a String of ANSI terminal lines.
    pub fn render_frame(&mut self, width: usize, height: usize) -> String {
        let max_rows = height.saturating_sub(4).max(4);
        self.viewport_height = max_rows;
        let mut lines = Vec::new();

        let cur_fi = self.current_file_idx();
        let f = self.output.files.get(cur_fi);

        let common_prefix = self.output.common_directory_prefix();

        // Header bar
        lines.extend(render_header_bar(
            &self.repo,
            width,
            common_prefix.as_deref(),
            Some(self.output.total_counts()),
            Some(&self.output.branch),
            self.commit_range.as_deref(),
            self.target.is_staged(),
            &self.theme.palette,
        ));

        // Secondary status bar
        let total_files = self.output.files.len();
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let total_hunks = f.map_or(0, |file| file.hunks.len().max(1));
        let hunk_counts = f.and_then(|file| file.hunks.get(cur_hunk).map(|h| h.counts()));
        lines.extend(render_status_bar(crate::view::StatusBarContext {
            cur_fi,
            total_files,
            cur_hunk,
            total_hunks,
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            num_buffer: &self.num_buffer,
            common_prefix: common_prefix.as_deref(),
            width,
            is_streaming: self.is_streaming,
            streaming_dot: self.streaming_dot(),
            file: f,
            hunk_counts,
            theme: &self.theme,
        }));

        // Ephemeral action feedback (note saved/deleted, git ops, ...)
        if let Some((msg, is_ok)) = self.status_message.clone() {
            let color = if is_ok {
                self.theme.palette.header_staged
            } else {
                self.theme.palette.count_dels
            };
            lines.push(format!("{} {msg}\x1b[39m", color.fg_code()));
        }

        if let Some(f) = f {
            // Inline note draft if active
            if let Some(ref draft) = self.note_draft {
                let loc = if let Some(sel) = self.compute_selection() {
                    if sel.start == sel.end {
                        format!("{}:{}", f.path, sel.start)
                    } else {
                        format!("{}:{}-{}", f.path, sel.start, sel.end)
                    }
                } else {
                    let line = self
                        .current_flat_row()
                        .and_then(|fr| self.flat_row_coord(fr))
                        .unwrap_or(0);
                    format!("{}:{}", f.path, line)
                };
                lines.push(format!(
                    "\x1b[38;5;220m Note on {loc}: {draft}_  [ctrl+s save · enter newline · esc cancel]\x1b[39m"
                ));
            }
            let viewport_start_line = lines.len();
            let vh = height.saturating_sub(viewport_start_line + 2);
            self.last_viewport_start = viewport_start_line;
            self.viewport_height = vh;
            self.screen_height = height;

            let (mut grid_lines, has_scrollbar, thumb_offset, thumb_size, content_width) =
                self.render_diff_viewport_lines(width, vh, height);

            for (row_i, gline) in grid_lines.iter_mut().enumerate() {
                debug_assert!(
                    visible_width(gline) <= content_width,
                    "grid line visual width ({}) exceeds content_width ({})",
                    visible_width(gline),
                    content_width
                );
                *gline = clamp_or_pad_ansi(gline, content_width);
                if has_scrollbar && !self.raw_diff {
                    // Thumb geometry is full-track based: grid row i sits
                    // at track row viewport_start_line + i.
                    let track_row = viewport_start_line + row_i;
                    let sb_char =
                        if track_row >= thumb_offset && track_row < thumb_offset + thumb_size {
                            "\x1b[38;5;248m█\x1b[0m"
                        } else {
                            "\x1b[38;5;236m│\x1b[0m"
                        };
                    gline.push_str(sb_char);
                }
            }

            lines.extend(grid_lines);
        } else {
            let viewport_start_line = lines.len();
            let vh = height.saturating_sub(viewport_start_line + 2);
            self.last_viewport_start = viewport_start_line;
            self.viewport_height = vh;
            self.screen_height = height;

            lines.push(format!(
                "{} No diff to display.\x1b[0m",
                self.theme.palette.status_bar_dim.fg_code()
            ));
            for _ in 1..vh {
                lines.push(String::new());
            }
        }

        let (divider, ribbon) = render_bottom_ribbon(width, &self.theme.palette);
        lines.push(divider);
        lines.push(ribbon);

        // Help modal overlays the full screen when toggled on.
        if self.show_help {
            lines = render_help_modal(width, height, &self.theme);
        }

        // File finder dialog overlays the full screen with top finder + bottom
        // preview when active.
        if let Some(finder) = &self.file_finder {
            lines = render_finder_modal(&self.output, finder, width, height, &self.theme);
        }

        lines.truncate(height);

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(line);
            if i + 1 < lines.len() {
                out.push_str("\x1b[K\x1b[0m");
            } else {
                out.push_str("\x1b[0m");
            }
        }
        out
    }

    /// Switch between the visual (reflowed) and raw unified-diff
    /// representations, anchoring the cursor at its exact line: capture
    /// `(file, coord, side)`, rebuild the other view, jump to the
    /// equivalent row (same coordinate, preferring the same side), and
    /// preserve the visual screen-y pivot position.
    #[tracing::instrument(level = "trace", skip(self), fields(cur_raw = self.raw_diff, cur_row = self.row_idx, cur_file = self.current_file_idx()))]
    pub fn toggle_raw_diff(&mut self) {
        let old_vis = self.cursor_visual_line();
        let screen_y = old_vis.saturating_sub(self.scroll);

        let cur_hunk_hi = self.current_hunk();
        let is_sep_row = self.current_flat_row().is_some_and(|fr| fr.is_hunk_divider);

        let (file, target_new, target_old) = {
            let Some(row) = self.current_flat_row() else {
                return;
            };
            if let Some(r) = self.row_of(row)
                && (r.new_line.is_some() || r.old_line.is_some())
            {
                (row.file_idx, r.new_line, r.old_line)
            } else {
                let mut neighbor_coord = None;
                for offset in 1..self.flat.len() {
                    if self.row_idx >= offset {
                        let prev = &self.flat[self.row_idx - offset];
                        if prev.file_idx == row.file_idx
                            && let Some(c) = self.flat_row_coord(prev)
                        {
                            neighbor_coord = Some(c);
                            break;
                        }
                    }
                    if self.row_idx + offset < self.flat.len() {
                        let next = &self.flat[self.row_idx + offset];
                        if next.file_idx == row.file_idx
                            && let Some(c) = self.flat_row_coord(next)
                        {
                            neighbor_coord = Some(c);
                            break;
                        }
                    }
                }
                (row.file_idx, neighbor_coord, neighbor_coord)
            }
        };
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;
        let was_raw = self.raw_diff;
        self.raw_diff = !self.raw_diff;
        self.anchor = None;

        let mut idx = None;
        if is_sep_row
            && let Some(hi) = cur_hunk_hi
            && let Some(hs) = self
                .hunk_starts
                .iter()
                .find(|h| h.file == file && h.hunk == hi)
        {
            if was_raw {
                // Transitioning from raw to visual: use preceding gap row if
                // present
                if hs.flat > 0
                    && self
                        .flat
                        .get(hs.flat - 1)
                        .is_some_and(|fr| fr.is_hunk_divider)
                {
                    idx = Some(hs.flat - 1);
                } else {
                    idx = Some(hs.flat);
                }
            } else {
                // Transitioning from visual to raw: use hunk_start separator
                // row
                idx = Some(hs.flat);
            }
        }
        // Priority 1: Match on old_line if coming from a deletion
        if target_new.is_none()
            && let Some(target_ol) = target_old
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == file
                    && self.row_of(item).and_then(|r| r.old_line) == Some(target_ol)
                {
                    idx = Some(i);
                    break;
                }
            }
        }
        // Priority 2: Match on new_line if coming from an addition/context/mod
        if idx.is_none()
            && let Some(target_nl) = target_new
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == file
                    && self.row_of(item).and_then(|r| r.new_line) == Some(target_nl)
                {
                    idx = Some(i);
                    break;
                }
            }
        }
        // Priority 3: Match on old_line as fallback
        if idx.is_none()
            && let Some(target_ol) = target_old
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == file
                    && self.row_of(item).and_then(|r| r.old_line) == Some(target_ol)
                {
                    idx = Some(i);
                    break;
                }
            }
        }
        // Priority 4: Cross-side fallback
        if idx.is_none() {
            let coord = target_new.or(target_old);
            if let Some(c) = coord {
                for (i, item) in self.flat.iter().enumerate() {
                    if item.file_idx == file
                        && self
                            .row_of(item)
                            .is_some_and(|r| r.new_line == Some(c) || r.old_line == Some(c))
                    {
                        idx = Some(i);
                        break;
                    }
                }
            }
        }
        // Priority 5: Proximity fallback (closest line in the same file if
        // target is in a gap)
        if idx.is_none() {
            let target = target_new.or(target_old).unwrap_or(0);
            let mut best = None;
            let mut best_diff = u32::MAX;
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != file || item.is_file_header {
                    continue;
                }
                if let Some(c) = self.flat_row_coord(item) {
                    let diff = target.abs_diff(c);
                    if diff < best_diff {
                        best_diff = diff;
                        best = Some(i);
                    }
                }
            }
            idx = best;
        }
        let idx = idx.unwrap_or_else(|| self.first_row_of_file(file));
        self.row_idx = idx;
        self.vis_line = 0;

        let new_vis = self.cursor_visual_line();
        let width = if self.terminal_width > 0 {
            self.terminal_width
        } else if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let content_width = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };
        let (_, _, _, total_vis_lines) = self.cached_geometry_tuple(content_width);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = new_vis.saturating_sub(screen_y).min(max_scroll);

        if cfg!(debug_assertions) && !self.flat.is_empty() {
            debug_assert!(
                self.row_idx < self.flat.len(),
                "Cursor row_idx must remain within flat row bounds after view toggle"
            );
        }
    }

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

        let file = self.output.files.get(cur_fi);

        let has_status_msg = self.status_message.is_some();

        let mut constraints = vec![
            Constraint::Length(1), // Header bar
            Constraint::Length(1), // Secondary status bar
        ];
        if has_status_msg {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Min(1)); // Main diff area
        constraints.push(Constraint::Length(2)); // Bottom ribbon

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints.clone())
            .split(area);

        // Scrollbar pre-pass: the horizontal split needs to know up front
        // whether the bar column exists. Heights come from the vertical
        // chunks above, so this estimates with the scrollbar-width content.
        let diff_chunk_idx = if has_status_msg { 3 } else { 2 };
        let diff_vh = chunks[diff_chunk_idx].height as usize;
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
        let common_prefix = self.output.common_directory_prefix();
        let header_line = crate::view::header_bar_line(
            &self.repo,
            &self.theme,
            common_prefix.as_deref(),
            Some(self.output.total_counts()),
            Some(&self.output.branch),
            self.commit_range.as_deref(),
            self.target.is_staged(),
        );
        let header_bg = crate::view::to_color(self.theme.palette.header_bar_bg);
        f.render_widget(
            Paragraph::new(header_line).style(Style::default().bg(header_bg)),
            main_chunks[chunk_idx],
        );
        chunk_idx += 1;

        // 2. Status Bar
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let hunk_counts = file.and_then(|fl| fl.hunks.get(cur_hunk).map(|h| h.counts()));
        let status_ctx = crate::view::StatusBarContext {
            cur_fi,
            total_files: self.output.files.len(),
            cur_hunk,
            total_hunks: file.map_or(0, |fl| fl.hunks.len().max(1)),
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            num_buffer: &self.num_buffer,
            common_prefix: common_prefix.as_deref(),
            // The bar paints into the scrollbar-narrowed app column, not
            // the full viewport: targeting full width clips the last cell
            // (usually the filename tail) under the scrollbar.
            width: main_width,
            is_streaming: self.is_streaming,
            streaming_dot: self.streaming_dot(),
            file,
            hunk_counts,
            theme: &self.theme,
        };
        let status_line = status_bar_line(status_ctx);
        let status_bg = crate::view::to_color(self.theme.palette.status_bar_bg);
        f.render_widget(
            Paragraph::new(status_line).style(Style::default().bg(status_bg)),
            main_chunks[chunk_idx],
        );
        chunk_idx += 1;

        // 3. Status Message
        if let Some((ref msg, is_ok)) = self.status_message {
            let color = if is_ok {
                crate::view::to_color(self.theme.palette.header_staged)
            } else {
                crate::view::to_color(self.theme.palette.count_dels)
            };
            let bg = crate::view::to_color(self.theme.palette.bg_base);
            let p = Paragraph::new(format!(" {msg}")).style(Style::default().fg(color).bg(bg));
            f.render_widget(p, main_chunks[chunk_idx]);
            chunk_idx += 1;
        }

        let diff_area = main_chunks[chunk_idx];
        chunk_idx += 1;
        let ribbon_area = main_chunks[chunk_idx];

        let Some(file) = file else {
            let bg = crate::view::to_color(self.theme.palette.bg_base);
            let p = Paragraph::new(" No diff to display.").style(
                Style::default()
                    .fg(crate::view::to_color(self.theme.palette.status_bar_dim))
                    .bg(bg),
            );
            f.render_widget(p, diff_area);

            let (div_line, rib_line) = bottom_ribbon_lines(main_width, &self.theme.palette);
            f.render_widget(
                Paragraph::new(vec![div_line, rib_line]).style(Style::default().bg(bg)),
                ribbon_area,
            );

            self.last_viewport_start = diff_area.y as usize;
            self.viewport_height = diff_area.height as usize;
            self.last_content_width = main_width;
            self.screen_height = track_h;

            if self.show_help {
                crate::view::render_help_modal_widget(f, &self.theme, area);
            }
            if let Some(ref finder) = self.file_finder {
                crate::view::render_finder_modal_widget(f, &self.output, finder, &self.theme, area);
            }
            return;
        };

        let file_path = file.path.clone();

        // 4. Diff Grid Viewport (no scrollbar chars here anymore; the bar
        // owns its own full-height column). Pass the full width: the
        // viewport fn deducts the bar column itself, so passing main_width
        // would subtract it twice and desync hit column from drawn column.
        let (mut grid_lines, has_scrollbar, thumb_offset, thumb_size, content_width) =
            self.render_diff_viewport_lines(width, diff_area.height as usize, track_h);

        for gline in grid_lines.iter_mut() {
            debug_assert!(
                visible_width(gline) <= content_width,
                "grid line visual width ({}) exceeds content_width ({})",
                visible_width(gline),
                content_width
            );
            *gline = clamp_or_pad_ansi(gline, content_width);
        }

        let paragraph_lines: Vec<ratatui::text::Line> = grid_lines
            .into_iter()
            .map(|l| crate::view::ansi_to_ratatui_line(&l))
            .collect();
        let diff_bg = crate::view::to_color(self.theme.palette.bg_base);
        let diff_fg = crate::view::to_color(self.theme.palette.syntax_text);
        f.render_widget(
            Paragraph::new(paragraph_lines).style(Style::default().fg(diff_fg).bg(diff_bg)),
            diff_area,
        );

        // 5. Full-height scrollbar column.
        if let Some(bar) = bar_area
            && has_scrollbar
        {
            let thumb_fg = self.theme.palette.scrollbar_thumb.fg_code();
            let track_fg = self.theme.palette.scrollbar_track.fg_code();
            let thumb_str = format!("{thumb_fg}█\x1b[0m");
            let track_str = format!("{track_fg}│\x1b[0m");
            let bar_strings: Vec<String> = (0..track_h)
                .map(|r| {
                    if r >= thumb_offset && r < thumb_offset + thumb_size {
                        thumb_str.clone()
                    } else {
                        track_str.clone()
                    }
                })
                .collect();
            let bar_lines: Vec<ratatui::text::Line> = bar_strings
                .iter()
                .map(|l| crate::view::ansi_to_ratatui_line(l))
                .collect();
            let track_bg = crate::view::to_color(self.theme.palette.scrollbar_track);
            f.render_widget(
                Paragraph::new(bar_lines).style(Style::default().bg(track_bg)),
                bar,
            );
        }

        // 6. Bottom Ribbon
        let (div_line, rib_line) = bottom_ribbon_lines(main_width, &self.theme.palette);
        let rib_bg = crate::view::to_color(self.theme.palette.bg_base);
        f.render_widget(
            Paragraph::new(vec![div_line, rib_line]).style(Style::default().bg(rib_bg)),
            ribbon_area,
        );

        // Update viewport metrics for diff area
        self.last_viewport_start = diff_area.y as usize;
        self.viewport_height = diff_area.height as usize;
        self.last_content_width = content_width;
        self.screen_height = track_h;

        // 6. Help Modal Overlay
        if self.show_help {
            crate::view::render_help_modal_widget(f, &self.theme, area);
        }

        // 7. File Finder Modal Overlay
        if let Some(ref finder) = self.file_finder {
            crate::view::render_finder_modal_widget(f, &self.output, finder, &self.theme, area);
        }

        // 8. Note Draft Modal Overlay
        if self.note_draft.is_some() || self.note_editor.is_some() {
            let loc = if let Some(sel) = self.compute_selection() {
                if sel.start == sel.end {
                    format!("{file_path}:{}", sel.start)
                } else {
                    format!("{file_path}:{}-{}", sel.start, sel.end)
                }
            } else {
                let line = self
                    .current_flat_row()
                    .and_then(|fr| self.flat_row_coord(fr))
                    .unwrap_or(0);
                format!("{file_path}:{line}")
            };
            let note_area = if !self.raw_diff && has_scrollbar {
                main_area
            } else {
                area
            };
            let draft = self.note_draft.as_deref().unwrap_or("");
            crate::view::render_note_modal_widget(
                f,
                &loc,
                draft,
                self.note_editor.as_mut(),
                &self.theme,
                note_area,
            );
        }
    }

    /// Render the diff rows for the current viewport slice.
    #[allow(clippy::type_complexity)]
    /// Origin of a rendered viewport grid line for width-invariant
    /// diagnostics: the owning file path and flat row, or a marker for
    /// padding and out-of-range rows.
    pub(crate) fn viewport_line_origin(&self, flat_idx: Option<usize>) -> String {
        let Some(i) = flat_idx else {
            return "viewport padding".to_string();
        };
        let Some(item) = self.flat.get(i) else {
            return format!("flat row {i} (out of range)");
        };
        let Some(file) = self.output.files.get(item.file_idx) else {
            return format!("flat row {i} (unknown file {})", item.file_idx);
        };
        format!("{} flat row {i}", file.path)
    }

    pub fn render_diff_viewport_lines(
        &mut self,
        width: usize,
        vh: usize,
        track_h: usize,
    ) -> (Vec<String>, bool, usize, usize, usize) {
        let frame_start = std::time::Instant::now();
        let max_rows = vh.max(4);
        let mut lines = Vec::new();
        // Flat-row origin of each pushed grid line for width-invariant
        // diagnostics (None for viewport padding).
        let mut line_src: Vec<Option<usize>> = Vec::new();

        let band = self.band_rows();
        let hunk_extent = self.hunk_ctx_extent_rows();
        let cur_hunk = self.current_hunk();
        let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
            HashMap::new()
        } else {
            self.hunk_starts
                .iter()
                .map(|h| (h.flat, (h.file, h.hunk)))
                .collect()
        };

        // Compute visual geometry with content_width, reusing the cached
        // layout when inputs are unchanged instead of remeasuring the
        // whole content every frame.
        let est_content_width = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };
        let mut geo = self.cached_geometry(est_content_width);

        let has_scrollbar = !self.raw_diff && geo.total_vis_lines > vh && vh > 2;
        let content_width = if has_scrollbar {
            width.saturating_sub(1)
        } else {
            width
        };
        self.terminal_width = width;
        self.last_content_width = content_width;

        if !has_scrollbar && content_width != est_content_width {
            geo = self.cached_geometry(content_width);
        }
        let super::geometry::CachedGeometry {
            starts,
            counts,
            seg_counts,
            nav_counts,
            total_vis_lines,
            callouts,
            ..
        } = geo;

        let cur_row_nav = nav_counts.get(self.row_idx).copied().unwrap_or(1);
        if self.vis_line >= cur_row_nav {
            self.vis_line = cur_row_nav.saturating_sub(1);
        }
        let min_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let cursor_vis = min_vis + self.vis_line;

        let mut pin_applied = false;
        if let Some((pinned, pin_row)) = self.scroll_pin.take()
            && self.row_idx == pin_row
        {
            self.scroll = pinned;
            pin_applied = true;
        }
        if !pin_applied {
            let top_vis = if self.vis_line == 0 {
                min_vis
            } else {
                cursor_vis
            };
            if top_vis < self.scroll {
                self.scroll = top_vis;
            }
            if cursor_vis >= self.scroll + max_rows {
                self.scroll = cursor_vis.saturating_sub(max_rows.saturating_sub(1));
            }
        }
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);

        // Viewport iteration, seeking the first visible row by binary
        // search over visual prefix sums instead of scanning from the top
        // every frame. Rows tile visual space exactly
        // (`starts[i+1] == starts[i] + counts[i]`), so the row containing
        // `scroll` is the last one starting at or before it.
        let first_idx = if self.flat.is_empty() {
            0
        } else {
            starts
                .partition_point(|&s| s <= self.scroll)
                .saturating_sub(1)
        };
        let mut vis_i = starts.get(first_idx).copied().unwrap_or(0);
        for (idx, item) in self.flat.iter().enumerate().skip(first_idx) {
            let count = counts[idx];
            if vis_i + count <= self.scroll {
                vis_i += count;
                continue;
            }
            if vis_i >= self.scroll + max_rows {
                break;
            }

            let in_band = band[idx];
            let in_hunk_extent = !in_band && hunk_extent[idx];

            let div_segs = if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                let h = &self.output.files[div_fi].hunks[div_hi];
                let hidden_gap = self.hunk_leading_gap(div_fi, div_hi);
                let is_cur = div_fi == self.current_file_idx() && cur_hunk == Some(div_hi);
                let d_segs = self.hunk_div_seg_count(div_fi, div_hi, content_width);
                let text_cols = content_width.max(1);
                for s_idx in 0..d_segs {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let seg_start = s_idx * text_cols;
                        line_src.push(Some(idx));
                        lines.push(crate::view::diff_grid::render_hunk_divider_segment(
                            crate::render::Badge {
                                idx: div_hi,
                                total: self.output.files[div_fi].hunks.len(),
                            },
                            h,
                            hidden_gap,
                            is_cur,
                            is_selected,
                            seg_start,
                            content_width,
                            &self.theme,
                        ));
                    }
                    vis_i += 1;
                }
                d_segs
            } else {
                0
            };

            if item.is_file_header {
                let file_obj = &self.output.files[item.file_idx];
                let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                let seg_count = seg_counts[idx];
                let common_prefix = self.output.common_directory_prefix();
                for s_idx in 0..seg_count {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let is_current = item.file_idx == self.current_file_idx();
                        let text_cols = content_width.max(1);
                        let seg_start = s_idx * text_cols;
                        line_src.push(Some(idx));
                        lines.push(crate::view::diff_grid::render_file_header_segment(
                            file_obj,
                            crate::render::Badge {
                                idx: item.file_idx,
                                total: self.output.files.len(),
                            },
                            is_collapsed,
                            is_selected,
                            is_current,
                            in_band,
                            seg_start,
                            text_cols,
                            &self.theme,
                            common_prefix.as_deref(),
                        ));
                    }
                    vis_i += 1;
                }
            } else if item.is_hunk_divider {
                let seg_count = seg_counts[idx];
                let hunks = if !self.output.files[item.file_idx].raw_hunks.is_empty() {
                    &self.output.files[item.file_idx].raw_hunks
                } else {
                    &self.output.files[item.file_idx].hunks
                };
                let h = &hunks[item.hunk_idx];
                let sep_text = format!(
                    "@@ -{},{} +{},{} @@",
                    h.old_start, h.old_count, h.new_start, h.new_count
                );
                for s_idx in 0..seg_count {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let text_cols = content_width.max(1);
                        let start_col = s_idx * text_cols;
                        let piece = slice_by_visual_width(&sep_text, start_col, text_cols);
                        let is_last_seg = s_idx + 1 == seg_count;
                        line_src.push(Some(idx));
                        lines.push(render_raw_row(
                            RowKind::Ctx,
                            "",
                            &piece,
                            is_selected,
                            in_band && !is_selected,
                            is_last_seg,
                            &self.theme,
                        ));
                    }
                    vis_i += 1;
                }
            } else if let Some(r) = self.row_of(item) {
                let seg_count = seg_counts[idx];
                for s_idx in 0..seg_count {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == div_segs + s_idx;
                        let pfx = if s_idx == 0 {
                            crate::render::styled_prefix(
                                r.kind,
                                r.mov_side,
                                r.group,
                                r.new_line,
                                r.old_line,
                                &r.runs,
                                in_hunk_extent,
                                &self.theme,
                            )
                        } else {
                            "\x1b[38;5;242m      \x1b[39m".to_string()
                        };
                        let rendered_row = if self.raw_diff {
                            let prefix_str = pfx.as_str();
                            let text_cols = content_width.saturating_sub(6).max(1);
                            let start_col = s_idx * text_cols;
                            let piece = slice_by_visual_width(&r.text, start_col, text_cols);
                            let is_last_seg = s_idx + 1 == seg_count;
                            render_raw_row(
                                r.kind,
                                prefix_str,
                                &piece,
                                is_selected,
                                in_band && !is_selected,
                                is_last_seg,
                                &self.theme,
                            )
                        } else {
                            let text_cols = content_width.saturating_sub(6).max(1);
                            let seg_start = s_idx * text_cols;
                            let seg_end = seg_start + text_cols;
                            render_row_segment(
                                &r.text,
                                r.del_text.as_deref(),
                                &r.spans,
                                r.kind,
                                &r.runs,
                                seg_start,
                                seg_end,
                                &pfx,
                                is_selected,
                                in_band && !is_selected,
                                self.col_offset,
                                self.word_diff,
                                self.syntax_on,
                                &self.theme,
                            )
                        };
                        line_src.push(Some(idx));
                        lines.push(rendered_row);
                    }
                    vis_i += 1;
                }
            }

            if let Some(note_lines) = callouts.get(&idx) {
                let focused_note_range = if idx == self.row_idx {
                    match self.visual_target_at(self.row_idx, self.vis_line) {
                        Some(super::notes::VisualTarget::Note {
                            line_idx,
                            total_note_lines,
                            ..
                        }) => {
                            let note_start = div_segs + seg_counts[idx];
                            if self.vis_line >= note_start {
                                let start = (self.vis_line - note_start).saturating_sub(line_idx);
                                Some(start..start + total_note_lines)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                for (nl_idx, nline) in note_lines.iter().enumerate() {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_focused = focused_note_range
                            .as_ref()
                            .is_some_and(|r| r.contains(&nl_idx));
                        let is_current_line = idx == self.row_idx
                            && self.vis_line == div_segs + seg_counts[idx] + nl_idx;
                        line_src.push(Some(idx));
                        let line_str = if is_focused {
                            super::notes::style_focused_note_line(nline, is_current_line)
                        } else {
                            nline.clone()
                        };
                        lines.push(clamp_or_pad_ansi(&line_str, content_width));
                    }
                    vis_i += 1;
                }
            }

            if !self.raw_diff
                && idx == self.last_row_of_file(item.file_idx)
                && self.hunk_trailing_gap(item.file_idx) > 0
            {
                let gap = self.hunk_trailing_gap(item.file_idx);
                let bot_segs = self.bottom_gap_seg_count(item.file_idx, content_width);
                let is_cur = item.file_idx == self.current_file_idx() && idx == self.row_idx;
                let text_cols = content_width.max(1);
                let note_count = callouts.get(&idx).map_or(0, std::vec::Vec::len);
                let bottom_div_vis_start = div_segs + seg_counts[idx] + note_count;
                for s_idx in 0..bot_segs {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected =
                            idx == self.row_idx && self.vis_line == bottom_div_vis_start + s_idx;
                        let seg_start = s_idx * text_cols;
                        let bottom_div = crate::view::diff_grid::render_bottom_gap_divider_segment(
                            gap,
                            is_cur,
                            is_selected,
                            seg_start,
                            content_width,
                            &self.theme,
                        );
                        line_src.push(Some(idx));
                        lines.push(bottom_div);
                    }
                    vis_i += 1;
                }
            }
        }

        while lines.len() < vh {
            line_src.push(None);
            lines.push(String::new());
        }

        if cfg!(debug_assertions) {
            for (idx, line) in lines.iter().enumerate() {
                let vw = visible_width(line);
                debug_assert!(
                    vw <= content_width,
                    "render_diff_viewport_lines produced grid line {idx} ({}) with visual width ({vw}) > content_width ({content_width}):\n\n{}",
                    self.viewport_line_origin(line_src.get(idx).copied().flatten()),
                    escape_ansi_for_diagnostic(line),
                );
            }
        }

        let (thumb_offset, thumb_size) = if has_scrollbar {
            let header_vis = counts.first().copied().unwrap_or(1);
            Self::scrollbar_thumb_for(track_h, vh, total_vis_lines, self.scroll, header_vis)
                .unwrap_or((0, 0))
        } else {
            (0, 0)
        };

        tracing::trace!(
            scroll = self.scroll,
            rows = lines.len(),
            elapsed_ms = frame_start.elapsed().as_millis(),
            "viewport frame rendered"
        );

        (
            lines,
            has_scrollbar,
            thumb_offset,
            thumb_size,
            content_width,
        )
    }
}
