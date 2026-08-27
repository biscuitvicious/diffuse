//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use diffuse_core::{
    notes::NotesDB,
    wire::{OutputFile, PipelineOutput},
};
use diffuse_render::{
    DiffuseTheme, render_raw_row, render_row_segment, slice_by_visual_width, styled_prefix,
};

use crate::{
    command::TuiCommand,
    types::{FlatRow, HunkMeta, HunkStart, Selection},
    view::{
        file_header_text,
        header::{render_bottom_ribbon, render_header_bar, render_status_bar, visible_width},
        hunk_divider_text,
        modals::{render_finder_modal, render_help_modal},
        render_bottom_gap_divider, render_file_header_segment, render_hunk_divider_segment,
    },
};

mod anchor;
mod collapse;
mod context;
mod finder;
mod flat;
mod geometry;
mod navigation;
mod notes;
mod query;
mod selection;
mod sync;

pub struct AppState {
    pub output: PipelineOutput,
    pub flat: Vec<FlatRow>,
    pub row_idx: usize,
    pub vis_line: usize,
    pub scroll: usize,
    pub col_offset: usize,
    pub num_buffer: String,
    pub syntax_on: bool,
    pub word_diff: bool,
    pub raw_diff: bool,
    pub show_help: bool,
    pub note_draft: Option<String>,
    pub anchor: Option<usize>,
    pub sel_pivot: Option<usize>,
    pub confirmed_sel: Option<Selection>,
    pub theme: DiffuseTheme,
    pub repo: String,
    pub should_quit: bool,
    pub status_message: Option<(String, bool)>, // (message, is_error)
    pub viewport_height: usize,
    pub notes: NotesDB,
    pub notes_path: PathBuf,
    pub hunk_starts: Vec<HunkStart>,
    hunk_meta: Vec<Vec<HunkMeta>>,
    note_id_counter: u64,
    pub repo_root: PathBuf,
    pub git_args: Vec<String>,
    pub pending_editor: Option<crate::editor::EditorRequest>,
    pub file_finder: Option<crate::finder::FileFinderState>,
    pub collapsed_files: HashSet<usize>,
    pub last_content_width: usize,
    pub terminal_width: usize,
    pub is_dragging_scrollbar: bool,
    pub last_viewport_start: usize,
    pub is_streaming: bool,
    pub expand_lines: usize,
    /// Viewport pin set by programmatic view pivots (context expansion):
    /// `(scroll, row_idx_at_pin_time)`. Every `render_frame` applies it
    /// verbatim instead of re-clamping around the cursor — until the cursor
    /// moves or new user input arrives (see `execute`).
    scroll_pin: Option<(usize, usize)>,
}

impl AppState {
    pub fn new(output: PipelineOutput, repo: String, notes_path: PathBuf) -> Self {
        let mut collapsed_files = std::collections::HashSet::new();
        for (fi, f) in output.files.iter().enumerate() {
            if f.status == "untracked" {
                collapsed_files.insert(fi);
            }
        }

        let (flat, hunk_starts) = Self::build_visual_flat(&output, &collapsed_files);

        // Start on first coordinate row if available, or first row (e.g. header)
        let start_row = flat
            .iter()
            .position(|r| Self::flat_coord(&output, false, r).is_some())
            .unwrap_or(0);

        let hunk_meta = Self::build_hunk_meta(&output);

        Self {
            output,
            flat,
            row_idx: start_row,
            vis_line: 0,
            scroll: 0,
            col_offset: 0,
            num_buffer: String::new(),
            syntax_on: true,
            word_diff: true,
            raw_diff: false,
            show_help: false,
            note_draft: None,
            anchor: None,
            sel_pivot: None,
            confirmed_sel: None,
            theme: DiffuseTheme::MAGIC,
            repo,
            should_quit: false,
            status_message: None,
            viewport_height: 20,
            notes: diffuse_core::notes::load_notes(&notes_path),
            notes_path,
            hunk_starts,
            hunk_meta,
            note_id_counter: 0,
            repo_root: PathBuf::from("."),
            git_args: Vec::new(),
            pending_editor: None,
            file_finder: None,
            collapsed_files,
            last_content_width: 80,
            terminal_width: 80,
            is_dragging_scrollbar: false,
            last_viewport_start: 2,
            is_streaming: false,
            expand_lines: diffuse_core::DEFAULT_EXPAND_LINES,
            scroll_pin: None,
        }
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
        self
    }

    pub fn with_repo_context(mut self, repo_root: PathBuf, git_args: Vec<String>) -> Self {
        self.repo_root = repo_root;
        self.git_args = git_args;
        self
    }

    pub fn current_file_idx(&self) -> usize {
        self.flat.get(self.row_idx).map(|r| r.file_idx).unwrap_or(0)
    }

    pub fn current_file(&self) -> Option<&OutputFile> {
        self.output.files.get(self.current_file_idx())
    }

    pub fn current_flat_row(&self) -> Option<&FlatRow> {
        self.flat.get(self.row_idx)
    }

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

