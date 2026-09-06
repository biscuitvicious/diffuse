use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::Paragraph,
};

use crate::{
    core::parse::RowKind,
    render::{render_raw_row, render_row_segment, slice_by_visual_width},
    view::{
        bottom_ribbon_lines,
        header::{
            render_bottom_ribbon, render_header_bar, render_status_bar, status_bar_line,
            visible_width,
        },
        modals::{render_finder_modal, render_help_modal},
    },
};

use super::AppState;

impl AppState {
    /// Render full visual screen frame as a String of ANSI terminal lines.
    pub fn render_frame(&mut self, width: usize, height: usize) -> String {
        let max_rows = height.saturating_sub(4).max(4);
        self.viewport_height = max_rows;
        let mut lines = Vec::new();

        let cur_fi = self.current_file_idx();
        let f = match self.output.files.get(cur_fi) {
            Some(file) => file,
            None => {
                return "\x1b[2J\x1b[H No diff to display.".to_string();
            }
        };

        let common_prefix = self.output.common_directory_prefix();

        // Header bar
        lines.extend(render_header_bar(
            &self.repo,
            f,
            width,
            common_prefix.as_deref(),
        ));

        // Secondary status bar
        let total_files = self.output.files.len();
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let total_hunks = f.hunks.len().max(1);
        lines.extend(render_status_bar(crate::view::StatusBarContext {
            cur_fi,
            total_files,
            cur_hunk,
            total_hunks,
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            num_buffer: &self.num_buffer,
            branch: &self.output.branch,
            common_prefix: common_prefix.as_deref(),
            width,
            is_streaming: self.is_streaming,
        }));

        // Ephemeral action feedback (note saved/deleted, git ops, ...)
        if let Some((msg, is_ok)) = self.status_message.clone() {
            lines.push(format!(
                "\x1b[{}m {msg}\x1b[39m",
                if is_ok { "38;5;120" } else { "38;5;203" }
            ));
        }

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

        let (mut grid_lines, has_scrollbar, thumb_offset, thumb_size, content_width) =
            self.render_diff_viewport_lines(width, vh);

        if !self.raw_diff {
            for (row_i, gline) in grid_lines.iter_mut().enumerate() {
                let vis_w = visible_width(gline);
                if vis_w < content_width {
                    gline.push_str(&" ".repeat(content_width - vis_w));
                }
                gline.push_str("\x1b[0m");
                if has_scrollbar {
                    let sb_char = if row_i >= thumb_offset && row_i < thumb_offset + thumb_size {
                        "\x1b[38;5;248m█\x1b[0m"
                    } else {
                        "\x1b[38;5;236m│\x1b[0m"
                    };
                    gline.push_str(sb_char);
                }
            }
        }
        lines.extend(grid_lines);

        let (divider, ribbon) = render_bottom_ribbon(width);
        lines.push(divider);
        lines.push(ribbon);

        // Help modal overlays the full screen when toggled on.
        if self.show_help {
            lines = render_help_modal(width, height);
        }

        // File finder dialog overlays the full screen with top finder + bottom
        // preview when active.
        if let Some(finder) = &self.file_finder {
            lines = render_finder_modal(&self.output, finder, width, height);
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
        let (_, _, _, total_vis_lines) = self.visual_geometry(content_width);
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
        let file = match self.output.files.get(cur_fi) {
            Some(file) => file,
            None => {
                let p = Paragraph::new(" No diff to display.")
                    .style(Style::default().fg(Color::Indexed(244)));
                f.render_widget(p, area);
                return;
            }
        };

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
            .constraints(constraints)
            .split(area);

        let mut chunk_idx = 0;

        let file_path = file.path.clone();

        // 1. Header Bar
        let common_prefix = self.output.common_directory_prefix();
        let header_line = crate::view::header_bar_line_themed(
            &self.repo,
            file,
            &self.theme,
            common_prefix.as_deref(),
        );
        let header_bg = crate::view::to_color(self.theme.palette.header_bar_bg);
        f.render_widget(
            Paragraph::new(header_line).style(Style::default().bg(header_bg)),
            chunks[chunk_idx],
        );
        chunk_idx += 1;

        // 2. Status Bar
        let status_ctx = crate::view::StatusBarContext {
            cur_fi,
            total_files: self.output.files.len(),
            cur_hunk: self.current_hunk().unwrap_or(0),
            total_hunks: file.hunks.len().max(1),
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            num_buffer: &self.num_buffer,
            branch: &self.output.branch,
            common_prefix: common_prefix.as_deref(),
            width,
            is_streaming: self.is_streaming,
        };
        let status_line = status_bar_line(status_ctx);
        f.render_widget(Paragraph::new(status_line), chunks[chunk_idx]);
        chunk_idx += 1;

        // 3. Status Message
        if let Some((ref msg, is_ok)) = self.status_message {
            let color = if is_ok {
                Color::Indexed(120)
            } else {
                Color::Indexed(203)
            };
            let p = Paragraph::new(format!(" {msg}")).style(Style::default().fg(color));
            f.render_widget(p, chunks[chunk_idx]);
            chunk_idx += 1;
        }

        let diff_area = chunks[chunk_idx];
        chunk_idx += 1;
        let ribbon_area = chunks[chunk_idx];

        // 4. Diff Grid Viewport
        let (mut grid_lines, has_scrollbar, thumb_offset, thumb_size, content_width) =
            self.render_diff_viewport_lines(width, diff_area.height as usize);

        if !self.raw_diff {
            let thumb_fg = self.theme.palette.scrollbar_thumb.fg_code();
            let track_fg = self.theme.palette.scrollbar_track.fg_code();
            let thumb_str = format!("{thumb_fg}█\x1b[0m");
            let track_str = format!("{track_fg}│\x1b[0m");

            for (row_i, gline) in grid_lines.iter_mut().enumerate() {
                let vis_w = visible_width(gline);
                if vis_w < content_width {
                    gline.push_str(&" ".repeat(content_width - vis_w));
                }
                gline.push_str("\x1b[0m");
                if has_scrollbar {
                    let sb_char = if row_i >= thumb_offset && row_i < thumb_offset + thumb_size {
                        &thumb_str
                    } else {
                        &track_str
                    };
                    gline.push_str(sb_char);
                }
            }
        } else {
            for gline in grid_lines.iter_mut() {
                let vis_w = visible_width(gline);
                if vis_w < content_width {
                    gline.push_str(&" ".repeat(content_width - vis_w));
                }
                gline.push_str("\x1b[0m");
            }
        }

        let paragraph_lines: Vec<ratatui::text::Line> = grid_lines
            .into_iter()
            .map(|l| crate::view::ansi_to_ratatui_line(&l))
            .collect();
        f.render_widget(Paragraph::new(paragraph_lines), diff_area);

        // 5. Bottom Ribbon
        let (div_line, rib_line) = bottom_ribbon_lines(width);
        f.render_widget(Paragraph::new(vec![div_line, rib_line]), ribbon_area);

        // Update viewport metrics for diff area
        self.last_viewport_start = diff_area.y as usize;
        self.viewport_height = diff_area.height as usize;
        self.last_content_width = content_width;

        // 6. Help Modal Overlay
        if self.show_help {
            crate::view::render_help_modal_widget(f, area);
        }

        // 7. File Finder Modal Overlay
        if let Some(ref finder) = self.file_finder {
            crate::view::render_finder_modal_widget(f, &self.output, finder, area);
        }

        // 8. Note Draft Modal Overlay
        if let Some(ref draft) = self.note_draft {
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
                Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width.saturating_sub(1),
                    height: area.height,
                }
            } else {
                area
            };
            crate::view::render_note_modal_widget(f, &loc, draft, note_area);
        }
    }

    /// Render the diff rows for the current viewport slice.
    #[allow(clippy::type_complexity)]
    pub fn render_diff_viewport_lines(
        &mut self,
        width: usize,
        vh: usize,
    ) -> (Vec<String>, bool, usize, usize, usize) {
        let max_rows = vh.max(4);
        let mut lines = Vec::new();

        let callouts = self.note_callouts();
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

        // Compute visual geometry with content_width
        let est_content_width = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };
        let (mut starts, mut counts, mut seg_counts, mut total_vis_lines) =
            self.visual_geometry(est_content_width);

        let has_scrollbar = !self.raw_diff && total_vis_lines > vh && vh > 2;
        let content_width = if has_scrollbar {
            width.saturating_sub(1)
        } else {
            width
        };
        self.terminal_width = width;
        self.last_content_width = content_width;

        if !has_scrollbar && content_width != est_content_width {
            let res = self.visual_geometry(content_width);
            starts = res.0;
            counts = res.1;
            seg_counts = res.2;
            total_vis_lines = res.3;
        }

        // Adjust scroll based on cursor visual position
        let hunk_div_offset = if let Some(&(div_fi, div_hi)) = div_at.get(&self.row_idx) {
            self.hunk_div_seg_count(div_fi, div_hi, content_width)
        } else {
            0
        };
        let cur_row_segs = seg_counts.get(self.row_idx).copied().unwrap_or(1);
        if self.vis_line >= cur_row_segs {
            self.vis_line = cur_row_segs.saturating_sub(1);
        }
        let min_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let cursor_vis = min_vis + hunk_div_offset + self.vis_line;

        let mut pin_applied = false;
        if let Some((pinned, pin_row)) = self.scroll_pin {
            if self.row_idx == pin_row {
                self.scroll = pinned;
                pin_applied = true;
            } else {
                self.scroll_pin = None;
            }
        }
        if !pin_applied {
            if min_vis < self.scroll {
                self.scroll = min_vis;
            }
            if cursor_vis >= self.scroll + max_rows {
                self.scroll = cursor_vis.saturating_sub(max_rows.saturating_sub(1));
            }
        }
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);

        // Viewport iteration
        let mut vis_i = 0;
        for (idx, item) in self.flat.iter().enumerate() {
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

            if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                let h = &self.output.files[div_fi].hunks[div_hi];
                let hidden_gap = self.hunk_leading_gap(div_fi, div_hi);
                let is_cur = div_fi == self.current_file_idx() && cur_hunk == Some(div_hi);
                let div_segs = self.hunk_div_seg_count(div_fi, div_hi, content_width);
                let text_cols = content_width.max(1);
                for s_idx in 0..div_segs {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let seg_start = s_idx * text_cols;
                        lines.push(crate::view::diff_grid::render_hunk_divider_segment_themed(
                            div_hi,
                            h,
                            hidden_gap,
                            is_cur,
                            seg_start,
                            content_width,
                            &self.theme,
                        ));
                    }
                    vis_i += 1;
                }
            }

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
                        lines.push(crate::view::diff_grid::render_file_header_segment_themed(
                            file_obj,
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
                        lines.push(render_raw_row(
                            RowKind::Ctx,
                            "",
                            &piece,
                            is_selected,
                            in_band && !is_selected,
                            is_last_seg,
                        ));
                    }
                    vis_i += 1;
                }
            } else if let Some(r) = self.row_of(item) {
                let seg_count = seg_counts[idx];
                for s_idx in 0..seg_count {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let pfx = if s_idx == 0 {
                            crate::render::styled_prefix_themed(
                                r.kind,
                                r.mov_side,
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
                        lines.push(rendered_row);
                    }
                    vis_i += 1;
                }

                if !self.raw_diff
                    && idx == self.last_row_of_file(item.file_idx)
                    && self.hunk_trailing_gap(item.file_idx) > 0
                {
                    let gap = self.hunk_trailing_gap(item.file_idx);
                    let bot_segs = self.bottom_gap_seg_count(item.file_idx, content_width);
                    let is_cur = item.file_idx == self.current_file_idx() && idx == self.row_idx;
                    let text_cols = content_width.max(1);
                    for s_idx in 0..bot_segs {
                        if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                            let seg_start = s_idx * text_cols;
                            let bottom_div =
                                crate::view::diff_grid::render_bottom_gap_divider_segment(
                                    gap,
                                    is_cur,
                                    seg_start,
                                    content_width,
                                );
                            lines.push(bottom_div);
                        }
                        vis_i += 1;
                    }
                }
            }

            if let Some(note_lines) = callouts.get(&idx) {
                for nline in note_lines {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        lines.push(nline.clone());
                    }
                    vis_i += 1;
                }
            }
        }

        while lines.len() < vh {
            lines.push(String::new());
        }

        let (thumb_size, thumb_offset) = if has_scrollbar {
            let thumb_size = ((vh as f64 / total_vis_lines as f64) * vh as f64)
                .round()
                .clamp(1.0, (vh.saturating_sub(1)) as f64) as usize;
            let max_scroll = total_vis_lines.saturating_sub(vh);
            let thumb_offset = if max_scroll == 0 {
                0
            } else {
                ((self.scroll as f64 / max_scroll as f64) * (vh.saturating_sub(thumb_size)) as f64)
                    .round() as usize
            };
            (thumb_size, thumb_offset)
        } else {
            (0, 0)
        };

        (
            lines,
            has_scrollbar,
            thumb_offset,
            thumb_size,
            content_width,
        )
    }
}
