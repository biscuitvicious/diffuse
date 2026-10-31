//! Full terminal frame generation and raw unified-diff mode toggling.

use crate::{
    app::AppState,
    core::notes::DiffCoord,
    render::{ColorExt, ThemePalette, ansi::optimize_ansi},
    view::{
        clamp_or_pad_ansi,
        header::{render_bottom_ribbon, render_header_bar, render_status_bar},
        modals::{render_finder_modal, render_help_modal},
        visible_width,
    },
};

/// Render ANSI escape bytes as the literal text `\x1b` so diagnostic
/// messages stay readable instead of emitting raw control codes.
pub(crate) fn escape_ansi_for_diagnostic(s: &str) -> String {
    s.replace('\x1b', "\\x1b")
}

/// Blank secondary line keeping the status background when empty, so the
/// reserved slot never changes color or height.
pub(crate) fn blank_status_ansi(width: usize, palette: &ThemePalette) -> String {
    format!(
        "{}{}\x1b[0m",
        palette.status_bar_bg.bg_code(),
        " ".repeat(width)
    )
}

impl AppState {
    /// Render full visual screen frame as a String of ANSI terminal lines.
    pub fn render_frame(&mut self, width: usize, height: usize) -> String {
        let max_rows = height.saturating_sub(4).max(4);
        self.viewport_height = max_rows;
        let mut lines = Vec::new();

        self.poll_status_expiry(std::time::Instant::now());

        let cur_fi = self.current_file_idx();
        let f = self.output.files.get(cur_fi);

        let common_prefix = self.output.common_directory_prefix();

        // Header bar
        lines.extend(render_header_bar(
            &self.repo,
            width,
            Some(self.output.total_counts()),
            Some(&self.output.branch),
            self.commit_range.as_deref(),
            self.target.is_staged(),
            &self.theme.palette,
        ));

        let status_msg = self.visible_status_message();
        let total_files = self.output.files.len();
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let total_hunks = f.map_or(0, |file| file.hunks.len().max(1));
        let hunk_counts = f.and_then(|file| file.hunks.get(cur_hunk).map(|h| h.counts()));
        let status_lines = render_status_bar(crate::view::StatusBarContext {
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
            status_message: status_msg,
            file: f,
            hunk_counts,
            theme: &self.theme,
        });

        if let Some(f) = f {
            // Secondary status bar above the viewport; the slot is always
            // reserved with the status background so showing/hiding never
            // shifts diff rows nor changes the bar color.
            if status_lines.is_empty() {
                lines.push(blank_status_ansi(width, &self.theme.palette));
            } else {
                lines.extend(status_lines);
            }
            // Inline note draft if active
            if let Some(ref draft) = self.note_draft {
                let loc = if let Some(sel) = self.compute_selection() {
                    sel.loc_label(&f.path)
                } else {
                    let coord = self
                        .current_flat_row()
                        .and_then(|fr| self.row_of(fr))
                        .and_then(DiffCoord::from_row)
                        .map_or_else(String::new, DiffCoord::label);
                    format!("{}:{coord}", f.path)
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
                    let (_, counts, _, total_vis_lines) = self.cached_geometry_tuple(content_width);
                    let header_vis = counts.first().copied().unwrap_or(1);
                    let subcell = Self::scrollbar_subcell_for(
                        height,
                        vh,
                        total_vis_lines,
                        self.scroll,
                        header_vis,
                    );
                    // Thumb geometry is full-track based: grid row i sits
                    // at track row viewport_start_line + i.
                    let track_row = viewport_start_line + row_i;
                    let sb_char = if let Some(sub) = subcell {
                        sub.cell_at(track_row).render_ansi(&self.theme.palette)
                    } else if track_row >= thumb_offset && track_row < thumb_offset + thumb_size {
                        "\x1b[38;5;248m█\x1b[0m".to_string()
                    } else {
                        "\x1b[38;5;236m│\x1b[0m".to_string()
                    };
                    gline.push_str(&sb_char);
                }
            }

            lines.extend(grid_lines);
        } else {
            // Secondary slot above the message area, always with the
            // status background. The empty-state notice lives in the
            // secondary bar itself (persistent, no timeout); the main
            // area stays blank so geometry never shifts.
            if status_lines.is_empty() {
                lines.push(blank_status_ansi(width, &self.theme.palette));
            } else {
                lines.extend(status_lines);
            }
            let viewport_start_line = lines.len();
            let vh = height.saturating_sub(viewport_start_line + 2);
            self.last_viewport_start = viewport_start_line;
            self.viewport_height = vh;
            self.screen_height = height;

            for _ in 0..vh {
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
        // Drop redundant SGR state frame-wide (grid, header, ribbon,
        // modals): identical terminal effect per line, smaller payload.
        optimize_ansi(&out).into_owned()
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
        let is_sep_row = self
            .current_flat_row()
            .is_some_and(|fr| fr.is_hunk_divider());

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
                        .is_some_and(|fr| fr.is_hunk_divider())
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
                if item.file_idx != file || !item.is_diff_row() {
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
}