        // Header bar
        lines.extend(render_header_bar(&self.repo, f, width));

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
            let loc = format!(
                "{}:{}",
                f.path,
                self.current_flat_row()
                    .and_then(|fr| self.flat_row_coord(fr))
                    .unwrap_or(0)
            );
            lines.push(format!(
                "\x1b[38;5;220m Note on {loc}: {draft}_  [ctrl+enter save · esc cancel]\x1b[39m"
            ));
        }

        let viewport_start_line = lines.len();
        let vh = height.saturating_sub(viewport_start_line + 2);
        let max_rows = vh.max(4);
        self.last_viewport_start = viewport_start_line;
        self.viewport_height = vh;

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
        let hunk_div_offset = if div_at.contains_key(&self.row_idx) {
            1
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
            // A programmatic pivot (context expansion) pinned the viewport;
            // honour it on every render instead of re-clamping around the
            // cursor — until the cursor moves or new input arrives.
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
        let mut scrollbar_chars = Vec::new();
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
                let txt = hunk_divider_text(div_hi, h, hidden_gap);
                let w = visible_width(&txt);
                let segs = ((w + content_width - 1) / content_width.max(1)).max(1);
                for s_idx in 0..segs {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        lines.push(render_hunk_divider_segment(
                            div_hi,
                            h,
                            hidden_gap,
                            is_cur,
                            s_idx * content_width,
                            content_width,
                        ));
                        scrollbar_chars.push("\x1b[38;5;236m│\x1b[0m".to_string());
                    }
                    vis_i += 1;
                }
            }

            if item.is_file_header {
                let file_obj = &self.output.files[item.file_idx];
                let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                let is_selected = idx == self.row_idx;
                let is_current = item.file_idx == self.current_file_idx();
                let txt = file_header_text(file_obj, is_collapsed);
                let w = visible_width(&txt);
                let segs = ((w + content_width - 1) / content_width.max(1)).max(1);
                for s_idx in 0..segs {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        lines.push(render_file_header_segment(
                            file_obj,
                            is_collapsed,
                            is_selected,
                            is_current,
                            in_band,
                            s_idx * content_width,
                            content_width,
                        ));
                        scrollbar_chars.push("\x1b[38;5;236m│\x1b[0m".to_string());
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
                            diffuse_core::parse::RowKind::Ctx,
                            "",
                            &piece,
                            is_selected,
                            in_band && !is_selected,
                            is_last_seg,
                        ));
                        scrollbar_chars.push("\x1b[38;5;236m│\x1b[0m".to_string());
                    }
                    vis_i += 1;
                }
            } else if let Some(r) = self.row_of(item) {
                let seg_count = seg_counts[idx];
                for s_idx in 0..seg_count {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let pfx = if s_idx == 0 {
                            styled_prefix(
                                r.kind,
                                r.new_line,
                                r.old_line,
                                r.moved,
                                &r.runs,
                                in_hunk_extent,
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
                                r.moved,
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
                        scrollbar_chars.push("\x1b[38;5;236m│\x1b[0m".to_string());
                    }
                    vis_i += 1;
                }

                if !self.raw_diff
                    && idx == self.last_row_of_file(item.file_idx)
                    && self.hunk_trailing_gap(item.file_idx) > 0
                {
                    if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                        let is_cur =
                            item.file_idx == self.current_file_idx() && idx == self.row_idx;
                        let gap = self.hunk_trailing_gap(item.file_idx);
                        let bottom_div = render_bottom_gap_divider(gap, is_cur, content_width);
                        lines.push(bottom_div);
                        scrollbar_chars.push("\x1b[38;5;236m│\x1b[0m".to_string());
                    }
                    vis_i += 1;
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

        // Fill remaining height with empty rows if total rows < max_rows
        while lines.len() < height.saturating_sub(2) {
            lines.push(String::new());
        }

        // Visual Scrollbar overlay and line padding across all viewport lines
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

        if !self.raw_diff {
            for row_i in 0..vh {
                let line_idx = viewport_start_line + row_i;
                if line_idx < lines.len() {
                    let vis_w = visible_width(&lines[line_idx]);
                    if vis_w < content_width {
                        lines[line_idx].push_str(&" ".repeat(content_width - vis_w));
                    }
                    lines[line_idx].push_str("\x1b[0m");
                    if has_scrollbar {
                        let sb_char = if row_i >= thumb_offset && row_i < thumb_offset + thumb_size
                        {
                            "\x1b[38;5;248m█\x1b[0m"
                        } else {
                            "\x1b[38;5;236m│\x1b[0m"
                        };
                        lines[line_idx].push_str(sb_char);
                    }
                }
            }
        }

        let (divider, ribbon) = render_bottom_ribbon(width);
        lines.push(divider);
        lines.push(ribbon);

        // Help modal overlays the frame when toggled on.
        if self.show_help {
            let modal = render_help_modal(width);
            let start = lines.len().saturating_sub(modal.len()) / 2;
            for (i, mline) in modal.iter().enumerate() {
                let idx = start + i;
                if idx < lines.len() {
                    lines[idx] = mline.clone();
                }
            }
        }

        // File finder dialog overlays the frame when active.
        if let Some(finder) = &self.file_finder {
            let modal = render_finder_modal(&self.output, finder, width);
            let start = lines.len().saturating_sub(modal.len()) / 2;
            for (i, mline) in modal.iter().enumerate() {
                let idx = start + i;
                if idx < lines.len() {
                    lines[idx] = mline.clone();
                }
            }
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
                // Transitioning from raw to visual: use preceding gap row if present
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
                // Transitioning from visual to raw: use hunk_start separator row
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
        // Priority 5: Proximity fallback (closest line in the same file if target is in
        // a gap)
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
    }
}

impl AppState {
    /// Execute a high-level user command to mutate application state.
    pub fn execute(&mut self, cmd: TuiCommand) {
        // New user input invalidates a viewport pin — except the trailing
        // mouse-up of the very click that created it (crossterm delivers Down
        // and Up, and both trigger a render).
        if !matches!(cmd, TuiCommand::EndMouseDrag) {
            self.scroll_pin = None;
        }
        match cmd {
            TuiCommand::Quit => self.should_quit = true,
            TuiCommand::Dismiss => {
                if self.file_finder.is_some() {
                    self.cancel_file_finder();
                } else if self.note_draft.is_some() {
                    self.note_draft = None;
                } else if self.show_help {
                    self.show_help = false;
                } else if self.anchor.is_some() || self.confirmed_sel.is_some() {
                    self.anchor = None;
                    self.confirmed_sel = None;
                    self.sel_pivot = None;
                }
            }
            TuiCommand::MoveUp(n) | TuiCommand::MouseScrollUp(n) => {
                for _ in 0..n {
                    self.move_up();
                }
            }
            TuiCommand::MoveDown(n) | TuiCommand::MouseScrollDown(n) => {
                for _ in 0..n {
                    self.move_down();
                }
            }
            TuiCommand::PageUp => self.page_up(),
            TuiCommand::PageDown => self.page_down(),
            TuiCommand::HalfPageUp => {
                let delta = (self.viewport_height / 2).max(1);
                self.scroll_to_visual(self.scroll.saturating_sub(delta));
            }
            TuiCommand::HalfPageDown => {
                let delta = (self.viewport_height / 2).max(1);
                self.scroll_to_visual(self.scroll + delta);
            }
            TuiCommand::MoveToTop => self.move_top(),
            TuiCommand::MoveToBottom => self.move_bottom(),
            TuiCommand::ScrollLeft(n) => self.col_offset = self.col_offset.saturating_sub(n),
            TuiCommand::ScrollRight(n) => self.col_offset += n,
            TuiCommand::JumpHunkPrev => self.prev_hunk(),
            TuiCommand::JumpHunkNext => self.next_hunk(),
            TuiCommand::JumpFilePrev => self.prev_file(),
            TuiCommand::JumpFileNext => self.next_file(),
            TuiCommand::JumpToLine(line) => self.goto_line(line),
            TuiCommand::ToggleSyntax => self.syntax_on = !self.syntax_on,
            TuiCommand::ToggleWordDiff => self.word_diff = !self.word_diff,
            TuiCommand::ToggleRawDiff => self.toggle_raw_diff(),
            TuiCommand::ToggleHelp => self.show_help = !self.show_help,
            TuiCommand::ExpandAllHidden => {
                self.expand_all_hidden();
            }
            TuiCommand::ResetCurrentHunkContext => {
                self.reset_current_hunk();
            }
            TuiCommand::ResetAllContext => {
                self.reset_all_context();
            }
            TuiCommand::ToggleCollapseFile(None) => {
                if self
                    .flat
                    .get(self.row_idx)
                    .is_some_and(|fr| fr.is_file_header)
                {
                    self.toggle_current_file_collapse();
                } else if let Some((fi, hi)) = self.is_cursor_on_hunk_divider() {
                    if hi == 0 {
                        let _ = self.expand_hunk_above(fi, 0);
                    } else {
                        let _ = self.expand_hunk_below(fi, hi - 1);
                        let _ = self.expand_hunk_above(fi, hi);
                    }
                }
                // Enter on a content row is a no-op: collapsing is reserved
                // for the file header row.
            }
            TuiCommand::ToggleCollapseFile(Some(fi)) => self.toggle_file_collapse(fi),
            TuiCommand::OpenFileFinder => self.open_file_finder(),
            TuiCommand::CloseFileFinder => self.cancel_file_finder(),
            TuiCommand::ConfirmFileFinder => self.confirm_file_finder(),
            TuiCommand::UpdateFileFinderQuery(q) => {
                if let Some(finder) = &mut self.file_finder {
                    finder.query = q;
                    finder.update_matches(&self.output.files);
                }
                self.track_finder_selection();
            }
            TuiCommand::StartSelection | TuiCommand::ConfirmSelection => {
                self.start_or_confirm_selection();
            }
            TuiCommand::ClearSelection => {
                self.anchor = None;
                self.confirmed_sel = None;
            }
            TuiCommand::OpenEditor => {
                if let Some(file) = self.current_file()
                    && let Some(row) = self.current_flat_row()
                    && let Some(c) = self.flat_row_coord(row)
                {
                    let range = self.compute_selection().and_then(|sel| {
                        if sel.side_is_new {
                            Some((sel.start, sel.end))
                        } else {
                            None
                        }
                    });
                    self.pending_editor = Some(crate::editor::EditorRequest {
                        file: file.path.clone(),
                        line: c,
                        range,
                    });
                }
            }
            TuiCommand::Stage => {
                let repo_root = self.repo_root.clone();
                let git_args = self.git_args.clone();
                match crate::actions::stage_action(self, &repo_root, &git_args, false) {
                    Ok(()) => {
                        self.status_message = Some(("staged hunk".into(), true));
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_args);
                    }
                    Err(e) => self.status_message = Some((e, false)),
                }
            }
            TuiCommand::Unstage => {
                let repo_root = self.repo_root.clone();
                let git_args = self.git_args.clone();
                match crate::actions::unstage_action(self, &repo_root, &git_args, false) {
                    Ok(()) => {
                        self.status_message = Some(("unstaged hunk".into(), true));
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_args);
                    }
                    Err(e) => self.status_message = Some((e, false)),
                }
            }
            TuiCommand::Discard => {
                let repo_root = self.repo_root.clone();
                let git_args = self.git_args.clone();
                match crate::actions::discard_action(self, &repo_root, &git_args, false) {
                    Ok(stash_ref) => {
                        let msg = if let Some(sr) = stash_ref {
                            format!("discarded hunk (stash: {sr})")
                        } else {
                            "discarded hunk".into()
                        };
                        self.status_message = Some((msg, true));
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_args);
                    }
                    Err(e) => self.status_message = Some((e, false)),
                }
            }
            TuiCommand::StartNoteDraft => self.note_draft = Some(String::new()),
            TuiCommand::CommitNoteDraft => self.commit_note_draft(),
            TuiCommand::CancelNoteDraft => self.note_draft = None,
            TuiCommand::DeleteNote => self.delete_notes(),
            TuiCommand::MouseClick { row, col, shift } => {
                let header_offset = self.last_viewport_start;
                let vh = self.viewport_height;
                let cw = self.last_content_width;
                let is_on_scrollbar = col >= cw;
                let in_viewport = row >= header_offset && row < header_offset + vh;
                // A click in the scrollbar column seeks — unless it lands on a
                // hunk divider block whose expansion arrows take priority (on
                // narrow screens the wrapped arrows sit flush against the
                // border column).
                let mut on_divider_block = false;
                if in_viewport && is_on_scrollbar {
                    let target_vis = self.scroll + (row - header_offset);
                    let (starts, counts, _seg, _total) =
                        self.visual_geometry(self.last_content_width);
                    on_divider_block =
                        starts
                            .iter()
                            .zip(counts.iter())
                            .enumerate()
                            .any(|(i, (&s, &c))| {
                                target_vis >= s
                                    && target_vis < s + c
                                    && self.hunk_starts.iter().any(|hs| hs.flat == i)
                            });
                }
                let scrollbar_seek = is_on_scrollbar && !on_divider_block;
                if scrollbar_seek && in_viewport {
                    self.is_dragging_scrollbar = true;
                    let row_in_vh = row - header_offset;
                    let (_starts, _counts, _seg, total_vis) =
                        self.visual_geometry(self.last_content_width);
                    let max_scroll = total_vis.saturating_sub(vh);
                    if max_scroll > 0 && vh > 1 {
                        let target_vis = ((row_in_vh as f64 / (vh - 1) as f64) * max_scroll as f64)
                            .round() as usize;
                        self.scroll_to_visual(target_vis);
                    }
                } else if in_viewport {
                    self.is_dragging_scrollbar = false;
                    let target_vis = self.scroll + (row - header_offset);
                    let (starts, counts, seg_counts, _total_vis) =
                        self.visual_geometry(self.last_content_width);
                    let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
                        HashMap::new()
                    } else {
                        self.hunk_starts
                            .iter()
                            .map(|h| (h.flat, (h.file, h.hunk)))
                            .collect()
                    };

                    for (i, &s) in starts.iter().enumerate() {
                        if target_vis >= s && target_vis < s + counts[i] {
                            if let Some(&(div_fi, div_hi)) = div_at.get(&i)
                                && let Some(f) = self.output.files.get(div_fi)
                                && let Some(hunk) = f.hunks.get(div_hi)
                            {
                                let hidden_gap = self.hunk_leading_gap(div_fi, div_hi);
                                if hidden_gap > 0 {
                                    // The divider may wrap across several visual
                                    // segments on narrow screens; translate the
                                    // click into unwrapped text columns.
                                    let seg_off = target_vis - s;
                                    let eff_col = seg_off * cw + col;
                                    let txt = hunk_divider_text(div_hi, hunk, hidden_gap);

                                    let hit = |pat: &str| {
                                        txt.find(pat).is_some_and(|idx| {
                                            let c = txt[..idx].chars().count();
                                            let w = pat.chars().count();
                                            eff_col >= c && eff_col < c + w
                                        })
                                    };

                                    let mut handled = true;
                                    if div_hi == 0 && hit("▼▼") {
                                        self.expand_hunk_above(div_fi, 0);
                                    } else if div_hi > 0 && hit("▲▼") {
                                        self.expand_hunk_both(div_fi, div_hi - 1);
                                    } else if div_hi > 0 && hit("▲▲") {
                                        self.expand_hunk_below(div_fi, div_hi - 1);
                                    } else if div_hi > 0 && hit("▼▼") {
                                        self.expand_hunk_above(div_fi, div_hi);
                                    } else {
                                        handled = false;
                                    }
                                    if handled {
                                        break;
                                    }
                                }
                            }

                            // Clicking the collapse/expand triangle on a file
                            // header toggles the file — the click is swallowed
                            // and does not select the row.
                            if let Some(item) = self.flat.get(i)
                                && item.is_file_header
                            {
                                let seg_off = target_vis - s;
                                let eff_col = seg_off * cw + col;
                                let c = crate::view::diff_grid::FILE_HEADER_INDICATOR_COL;
                                let hit_indicator =
                                    eff_col >= c.saturating_sub(1) && eff_col <= c + 2;
                                if hit_indicator {
                                    let fi = item.file_idx;
                                    self.execute(TuiCommand::ToggleCollapseFile(Some(fi)));
                                    break;
                                }
                            }

                            let prev_row = self.row_idx;
                            self.row_idx = i;
                            self.vis_line = (target_vis - s).min(seg_counts[i].saturating_sub(1));

                            if shift {
                                let pivot = self.sel_pivot.unwrap_or(prev_row);
                                self.anchor = Some(pivot);
                                self.confirmed_sel = self.compute_selection();
                                self.anchor = None;
                            } else {
                                self.anchor = None;
                                self.confirmed_sel = None;
                                self.sel_pivot = Some(i);
                            }
                            break;
                        }
                    }
                }
            }
            TuiCommand::MouseDrag { row, col: _, shift } => {
                let header_offset = self.last_viewport_start;
                let vh = self.viewport_height;
                if self.is_dragging_scrollbar {
                    let clamped_row =
                        row.clamp(header_offset, header_offset + vh.saturating_sub(1));
                    let row_in_vh = clamped_row - header_offset;
                    let (_starts, _counts, _seg, total_vis) =
                        self.visual_geometry(self.last_content_width);
                    let max_scroll = total_vis.saturating_sub(vh);
                    if max_scroll > 0 && vh > 1 {
                        let target_vis = ((row_in_vh as f64 / (vh - 1) as f64) * max_scroll as f64)
                            .round() as usize;
                        self.scroll_to_visual(target_vis);
                    }
                } else if row >= header_offset && row < header_offset + vh {
                    let prev_row = self.row_idx;
                    if self.anchor.is_none() {
                        let pivot = if shift {
                            self.sel_pivot.unwrap_or(prev_row)
                        } else {
                            prev_row
                        };
                        self.anchor = Some(pivot);
                        self.sel_pivot = Some(pivot);
                        self.confirmed_sel = None;
                    }
                    let target_vis = self.scroll + (row - header_offset);
                    let (starts, counts, _seg_counts, _total_vis) =
                        self.visual_geometry(self.last_content_width);
                    for (i, &s) in starts.iter().enumerate() {
                        if target_vis >= s && target_vis < s + counts[i] {
                            self.row_idx = i;
                            self.vis_line = 0;
                            break;
                        }
                    }
                }
            }
            TuiCommand::EndMouseDrag => {
                self.is_dragging_scrollbar = false;
                if let Some(anchor) = self.anchor {
                    if anchor != self.row_idx {
                        self.confirmed_sel = self.compute_selection();
                    }
                    self.anchor = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use diffuse_core::PipelineOptions;

    fn sample_output() -> diffuse_core::wire::PipelineOutput {
        let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
        let new_content = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_content = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        )
    }

    #[test]
    fn test_clicking_file_header_triangle_toggles_collapse() {
        let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
        let new_content = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n"
                .to_string(),
        )]
        .into_iter()
        .collect();
        let old_content = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        let mut state = AppState::new(
            diffuse_core::pipeline::run_pipeline_full(
                diff,
                Some(new_content),
                Some(old_content),
                PipelineOptions::default(),
            ),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-header-triangle.json"),
        );
        state.render_frame(120, 30);

        assert!(!state.collapsed_files.contains(&0), "starts expanded");
        let initial_len = state.flat.len();

        // Click exactly on the ▼ triangle (char col 12) on the header row
        let header_vis = state
            .visual_geometry(state.last_content_width)
            .0
            .first()
            .copied()
            .unwrap_or(0);
        let click_row = state.last_viewport_start + header_vis.saturating_sub(state.scroll);

        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
            shift: false,
        });
        assert!(
            state.collapsed_files.contains(&0),
            "clicking the triangle must collapse the file"
        );
        assert_eq!(state.flat.len(), 1, "collapsed to header only");

        // Expand again via the (now ▸) triangle
        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
            shift: false,
        });
        assert!(
            !state.collapsed_files.contains(&0),
            "clicking again must expand the file"
        );
        assert_eq!(state.flat.len(), initial_len);

        // A click elsewhere on the header must NOT toggle — it selects.
        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL + 10,
            shift: false,
        });
        assert!(
            !state.collapsed_files.contains(&0),
            "non-triangle header clicks must not collapse"
        );
    }

    #[test]
    fn test_note_commit_persists_and_deletes() {
        let notes_path = std::env::temp_dir().join(format!(
            "diffuse-test-note-lifecycle-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&notes_path);
        let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path.clone());

        let file = state.current_file().unwrap().path.clone();
        state.note_draft = Some("check this **carefully**".into());
        state.commit_note_draft();
        assert!(state.note_draft.is_none());

        let notes: Vec<_> =
            diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file).collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "check this **carefully**");
        assert!(
            notes[0].start > 0,
            "note must anchor to a real row coordinate"
        );

        let on_disk = diffuse_core::notes::load_notes(&notes_path);
        assert_eq!(
            diffuse_core::notes::notes_for_file(&on_disk, "test_repo", &file).count(),
            1,
            "commit must persist through the notes DB file"
        );

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("💬"),
            "a committed note must render a callout: {:.200}",
            frame
        );

        state.delete_notes();
        assert!(
            diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file)
                .next()
                .is_none(),
            "delete must remove the note from the DB"
        );
        let on_disk2 = diffuse_core::notes::load_notes(&notes_path);
        assert!(
            diffuse_core::notes::notes_for_file(&on_disk2, "test_repo", &file)
                .next()
                .is_none(),
            "delete must persist the removal"
        );

        let _ = std::fs::remove_file(notes_path);
    }

    #[test]
    fn test_help_modal_toggles_and_renders() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );

        let plain = state.render_frame(100, 30);
        assert!(
            !plain.contains("KEY BINDINGS"),
            "help modal must be hidden by default"
        );

        state.show_help = true;
        let with_help = state.render_frame(100, 30);
        assert!(
            with_help.contains("KEY BINDINGS"),
            "help modal must render when enabled"
        );
        assert!(with_help.contains("toggle syntax"));
    }

    #[test]
    fn test_selection_anchor_span_snaps_to_coord_rows() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        // flat[0] is the file header (no coordinate); AppState starts on the
        // first coordinate row (1020). The engine pads the hunk with empty
        // above-context rows (1022-1024), then the real rows: ctx
        // " let x = 1;" (1025), the mod pair (1026), the add (1027). Walk to
        // the first real row, anchor, extend over the mod.
        for _ in 0..3 {
            state.move_down();
        }
        state.start_or_confirm_selection();
        state.move_down();
        state.move_down();

        let sel = state
            .compute_selection()
            .expect("anchor over coordinate rows must yield a selection");
        assert_eq!(sel.file_idx, 0);
        assert!(sel.start < sel.end, "range must span two rows");
        assert_eq!(sel.start, 1025);
        assert_eq!(sel.end, 1027);
        assert!(
            sel.side_is_new,
            "a range led by a new-side row is a new-side selection"
        );
    }

    #[test]
    fn test_selection_v_confirm_clears_anchor_and_band_renders() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        for _ in 0..3 {
            state.move_down();
        }
        state.start_or_confirm_selection();
        state.move_down();
        state.move_down();

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("\x1b[48;5;240m"),
            "an active anchor must band the rows between anchor and cursor"
        );

        state.start_or_confirm_selection();
        assert!(state.anchor.is_none(), "confirming clears the anchor");
        let sel = state
            .confirmed_sel
            .expect("confirm must record a selection");
        assert_eq!(sel.start, 1025);
        assert_eq!(sel.end, 1027);

        let frame2 = state.render_frame(100, 30);
        assert!(
            frame2.contains("\x1b[48;5;240m"),
            "a confirmed selection must keep banding its coordinate range"
        );
    }

    #[test]
    fn test_selection_requires_coord_rows() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        // Anchor on the file header (no coordinate) and confirm in place: the
        // range collapses onto the header which carries no line number, so
        // the reference computes no selection.
        state.move_up();
        state.start_or_confirm_selection();
        state.start_or_confirm_selection();
        assert!(state.anchor.is_none());
        assert!(state.confirmed_sel.is_none(), "header has no coordinate");

        // Anchoring on the header then walking onto a coordinate row snaps
        // the low end down to the first coordinate row of the file.
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.move_up();
        state.start_or_confirm_selection();
        state.move_down();
        let sel = state
            .compute_selection()
            .expect("anchor over the header must snap to a coordinate row");
        assert_eq!(sel.start, 1022, "snap to the first padded ctx row");
        assert_eq!(sel.end, 1022);
    }

    #[test]
    fn test_raw_diff_uncollapses_mod_and_renders() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        assert!(!state.raw_diff);
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        let del = state.flat.iter().find(|fr| {
            state
                .row_of(fr)
                .is_some_and(|r| r.kind == diffuse_core::parse::RowKind::Del)
        });
        assert!(
            del.is_some()
                && state
                    .row_of(del.unwrap())
                    .unwrap()
                    .text
                    .contains("old comment"),
            "the mod pair must un-collapse into a - line"
        );
        let add = state.flat.iter().find(|fr| {
            state.row_of(fr).is_some_and(|r| {
                r.kind == diffuse_core::parse::RowKind::Add && r.text.contains("new comment")
            })
        });
        assert!(add.is_some(), "the mod pair must un-collapse into a + line");

        let sep = state.flat.iter().find(|fr| fr.is_hunk_divider);
        assert!(sep.is_some());

        let frame = state.render_frame(120, 40);
        assert!(frame.contains("@@ -1025,6 +1025,8 @@"), "raw @@ header");
        assert!(
            frame.contains("\x1b[38;5;203m-\x1b[39m\x1b[23m\x1b[38;5;203m"),
            "raw deleted lines render red marker and red text"
        );
        assert!(
            frame.contains("\x1b[38;5;120m+\x1b[39m\x1b[23m\x1b[38;5;120m"),
            "raw added lines render green marker and green text"
        );
    }

    #[test]
    fn test_raw_diff_toggle_anchors_cursor() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        // flat[5] is the mod pair (new 1026); walk onto it.
        for _ in 0..4 {
            state.move_down();
        }
        let r = state.row_of(&state.flat[state.row_idx]).unwrap();
        assert_eq!(r.kind, diffuse_core::parse::RowKind::Mod);
        assert_eq!(r.new_line, Some(1026));

        state.toggle_raw_diff();
        let r = state.row_of(&state.flat[state.row_idx]).unwrap();
        assert_eq!(
            r.kind,
            diffuse_core::parse::RowKind::Add,
            "the cursor resolves to the + line of the pair"
        );
        assert_eq!(r.new_line, Some(1026), "same coordinate after the switch");

        state.toggle_raw_diff();
        let r = state.row_of(&state.flat[state.row_idx]).unwrap();
        assert_eq!(
            AppState::row_coord(r),
            Some(1026),
            "round-tripping lands back on the same line"
        );
        assert!(!state.raw_diff);
    }

    #[test]
    fn test_raw_diff_exact_input_reproduction() {
        let diff = "diff --git a/cache.rs b/cache.rs\n--- a/cache.rs\n+++ b/cache.rs\n@@ -1,4 +1,4 @@\n-fn a() {\n-    let x = 1;\n-    let y = 2;\n+fn a() {\n+    let x = 10;\n+    let y = 20;\n }\n";
        let new_content = [(
            "cache.rs".to_string(),
            "fn a() {\n    let x = 10;\n    let y = 20;\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        let old_content = [(
            "cache.rs".to_string(),
            "fn a() {\n    let x = 1;\n    let y = 2;\n}\n".to_string(),
        )]
        .into_iter()
        .collect();

        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-raw-group.json"),
        );
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        let kinds: Vec<diffuse_core::parse::RowKind> = state
            .flat
            .iter()
            .filter(|r| !r.is_file_header && !r.is_hunk_divider)
            .map(|r| state.row_of(r).unwrap().kind)
            .collect();

        // Exact match with diff input: 3 deletions, 3 additions, 1 context line
        assert_eq!(
            kinds,
            vec![
                diffuse_core::parse::RowKind::Del,
                diffuse_core::parse::RowKind::Del,
                diffuse_core::parse::RowKind::Del,
                diffuse_core::parse::RowKind::Add,
                diffuse_core::parse::RowKind::Add,
                diffuse_core::parse::RowKind::Add,
                diffuse_core::parse::RowKind::Ctx,
            ],
            "raw diff must reproduce the exact hunk row sequence from the input diff"
        );
    }

    #[test]
    fn test_toggle_raw_diff_preserves_cursor_screen_pivot() {
        let mut diff = String::from(
            "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,40 +1,40 @@\n",
        );
        let mut lines = Vec::new();
        for i in 1..=50 {
            if i == 25 {
                diff.push_str("- line 25\n+ line 25 mod\n");
                lines.push(" line 25 mod".to_string());
            } else {
                diff.push_str(&format!(" line {i}\n"));
                lines.push(format!(" line {i}"));
            }
        }
        let new_content = [("test.rs".to_string(), lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            None,
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-pivot.json"),
        );
        state.viewport_height = 10;
        state.last_content_width = 80;
        state.render_frame(80, 10);

        // Position cursor at modified line (row 7) and scroll down by 2
        state.scroll = 2;
        state.row_idx = 7;
        let visual_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

        state.toggle_raw_diff();
        assert!(state.raw_diff);

        let raw_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);
        assert_eq!(
            visual_screen_y, raw_screen_y,
            "cursor screen-y position relative to viewport top must be preserved across raw diff toggle"
        );

        // Toggle back and verify round-trip stability
        state.toggle_raw_diff();
        assert!(!state.raw_diff);
        let roundtrip_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);
        assert_eq!(
            visual_screen_y, roundtrip_screen_y,
            "cursor screen-y position must match after roundtrip toggle"
        );
    }

    #[test]
    fn test_raw_diff_no_right_padding_or_scrollbar() {
        let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
 -let short = 1;
 +let short = 2;
 "#;
        let new_content = [("test.rs".to_string(), "let short = 2;\n".to_string())]
            .into_iter()
            .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            None,
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-no-pad.json"),
        );
        state.viewport_height = 20;
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        let frame = state.render_frame(80, 20);
        let lines: Vec<&str> = frame.lines().collect();

        // Check the diff rows (excluding banner/ribbon)
        for line in &lines[1..3] {
            // Must not contain scrollbar characters
            assert!(
                !line.contains('█') && !line.contains('│'),
                "raw diff must not draw scrollbar"
            );
            // Must not be right-padded with 50+ trailing spaces before \x1b[K
            assert!(
                !line.contains("                                                  "),
                "raw diff must not right-pad with spaces"
            );
        }
    }

    #[test]
    fn test_raw_diff_wrapped_line_no_false_trailing_highlight() {
        let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,1 +1,1 @@
+6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.
"#;
        let new_content = [(
            "test.rs".to_string(),
            "6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.\n".to_string(),
        )]
        .into_iter()
        .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            None,
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-wrapped.json"),
        );
        state.viewport_height = 20;
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        // Render at width 40 so the line wraps into multiple segments
        let frame = state.render_frame(40, 20);
        let lines: Vec<&str> = frame.lines().collect();

        // The first segment of the wrapped line must not contain green background
        // highlight \x1b[48;5;28m
        for line in &lines[2..4] {
            assert!(
                !line.contains("\x1b[48;5;28m"),
                "wrapped line segments without trailing spaces must not highlight intermediate spaces: {line}"
            );
        }
    }

    #[test]
    fn test_toggle_raw_diff_from_deleted_line_to_merged_mod_line() {
        let diff = r#"diff --git a/cache.rs b/cache.rs
--- a/cache.rs
+++ b/cache.rs
@@ -80,5 +86,5 @@
  context line 80
  context line 81
-/// LRU cache for path resolution results with modtime validation.
+/// LRU cache for path resolution results with modtime validation and
  context line 83
  context line 84
"#;
        let new_content = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation and\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
        let old_content = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation.\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-del-to-mod.json"),
        );
        state.viewport_height = 20;

        // Switch to raw diff
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        // Find the deleted line in raw diff (old_line = Some(82))
        let del_idx = state
            .flat
            .iter()
            .position(|r| {
                state.row_of(r).is_some_and(|row| {
                    row.kind == diffuse_core::parse::RowKind::Del && row.old_line == Some(82)
                })
            })
            .expect("must find deleted line 82 in raw diff");
        state.row_idx = del_idx;

        // Hit R to toggle back to visual diff
        state.toggle_raw_diff();
        assert!(!state.raw_diff);

        let current_row = &state.flat[state.row_idx];
        assert!(
            !current_row.is_file_header,
            "must not jump to file header row"
        );
        let current_out = state.row_of(current_row).unwrap();
        assert_eq!(
            current_out.old_line,
            Some(82),
            "selected row in visual diff must have old_line 82"
        );
        assert_eq!(
            current_out.new_line,
            Some(88),
            "selected row in visual diff must be the merged mod line 88"
        );
    }

    #[test]
    fn test_cache_rs_lines_115_and_141_stick_to_screen() {
        let diff = r#"diff --git a/src/tracer/vfs/cache.rs b/src/tracer/vfs/cache.rs
index 6f6222d..4aa7a1d 100644
--- a/src/tracer/vfs/cache.rs
+++ b/src/tracer/vfs/cache.rs
@@ -1,7 +1,13 @@
-use std::{cell::RefCell, num::NonZeroUsize, os::unix::fs::MetadataExt, path::PathBuf};
+use std::{
+    cell::RefCell,
+    num::NonZeroUsize,
+    os::unix::fs::MetadataExt,
+    path::PathBuf,
+    sync::atomic::{AtomicU64, Ordering},
+};

 use lru::LruCache;

 use super::{Intent, Links, ResolutionResult};

 // ── Cache Key
@@ -76,55 +82,104 @@ impl ResolutionFingerprint {
 }

 // ── Resolution Cache ────────────────────────────────────────────────────────

 const DEFAULT_CAPACITY: usize = 2048;

-/// LRU cache for path resolution results with modtime validation.
+/// LRU cache for path resolution results with modtime validation and
+/// **epoch-based invalidation**.
+///
+/// Every entry is tagged with the epoch snapshot taken when its resolution
+/// *started* (`VFS::resolve` reads the epoch before lookup). A mode flip /
+/// mount mutation bumps the epoch atomically:
+///
+/// - `lookup` rejects any entry whose tag differs from the caller's snapshot
+///   — a result computed under a pre-flip mount state is never served.
+/// - `insert` drops any entry whose tag is stale *at insert time* — a
+///   resolution that started before a flip but finishes its (possibly
+///   copy-up) computation after the bump cannot re-populate the cache with a
+///   pre-flip result (the in-flight-insert race). The epoch read at resolve
+///   start is the coherence anchor: an insert under an old epoch means the
+///   computation spanned a flip, so the result is discarded.
+///
+/// The epoch is a single `AtomicU64` — when the VFS becomes shared
+/// (multi-threaded multi-guest, plan/TOKIO_MULTITHREADED_HYPERVISOR.md) the
+/// bump stays a single atomic store; the `set_mode` + bump pair must then sit
+/// in one critical section (the per-guest `Mutex` from phase M2).
 #[derive(Debug)]
 pub(super) struct ResolutionCache {
     cache: RefCell<LruCache<CacheKey, ResolutionFingerprint>>,
-    results: RefCell<LruCache<CacheKey, ResolutionResult>>,
+    results: RefCell<LruCache<CacheKey, (u64, ResolutionResult)>>,
+    epoch: AtomicU64,
 }

 impl Clone for ResolutionCache {
     fn clone(&self) -> Self {
         Self {
             cache: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
             results: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
+            epoch: AtomicU64::new(self.epoch.load(Ordering::Relaxed)),
         }
     }
 }

 impl ResolutionCache {
     pub fn new() -> Self {
         Self {
             cache: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
             results: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
+            epoch: AtomicU64::new(0),
         }
     }

-    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {
+    /// Current epoch. A resolution must snapshot this *before* its first
+    /// lookup and pass it to both `lookup` and `insert`.
+    pub fn epoch(&self) -> u64 {
+        self.epoch.load(Ordering::Relaxed)
+    }
+
+    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {
         let fingerprint = self.cache.borrow().peek(key)?.clone();
-        if fingerprint.is_valid() {
-            return self.results.borrow_mut().get(key).cloned();
+        if !fingerprint.is_valid() {
+            self.cache.borrow_mut().pop(key);
+            self.results.borrow_mut().pop(key);
+            return None;
         }
-        self.cache.borrow_mut().pop(key);
-        self.results.borrow_mut().pop(key);
-        None
+        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();
+        if entry_epoch != epoch {
+            // Computed under an older mount state: the epoch bump (mode flip,
+            // mount mutation) invalidated it.
+            self.cache.borrow_mut().pop(key);
+            self.results.borrow_mut().pop(key);
+            return None;
         }
         Some(result)
     }

     pub fn insert(
         &self,
         key: CacheKey,
         result: ResolutionResult,
         fingerprint: ResolutionFingerprint,
+        epoch: u64,
     ) {
+        // The epoch read at resolve start is the coherence anchor. If it is
+        // already stale, the computation spanned a mode flip / mount change —
+        // discard the result rather than cache a mixed-state entry.
+        if self.epoch.load(Ordering::Relaxed) != epoch {
+            return;
+        }
         self.cache.borrow_mut().put(key.clone(), fingerprint);
-        self.results.borrow_mut().put(key, result);
+        self.results.borrow_mut().put(key, (epoch, result));
     }

+    /// Invalidate every cached entry. Epoch-based: a single atomic bump makes
+    /// all entries computed under an older epoch stale at lookup — race-free
+    /// even against an in-flight resolution inserting after this call (the
+    /// insert carries the pre-bump epoch and is dropped or never served).
+    /// The physical clear additionally bounds memory.
     pub fn clear(&self) {
+        self.epoch.fetch_add(1, Ordering::Relaxed);
         self.cache.borrow_mut().clear();
         self.results.borrow_mut().clear();
     }
 }
"#;
        let out =
            diffuse_core::pipeline::run_pipeline_full(diff, None, None, PipelineOptions::default());
        let mut state = AppState::new(
            out,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-test-cache-lines.json"),
        );

        // Exact reproduction test matching user's terminal geometry (74 rows, 300 cols)
        // on mirage.work
        let vis_idx_115 = state
            .flat
            .iter()
            .position(|r| {
                r.file_idx == 3 && state.row_of(r).is_some_and(|row| row.new_line == Some(115))
            })
            .unwrap_or_else(|| {
                state
                    .flat
                    .iter()
                    .position(|r| state.row_of(r).is_some_and(|row| row.new_line == Some(115)))
                    .unwrap_or_else(|| panic!("must find line 115"))
            });
        state.row_idx = vis_idx_115;
        state.vis_line = 0;

        // Set scroll so line 90 is at the top of the viewport (matching visual
        // screenshot)
        let vis_idx_90 = state
            .flat
            .iter()
            .position(|r| {
                r.file_idx == state.flat[vis_idx_115].file_idx
                    && state.row_of(r).is_some_and(|row| row.new_line == Some(90))
            })
            .unwrap_or_else(|| panic!("must find line 90"));
        state.scroll = state.visual_geometry(300).0[vis_idx_90];

        let frame_vis = state.render_frame(300, 74);
        let vis_cursor_line = frame_vis
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("115"))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("line 115 not in visual frame"));

        state.toggle_raw_diff();
        let frame_raw = state.render_frame(300, 74);
        let raw_cursor_line = frame_raw
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("115"))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("line 115 not in raw frame"));

        assert_eq!(
            vis_cursor_line, raw_cursor_line,
            "line 115 cursor screen line must not jump on 74x300 terminal: vis={vis_cursor_line}, raw={raw_cursor_line}"
        );

        // Switch back to visual mode for the loop test
        if state.raw_diff {
            state.toggle_raw_diff();
        }

        // Test line 115 and line 141 specifically, across different viewport heights
        // and scroll positions
        for target_line in [115, 141] {
            for vh in [25, 30, 40, 50] {
                let vis_idx = state
                    .flat
                    .iter()
                    .position(|r| {
                        state.row_of(r).is_some_and(|row| {
                            row.new_line == Some(target_line) || row.old_line == Some(target_line)
                        })
                    })
                    .unwrap_or_else(|| panic!("must find line {target_line} in visual flat"));
                state.row_idx = vis_idx;
                state.vis_line = 0;
                state.scroll = 0;

                let _ = state.render_frame(120, vh);
                let vis_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

                state.toggle_raw_diff();
                let _ = state.render_frame(120, vh);
                let raw_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

                assert_eq!(
                    vis_screen_y, raw_screen_y,
                    "line {target_line} (vh={vh}) screen Y must match on toggle to raw: vis={vis_screen_y}, raw={raw_screen_y}"
                );

                state.toggle_raw_diff();
                let _ = state.render_frame(120, vh);
                let roundtrip_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

                assert_eq!(
                    vis_screen_y, roundtrip_screen_y,
                    "line {target_line} (vh={vh}) screen Y must match on roundtrip toggle: vis={vis_screen_y}, roundtrip={roundtrip_screen_y}"
                );
            }
        }

        // Test line 115 in raw diff
        if !state.raw_diff {
            state.toggle_raw_diff();
        }
        assert!(state.raw_diff);

        let raw_idx_115 = state
            .flat
            .iter()
            .position(|r| state.row_of(r).is_some_and(|row| row.new_line == Some(115)))
            .unwrap_or_else(|| panic!("must find line 115 in raw flat"));
        state.row_idx = raw_idx_115;
        state.scroll = 30;

        let frame_raw_115 = state.render_frame(120, 30);
        let raw_115_cursor_line = frame_raw_115
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("\x1b[48;5;237m"))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("raw 115 cursor line not found"));

        state.toggle_raw_diff();
        assert!(!state.raw_diff);

        let frame_vis_115 = state.render_frame(120, 30);
        let vis_115_cursor_line = frame_vis_115
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("\x1b[48;5;237m"))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("visual 115 cursor line not found"));

        assert_eq!(
            raw_115_cursor_line, vis_115_cursor_line,
            "cursor screen line on line 115 must not move when switching from raw to visual diff"
        );
    }

    #[test]
    fn test_separator_expand_by_enter_and_click_and_merge() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let file = std::fs::File::open("../../tests/golden/mirage-2025-08.wire.json.gz")
            .or_else(|_| std::fs::File::open("tests/golden/mirage-2025-08.wire.json.gz"))
            .expect("must open mirage-2025-08.wire.json.gz");
        let mut gz = GzDecoder::new(file);
        let mut json_str = String::new();
        gz.read_to_string(&mut json_str).unwrap();
        let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();

        let mut state = AppState::new(
            output,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-mirage-notes-sep.json"),
        )
        .with_expand_lines(6);

        // Find cache.rs file index
        let fi = state
            .output
            .files
            .iter()
            .position(|f| f.path.contains("cache.rs"))
            .expect("cache.rs must exist");

        // Initial hunk count for cache.rs
        let init_hunk_count = state.output.files[fi].hunks.len();
        assert_eq!(init_hunk_count, 3, "cache.rs starts with 3 hunks");

        // Jump to H2 to expand context around the divider via Enter
        let h2_start = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;

        state.jump_to_row(h2_start, false);
        assert!(
            state.is_cursor_on_hunk_divider().is_some(),
            "cursor must be on H2 divider"
        );

        let initial_h1_below = state.hunk_meta[fi][0].below;
        let initial_h2_above = state.hunk_meta[fi][1].above;

        // Press Enter on H2 divider to expand context around divider by 6 lines
        state.execute(TuiCommand::ToggleCollapseFile(None));

        assert!(
            state.hunk_meta[fi][0].below >= initial_h1_below + 6,
            "H1 below context must increase by 6"
        );
        assert!(
            state.hunk_meta[fi][1].above >= initial_h2_above + 6,
            "H2 above context must increase by 6"
        );

        // Jump to H2 divider after expansion
        if let Some(hs) = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
        {
            state.jump_to_row(hs.flat, false);
        }

        let frame = state.render_frame(140, 80);
        assert!(
            frame.contains("───── H2 ·"),
            "H2 divider must still exist before fully merged"
        );
        assert!(
            frame.contains("hidden ▲▼ ▲▲ ▼▼"),
            "H2 divider must reflect hidden gap with ▲▼ ▲▲ ▼▼ buttons"
        );

        // Keep expanding until H1 and H2 merge
        while state.output.files[fi].hunks.len() == init_hunk_count {
            if let Some(hs) = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
            {
                state.jump_to_row(hs.flat, false);
                state.execute(TuiCommand::ToggleCollapseFile(None));
            } else {
                break;
            }
        }

        assert_eq!(
            state.output.files[fi].hunks.len(),
            init_hunk_count - 1,
            "expanding entire gap must merge H1 and H2 into 1 hunk"
        );
    }

    #[test]
    fn test_hunk_divider_arrow_click_and_syntax_highlighted_expansion() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let file = std::fs::File::open("../../tests/golden/mirage-2025-08.wire.json.gz")
            .or_else(|_| std::fs::File::open("tests/golden/mirage-2025-08.wire.json.gz"))
            .expect("must open mirage-2025-08.wire.json.gz");
        let mut gz = GzDecoder::new(file);
        let mut json_str = String::new();
        gz.read_to_string(&mut json_str).unwrap();
        let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();

        let mut state = AppState::new(
            output,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-mirage-notes-arrows.json"),
        )
        .with_expand_lines(6);

        // Find cache.rs file index
        let fi = state
            .output
            .files
            .iter()
            .position(|f| f.path.contains("cache.rs"))
            .expect("cache.rs must exist");

        let initial_h2_above = state.hunk_meta[fi][1].above;

        // Expand H2 above directly
        assert!(state.expand_hunk_above(fi, 1));
        assert_eq!(
            state.hunk_meta[fi][1].above,
            initial_h2_above + 6,
            "H2 above context must increase by 6"
        );

        // Verify prepended context rows in H2 have syntax runs
        let first_row = &state.output.files[fi].hunks[1].rows[0];
        assert_eq!(first_row.kind, diffuse_core::parse::RowKind::Ctx);
        assert!(
            !first_row.runs.is_empty(),
            "expanded context row must have syntax token runs"
        );
        assert!(
            first_row.runs.iter().any(|r| r.color.is_some()),
            "expanded context row must have colored syntax tokens"
        );

        let initial_h1_below = state.hunk_meta[fi][0].below;
        // Expand H1 below directly
        assert!(state.expand_hunk_below(fi, 0));
        assert_eq!(
            state.hunk_meta[fi][0].below,
            initial_h1_below + 6,
            "H1 below context must increase by 6"
        );

        // Verify appended context rows in H1 have syntax runs
        let last_h1_row = state.output.files[fi].hunks[0].rows.last().unwrap();
        assert_eq!(last_h1_row.kind, diffuse_core::parse::RowKind::Ctx);
        assert!(
            !last_h1_row.runs.is_empty(),
            "expanded context row must have syntax token runs"
        );

        // Verify that clicking an arrow on a hunk divider via MouseClick does NOT move
        // cursor
        let target_line = 100;
        let line_flat_idx = state
            .flat
            .iter()
            .position(|fr| {
                fr.file_idx == fi
                    && state
                        .row_of(fr)
                        .is_some_and(|r| r.new_line == Some(target_line))
            })
            .expect("must find target line in flat");
        state.jump_to_row(line_flat_idx, false);
        assert_eq!(state.row_idx, line_flat_idx);

        // Render frame with sufficient height so both H2 divider and target line are in
        // viewport
        state.render_frame(140, 80);
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let h2_hunk_start = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap();
        let h2_div_vis = starts[h2_hunk_start.flat];
        let click_row = state.last_viewport_start + h2_div_vis.saturating_sub(state.scroll);

        // Compute exact visual column of '▲' on H2 divider
        let h = &state.output.files[fi].hunks[1];
        let hidden_gap = state.hunk_leading_gap(fi, 1);
        let txt = hunk_divider_text(1, h, hidden_gap);
        let idx = txt.find("▲▲").unwrap();
        let col = txt[..idx].chars().count();

        // Click on the '▲' column
        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col,
            shift: false,
        });

        // Verify cursor is on the expanded hunk divider and divider line y position is
        // invariant
        let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
        let h2_div_vis_after = starts_after[state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat];
        let click_row_after =
            state.last_viewport_start + h2_div_vis_after.saturating_sub(state.scroll);
        assert_eq!(
            click_row, click_row_after,
            "divider line screen y position must remain exactly the same when expanding context"
        );
    }

    #[test]
    fn test_expand_all_and_reset_context_re_inserts_hunk_dividers() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let file = std::fs::File::open("../../tests/golden/mirage-2025-08.wire.json.gz")
            .or_else(|_| std::fs::File::open("tests/golden/mirage-2025-08.wire.json.gz"))
            .expect("must open mirage-2025-08.wire.json.gz");
        let mut gz = GzDecoder::new(file);
        let mut json_str = String::new();
        gz.read_to_string(&mut json_str).unwrap();
        let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();

        let mut state = AppState::new(
            output,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-mirage-notes-reset.json"),
        )
        .with_expand_lines(6);

        let fi = state
            .output
            .files
            .iter()
            .position(|f| f.path.contains("cache.rs"))
            .expect("cache.rs must exist");

        let init_hunks = state.output.files[fi].hunks.len();
        assert_eq!(init_hunks, 3, "cache.rs starts with 3 hunks");

        // 1. Expand all hidden context lines via 'a' shortcut
        state.execute(TuiCommand::ExpandAllHidden);
        assert_eq!(
            state.output.files[fi].hunks.len(),
            1,
            "expanding all context merges cache.rs into 1 contiguous hunk"
        );

        // 2. Reset context back to -U via 'c' shortcut (ResetCurrentHunkContext)
        let cache_file_row = state
            .flat
            .iter()
            .position(|fr| fr.file_idx == fi && !fr.is_file_header)
            .expect("row in cache.rs");
        state.jump_to_row(cache_file_row, false);
        state.execute(TuiCommand::ResetCurrentHunkContext);

        assert_eq!(
            state.output.files[fi].hunks.len(),
            3,
            "resetting context splits hunk back wherever interior ctx > 2 * -U, restoring 3 hunks"
        );

        // Verify dividers are re-rendered
        let frame = state.render_frame(140, 200);
        assert!(frame.contains("───── H1 ·"));
        assert!(frame.contains("───── H2 ·"));
        assert!(frame.contains("───── H3 ·"));
    }

    #[test]
    fn test_h1_divider_down_arrow_click_expands_top_and_stays_in_view() {
        let diff = r#"diff --git a/TODO.md b/TODO.md
index 1111111..2222222 100644
--- a/TODO.md
+++ b/TODO.md
@@ -10,4 +10,6 @@
 // line 10
+// line 11 added
+// line 12 added
 // line 13
"#;
        let mut old_lines = Vec::new();
        for i in 1..=25 {
            old_lines.push(format!("// line {i}"));
        }
        let mut new_lines = old_lines.clone();
        new_lines.insert(10, "// line 11 added".to_string());
        new_lines.insert(11, "// line 12 added".to_string());

        let contents = [("TODO.md".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("TODO.md".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content(contents)
            .with_old_content(old_contents)
            .run();
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-h1.json"),
        );

        let fi = 0;
        let gap = state.hunk_leading_gap(fi, 0);
        assert_eq!(gap, 6, "6 lines hidden before H1");

        // Render initial frame with small viewport (14 rows)
        let frame = state.render_frame(80, 14);
        assert!(frame.contains("6 hidden ▼▼"));
        assert!(!frame.contains("▲"));

        // Click down arrow on H1 divider
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let h1_start = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 0)
            .unwrap();
        let h1_div_vis = starts[h1_start.flat];
        let click_row = state.last_viewport_start + h1_div_vis.saturating_sub(state.scroll);

        let h = &state.output.files[fi].hunks[0];
        let txt = hunk_divider_text(0, h, 6);
        let idx = txt.find("▼▼").unwrap();
        let col = txt[..idx].chars().count();

        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col,
            shift: false,
        });

        // Frame after clicking down arrow: H1 should now start at line 1 and file
        // header must remain visible!
        let frame_after = state.render_frame(80, 14);
        assert!(
            frame_after.contains("TODO.md"),
            "file header must remain visible on screen"
        );
        assert!(
            frame_after.contains("───── H1 ·"),
            "H1 divider must remain visible on screen"
        );
        assert!(
            frame_after.contains("// line 1"),
            "expanded line 1 must be visible on screen"
        );
    }

    #[test]
    fn test_hunk_divider_both_arrow_click_merges_and_pivots_below_row() {
        let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -23,9 +24,9 @@
 // line 23
 // line 24
 // line 25
 // line 26
-// line 27 old
+// line 27 new
 // line 28
 // line 29
 // line 30
 // line 31
"#;
        let mut old_lines = Vec::new();
        for i in 1..=40 {
            if i == 27 {
                old_lines.push("// line 27 old".to_string());
            } else {
                old_lines.push(format!("// line {i}"));
            }
        }
        let mut new_lines = Vec::new();
        for i in 1..=40 {
            if i == 5 {
                new_lines.push("// inserted one".to_string());
            }
            if i == 28 {
                new_lines.push("// line 27 new".to_string());
            } else {
                new_lines.push(format!("// line {i}"));
            }
        }

        let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content(contents)
            .with_old_content(old_contents)
            .run();
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-both.json"),
        )
        .with_expand_lines(12);

        let fi = 0;
        assert_eq!(state.output.files[fi].hunks.len(), 2);
        let gap = state.hunk_leading_gap(fi, 1);
        assert_eq!(
            gap, 14,
            "14 lines hidden between H1 and H2; step=12 leaves 2 for the second call"
        );
        assert!(
            gap <= 2 * state.expand_lines,
            "gap must be fully consumed by the two sequential expansions"
        );

        // Render initial frame so layout state is populated
        state.render_frame(80, 20);
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);

        // Locate H2 divider and the boundary row: with n hidden lines split
        // (n+1)/2 up / n-(n+1)/2 down, the first downward-side row is the
        // original H2 first row minus the downward take.
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .map(|hs| hs.flat)
            .expect("H2 divider must exist in flat");
        let h2_first_new_line = state.output.files[fi].hunks[1]
            .rows
            .first()
            .and_then(|r| r.new_line)
            .expect("H2 must have rows");
        let top_take = gap.div_ceil(2).min(state.expand_lines);
        let bottom_take = gap - top_take;
        let below_row_new_line = h2_first_new_line - bottom_take as u32;

        let div_screen_y = starts[div_flat].saturating_sub(state.scroll);

        // Click the ↕↕ (expand both) arrow on the H2 divider: the gap splits
        // (n+1)/2 up / n-(n+1)/2 down, and on merge everything pivots around
        // the line where the divider was — scroll stays put.
        let scroll_before = state.scroll;
        // Reference row ABOVE the divider (last row of H1) — must not move.
        let ref_new_line = state.output.files[fi].hunks[0]
            .rows
            .iter()
            .rev()
            .find_map(|r| r.new_line)
            .expect("H1 must have rows");
        let (starts_pre, _c_pre, _s_pre, _) = state.visual_geometry(state.last_content_width);
        let ref_vis_before = state
            .flat_pos_of_coord(fi, Some(ref_new_line), None)
            .and_then(|pos| starts_pre.get(pos))
            .copied()
            .expect("H1 last row in flat");

        let h = &state.output.files[fi].hunks[1];
        let txt = hunk_divider_text(1, h, gap);
        let both_idx = txt.find("▲▼").expect("divider text must contain ▲▼");
        let col = txt[..both_idx].chars().count();

        state.execute(TuiCommand::MouseClick {
            row: state.last_viewport_start + div_screen_y,
            col,
            shift: false,
        });

        // The whole gap was consumed: H2 merged into H1
        assert_eq!(
            state.output.files[fi].hunks.len(),
            1,
            "fully consumed gap must merge H2 into H1"
        );

        // The boundary between the upward and downward halves sits exactly
        // where the divider was on screen; the top half pushed the content
        // above up by its visual length.
        let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
        let boundary_vis_after = state
            .flat_pos_of_coord(fi, Some(below_row_new_line), None)
            .and_then(|pos| starts_after.get(pos))
            .copied()
            .expect("boundary row must exist after merge");
        assert_eq!(
            boundary_vis_after.saturating_sub(state.scroll),
            div_screen_y,
            "the up/down boundary must pivot at the divider's old screen y"
        );
        let shift = state.scroll.saturating_sub(scroll_before);
        // Content above the divider moved up by exactly the scroll shift.
        let ref_vis_after = state
            .flat_pos_of_coord(fi, Some(ref_new_line), None)
            .and_then(|pos| starts_after.get(pos))
            .copied()
            .expect("reference row must exist after merge");
        assert_eq!(
            ref_vis_after, ref_vis_before,
            "content above keeps its absolute layout"
        );
        assert_eq!(
            ref_vis_after.saturating_sub(state.scroll),
            ref_vis_before
                .saturating_sub(scroll_before)
                .saturating_sub(shift),
            "content above the divider slides up by exactly the top half"
        );
    }

    #[test]
    fn test_wrapped_divider_arrow_click_at_scrollbar_column_expands() {
        // Regression: on narrow screens the divider wraps so the ▲▲ arrows sit
        // flush against the border/scrollbar column; tapping them there must
        // expand the gap instead of triggering a scrollbar seek that jumps to
        // another file.
        let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
        let mut old_lines = Vec::new();
        for i in 1..=40 {
            if i == 21 {
                old_lines.push("// line 21 old".to_string());
            } else {
                old_lines.push(format!("// line {i}"));
            }
        }
        let mut new_lines = Vec::new();
        for i in 1..=40 {
            if i == 5 {
                new_lines.push("// inserted one".to_string());
            }
            if i == 22 {
                new_lines.push("// line 21 new".to_string());
            } else {
                new_lines.push(format!("// line {i}"));
            }
        }
        let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content(contents)
            .with_old_content(old_contents)
            .run();
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-wrapped-arrows.json"),
        )
        .with_expand_lines(6);

        let fi = 0;
        // Narrow terminal: divider text (~60 chars) wraps across two segments
        state.render_frame(48, 30);
        assert!(
            state.last_content_width < 60,
            "content width must be narrower than the divider text for this scenario"
        );

        let (starts, _counts, _seg, _) = state.visual_geometry(state.last_content_width);
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let gap_before = state.hunk_leading_gap(fi, 1);
        let hk = &state.output.files[fi].hunks[1];
        let txt = hunk_divider_text(1, hk, gap_before);

        // Find the visual segment holding the '▲▲' button and its column
        // within that segment.
        let cw = state.last_content_width;
        let up_char_col = txt[..txt.find("▲▲").unwrap()].chars().count();
        let seg_off = up_char_col / cw;
        let col_in_seg = up_char_col % cw;

        let div_screen = starts[div_flat].saturating_sub(state.scroll) + seg_off;

        state.execute(TuiCommand::MouseClick {
            row: state.last_viewport_start + div_screen,
            col: col_in_seg,
            shift: false,
        });

        assert_eq!(
            state.hunk_leading_gap(fi, 1),
            gap_before.saturating_sub(6),
            "tap on wrapped ▲▲ must expand the gap"
        );
        // Clicking a divider button must never trigger a scrollbar seek: the
        // same file's divider must remain in the viewport.
        let (starts2, _c2, _s2, _t2) = state.visual_geometry(state.last_content_width);
        let div_screen_after = starts2[div_flat].saturating_sub(state.scroll);
        assert!(
            div_screen_after < state.viewport_height,
            "clicking a divider button must never trigger a scrollbar seek"
        );
    }

    #[test]
    fn test_hunk_divider_up_arrow_pinned_even_with_cursor_above() {
        // Regression: cursor sits on a row ABOVE the divider (typical after
        // scrolling/navigating); the render-time cursor clamp must not drag
        // scroll back and slide the divider down after an up-arrow expansion.
        let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
        let mut old_lines = Vec::new();
        for i in 1..=40 {
            if i == 21 {
                old_lines.push("// line 21 old".to_string());
            } else {
                old_lines.push(format!("// line {i}"));
            }
        }
        let mut new_lines = Vec::new();
        for i in 1..=40 {
            if i == 5 {
                new_lines.push("// inserted one".to_string());
            }
            if i == 22 {
                new_lines.push("// line 21 new".to_string());
            } else {
                new_lines.push(format!("// line {i}"));
            }
        }
        let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content(contents)
            .with_old_content(old_contents)
            .run();
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-up-pinned-cursor.json"),
        )
        .with_expand_lines(6);

        let fi = 0;

        // Park the cursor on an EARLY row of H1 (far above the H2 divider).
        // Small viewport so max_scroll > 0 and the render-time cursor clamp
        // has room to fight the divider pivot.
        let early_row = state
            .flat
            .iter()
            .position(|fr| {
                fr.file_idx == fi
                    && !fr.is_file_header
                    && !fr.is_hunk_divider
                    && state.row_of(fr).is_some_and(|r| r.new_line == Some(1))
            })
            .expect("first content row must exist");
        state.jump_to_row(early_row, false);

        state.render_frame(300, 20);
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_screen_before = starts[div_flat].saturating_sub(state.scroll);
        assert!(
            div_screen_before < state.viewport_height,
            "divider must be on screen for this scenario"
        );
        let cursor_before = state.row_idx;

        let gap_before = state.hunk_leading_gap(fi, 1);
        let hk = &state.output.files[fi].hunks[1];
        let txt = hunk_divider_text(1, hk, gap_before);
        let idx = txt.find("▲▲").unwrap();
        let col = txt[..idx].chars().count();
        state.execute(TuiCommand::MouseClick {
            row: state.last_viewport_start + div_screen_before,
            col,
            shift: false,
        });

        // Render like the real event loop does — this is where the clamp fired
        state.render_frame(300, 20);

        assert_eq!(
            state.hunk_leading_gap(fi, 1),
            gap_before.saturating_sub(6),
            "up arrow must have expanded 6 lines into the gap"
        );

        // Re-locate the divider: flat indices shifted with the inserted rows
        let div_flat_after = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_screen_after = starts_after[div_flat_after].saturating_sub(state.scroll);
        assert_eq!(
            div_screen_after, div_screen_before,
            "divider must stay pinned even when the cursor is above it"
        );
        assert_eq!(
            state.row_idx, cursor_before,
            "cursor must not move when clicking an expansion arrow"
        );
    }

    #[test]
    fn test_two_up_arrow_clicks_merge_with_wrapped_rows_and_lock_below_row() {
        // Static conversion of the exploratory worktree probe: two sequential
        // \u{25b2}\u{25b2} presses on a narrow screen with LONG context rows (they wrap
        // into several visual segments). The second press consumes the remaining
        // gap and merges; the row directly below the removed divider must stay
        // locked at its own pre-click screen position.
        let diff = r#"diff --git a/wide.rs b/wide.rs
index 1111111..2222222 100644
--- a/wide.rs
+++ b/wide.rs
@@ -1,8 +1,9 @@
 // line 1 -- padding padding padding padding padding padding padding padding
 // line 2 -- padding padding padding padding padding padding padding padding
 // line 3 -- padding padding padding padding padding padding padding padding
 // line 4 -- padding padding padding padding padding padding padding padding
+// inserted marker -- padding padding padding padding padding padding padding
 // line 5 -- padding padding padding padding padding padding padding padding
 // line 6 -- padding padding padding padding padding padding padding padding
 // line 7 -- padding padding padding padding padding padding padding padding
 // line 8 -- padding padding padding padding padding padding padding padding
@@ -19,9 +20,9 @@
 // line 19 -- padding padding padding padding padding padding padding padding
 // line 20 -- padding padding padding padding padding padding padding padding
 // line 21 -- padding padding padding padding padding padding padding padding
 // line 22 -- padding padding padding padding padding padding padding padding
-// line 23 old -- padding padding padding padding padding padding padding pad
+// line 23 new -- padding padding padding padding padding padding padding pad
 // line 24 -- padding padding padding padding padding padding padding padding
 // line 25 -- padding padding padding padding padding padding padding padding
 // line 26 -- padding padding padding padding padding padding padding padding
 // line 27 -- padding padding padding padding padding padding padding padding
"#;
        let pad = " -- padding padding padding padding padding padding padding padding";
        let mut old_lines = Vec::new();
        for i in 1..=40 {
            if i == 23 {
                old_lines.push(format!("// line 23 old{pad}"));
            } else {
                old_lines.push(format!("// line {i}{pad}"));
            }
        }
        let mut new_lines = Vec::new();
        for i in 1..=40 {
            if i == 5 {
                new_lines.push(format!("// inserted marker{pad}"));
            }
            if i == 24 {
                new_lines.push(format!("// line 23 new{pad}"));
            } else {
                new_lines.push(format!("// line {i}{pad}"));
            }
        }
        let contents = [("wide.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("wide.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content(contents)
            .with_old_content(old_contents)
            .run();
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-two-up-wrapped.json"),
        )
        .with_expand_lines(6);

        let fi = 0;
        assert_eq!(state.output.files[fi].hunks.len(), 2);
        // Anchor = first content row of H2 (the line directly below divider)
        let anchor_new_line = state.output.files[fi].hunks[1].rows[0]
            .new_line
            .expect("H2 must have rows");

        state.render_frame(50, 30);
        let cw = state.last_content_width;
        // Rows are long enough that they wrap into several visual segments.
        assert!(cw < 80, "scenario requires a narrow viewport");

        for click in 1..=2 {
            let gap = state.hunk_leading_gap(fi, 1);
            assert!(gap > 0, "click #{click}: gap must still be positive");
            let div_flat = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
                .unwrap()
                .flat;
            let (starts, counts, seg_counts, _) = state.visual_geometry(cw);
            let div_vis = starts[div_flat];
            // Park scroll so the divider sits mid-viewport
            state.scroll = div_vis.saturating_sub(6);
            let (starts, _counts2, _seg_counts2, _) = state.visual_geometry(cw);
            let dsy = starts[div_flat].saturating_sub(state.scroll);
            let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
            let anchor_screen_pre = state
                .flat_pos_of_coord(fi, Some(anchor_new_line), None)
                .map(|p| starts[p] + div_segs - state.scroll)
                .expect("anchor must exist");

            let hk = state.output.files[fi].hunks[1].clone();
            let txt = hunk_divider_text(1, &hk, gap);
            let idx = txt.find("\u{25b2}\u{25b2}").unwrap();
            let up_col = txt[..idx].chars().count();
            state.execute(TuiCommand::MouseClick {
                row: state.last_viewport_start + dsy + up_col / cw,
                col: up_col % cw,
                shift: false,
            });

            let merged = state.output.files[fi].hunks.len() == 1;
            let (starts2, _c2, _s2, _) = state.visual_geometry(cw);
            let anchor_screen_post = state
                .flat_pos_of_coord(fi, Some(anchor_new_line), None)
                .map(|p| starts2[p].saturating_sub(state.scroll));

            if !merged {
                // Divider survived: it must stay pinned at its screen y.
                let div_vis2 = state
                    .hunk_starts
                    .iter()
                    .find(|hs| hs.file == fi && hs.hunk == 1)
                    .map(|hs| starts2[hs.flat])
                    .unwrap();
                assert_eq!(
                    div_vis2.saturating_sub(state.scroll),
                    dsy,
                    "click #{click}: divider must stay pinned"
                );
            } else {
                // Merge: the below-row must keep its own screen position.
                assert_eq!(
                    anchor_screen_post,
                    Some(anchor_screen_pre),
                    "merged: row below the removed divider must stay locked"
                );
            }
        }
        assert_eq!(
            state.output.files[fi].hunks.len(),
            1,
            "two presses on a 10-line gap must fully consume and merge it"
        );
    }
}
