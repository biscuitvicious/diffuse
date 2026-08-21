//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use std::{collections::HashMap, path::PathBuf};

use diffuse_core::{
    notes::{NotesDB, Side},
    parse::RowKind,
    wire::{OutputFile, OutputRow, PipelineOutput},
};
use diffuse_render::{
    DiffuseTheme, display_text, render_raw_row, render_row_segment, slice_by_visual_width,
    styled_prefix,
};

use crate::{
    command::TuiCommand,
    types::{FlatRow, HunkMeta, HunkStart, Selection},
    view::{
        diff_grid::{render_file_header, render_hunk_divider},
        header::{render_bottom_ribbon, render_header_bar, render_status_bar, visible_width},
        modals::{render_finder_modal, render_help_modal},
    },
};

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
    pub collapsed_files: std::collections::HashSet<usize>,
    pub last_content_width: usize,
    pub terminal_width: usize,
    pub is_dragging_scrollbar: bool,
    pub last_viewport_start: usize,
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
            .position(|r| !r.is_file_header && Self::row_coord(&r.row).is_some())
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
        }
    }

    pub fn with_repo_context(mut self, repo_root: PathBuf, git_args: Vec<String>) -> Self {
        self.repo_root = repo_root;
        self.git_args = git_args;
        self
    }

    /// The visual (reflowed) flat sequence: per file a header banner, then the
    /// engine's pre-assembled visible rows (padded context, merged mod pairs).
    fn build_visual_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let flat = Self::build_flat(output, collapsed_files);
        let hunk_starts = Self::build_hunk_starts(output, &flat);
        (flat, hunk_starts)
    }

    /// The raw unified-diff flat sequence: per file a header banner, then per
    /// hunk an `@@` header row and the hunk rows exactly as loaded in the diff.
    /// Hunk navigation jumps to the `@@` rows.
    fn build_raw_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let mut flat = Vec::new();
        let mut hunk_starts = Vec::new();

        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi, &f.path));
            if collapsed_files.contains(&fi) {
                continue;
            }
            let hunks = if !f.raw_hunks.is_empty() {
                &f.raw_hunks
            } else {
                &f.hunks
            };
            for (hi, h) in hunks.iter().enumerate() {
                flat.push(FlatRow {
                    file_idx: fi,
                    hunk_idx: hi,
                    is_file_header: false,
                    row: OutputRow {
                        kind: diffuse_core::parse::RowKind::Sep,
                        new_line: None,
                        old_line: None,
                        text: format!(
                            "@@ -{},{} +{},{} @@",
                            h.old_start, h.old_count, h.new_start, h.new_count
                        ),
                        del_text: None,
                        spans: Default::default(),
                        runs: Vec::new(),
                        wholesale: false,
                        moved: false,
                    },
                });
                hunk_starts.push(HunkStart {
                    flat: flat.len() - 1,
                    file: fi,
                    hunk: hi,
                });
                for r in &h.rows {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hi,
                        is_file_header: false,
                        row: r.clone(),
                    });
                }
            }
        }
        (flat, hunk_starts)
    }

    /// Build the flat row list (shared by the visual and raw representations).
    fn build_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> Vec<FlatRow> {
        let mut flat = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi, &f.path));
            if collapsed_files.contains(&fi) {
                continue;
            }
            if !f.rows.is_empty() {
                for r in &f.rows {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: 0,
                        is_file_header: false,
                        row: r.clone(),
                    });
                }
            } else {
                for (hi, h) in f.hunks.iter().enumerate() {
                    for r in &h.rows {
                        flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            is_file_header: false,
                            row: r.clone(),
                        });
                    }
                }
            }
        }
        flat
    }

    fn file_header_row(fi: usize, path: &str) -> FlatRow {
        FlatRow {
            file_idx: fi,
            hunk_idx: 0,
            is_file_header: true,
            row: OutputRow {
                kind: diffuse_core::parse::RowKind::File,
                new_line: None,
                old_line: None,
                text: path.to_string(),
                del_text: None,
                spans: Default::default(),
                runs: Vec::new(),
                wholesale: false,
                moved: false,
            },
        }
    }

    /// Locate each hunk's first content row in the flat sequence by file index
    /// and coordinate envelope.
    fn build_hunk_starts(output: &PipelineOutput, flat: &[FlatRow]) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            let file_start = flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_file_header);
            let Some(file_start) = file_start else {
                continue;
            };
            if f.hunks.is_empty() {
                continue;
            }
            let mut search_from = file_start + 1;
            for (hi, h) in f.hunks.iter().enumerate() {
                if hi == 0 {
                    if search_from < flat.len() && flat[search_from].file_idx == fi {
                        starts.push(HunkStart {
                            flat: search_from,
                            file: fi,
                            hunk: hi,
                        });
                    }
                } else {
                    let target_start = if h.new_count > 0 {
                        h.new_start
                    } else {
                        h.old_start
                    };
                    let mut found = None;
                    for (idx, fr) in flat.iter().enumerate().skip(search_from) {
                        if fr.file_idx != fi {
                            break;
                        }
                        if let Some(coord) = Self::row_coord(&fr.row)
                            && coord >= target_start
                        {
                            found = Some(idx);
                            break;
                        }
                    }
                    if let Some(idx) = found {
                        starts.push(HunkStart {
                            flat: idx,
                            file: fi,
                            hunk: hi,
                        });
                        search_from = idx + 1;
                    }
                }
            }
        }
        starts
    }

    /// Per-file per-hunk context allocation. The wire carries no ctx counts,
    /// so recompute them over the hunks the same way `f.rows` was built
    /// (forced 6/6 above/below).
    fn build_hunk_meta(output: &PipelineOutput) -> Vec<Vec<HunkMeta>> {
        use diffuse_core::visible::allocate_context;
        output
            .files
            .iter()
            .map(|f| {
                let mut hunks: Vec<diffuse_core::parse::Hunk> = f
                    .hunks
                    .iter()
                    .map(diffuse_core::wire::OutputHunk::reconstruct)
                    .collect();
                for h in &mut hunks {
                    if h.above_ctx == 0 {
                        h.above_ctx = 6;
                    }
                    if h.below_ctx == 0 {
                        h.below_ctx = 6;
                    }
                }
                let alloc = allocate_context(&hunks, f.new_line_count, f.old_line_count);
                f.hunks
                    .iter()
                    .enumerate()
                    .map(|(i, _h)| HunkMeta {
                        above: alloc[i].above,
                        below: alloc[i].below,
                    })
                    .collect()
            })
            .collect()
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

    pub fn row_coord(row: &OutputRow) -> Option<u32> {
        row.new_line.or(row.old_line)
    }

    /// Commit the note draft onto the current row (or active selection) and
    /// persist it through the notes DB. Mirrors the reference
    /// `commitNoteAction`.
    pub fn commit_note_draft(&mut self) {
        let text = match self.note_draft.take() {
            Some(t) => t.trim().to_string(),
            None => return,
        };
        if text.is_empty() {
            return;
        }
        let (file, start, end, is_new) = if let Some(sel) = self.compute_selection() {
            let Some(file) = self.output.files.get(sel.file_idx) else {
                return;
            };
            (file.path.clone(), sel.start, sel.end, sel.side_is_new)
        } else {
            let Some(file) = self.current_file() else {
                return;
            };
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let Some(c) = Self::row_coord(&row.row) else {
                return;
            };
            (file.path.clone(), c, c, row.row.new_line.is_some())
        };
        self.note_id_counter += 1;
        let created_at = diffuse_core::notes::now_millis();
        let note = diffuse_core::notes::ReviewNote {
            id: format!(
                "{:x}-{:x}-{:x}",
                created_at,
                std::process::id(),
                self.note_id_counter
            ),
            file,
            side: if is_new { Side::New } else { Side::Old },
            start,
            end,
            text,
            created_at,
        };
        diffuse_core::notes::add_note(&mut self.notes, &self.repo, note);
        let _ = diffuse_core::notes::save_notes(&self.notes_path, &self.notes);
        self.confirmed_sel = None;
        self.anchor = None;
    }

    /// Delete notes overlapping the current row or active selection.
    /// Persists immediately.
    pub fn delete_notes(&mut self) {
        let (file, start, end) = if let Some(sel) = self.compute_selection() {
            let Some(file) = self.output.files.get(sel.file_idx) else {
                return;
            };
            (file.path.clone(), sel.start, sel.end)
        } else {
            let Some(file) = self.current_file() else {
                return;
            };
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let Some(c) = Self::row_coord(&row.row) else {
                return;
            };
            (file.path.clone(), c, c)
        };
        let new_removed = diffuse_core::notes::remove_notes(
            &mut self.notes,
            &self.repo,
            &file,
            Side::New,
            start,
            end,
        );
        let old_removed = diffuse_core::notes::remove_notes(
            &mut self.notes,
            &self.repo,
            &file,
            Side::Old,
            start,
            end,
        );
        let removed = new_removed + old_removed;
        let _ = diffuse_core::notes::save_notes(&self.notes_path, &self.notes);
        self.confirmed_sel = None;
        self.anchor = None;
        self.status_message = Some((
            if removed > 0 {
                format!(
                    "deleted {removed} note{}",
                    if removed == 1 { "" } else { "s" }
                )
            } else {
                "no note here".to_string()
            },
            removed > 0,
        ));
    }

    /// Viewport jump margin: 1/10 of the viewport height (rounded half-up,
    /// minimum 1 line).
    pub fn viewport_margin(&self) -> usize {
        ((self.viewport_height + 5) / 10).max(1)
    }

    /// Position cursor at `target_row` and adjust scroll to preserve a 1/10
    /// viewport margin above the jump point (or below if jumping to bottom).
    pub fn jump_to_row(&mut self, target_row: usize, is_bottom: bool) {
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let margin = self.viewport_margin();
        let max_rows = self.viewport_height.max(4);
        let (starts, _, _, total_vis_lines) = self.visual_geometry(self.last_content_width);

        let hunk_div_offset = if self.hunk_starts.iter().any(|h| h.flat == self.row_idx) {
            1
        } else {
            0
        };
        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0) + hunk_div_offset;

        if is_bottom {
            self.scroll = target_vis.saturating_sub(max_rows.saturating_sub(1 + margin));
        } else {
            self.scroll = target_vis.saturating_sub(margin);
        }

        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Jump to the line in the current file closest to `line_num`.
    pub fn goto_line(&mut self, line_num: u32) {
        let cur_fi = self.current_file_idx();
        let mut best: Option<(usize, u32)> = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != cur_fi || item.is_file_header {
                continue;
            }
            if let Some(c) = Self::row_coord(&item.row) {
                let dist = c.abs_diff(line_num);
                match best {
                    None => best = Some((i, dist)),
                    Some((_, best_dist)) if dist < best_dist => best = Some((i, dist)),
                    _ => {}
                }
            }
        }
        if let Some((idx, _)) = best {
            self.jump_to_row(idx, false);
        }
    }

    /// Map of flat-row index → pre-rendered note callout lines for the
    /// current file (`💬 [start-end] <markdown>`). A note anchors to the
    /// LAST row whose coordinate falls in its range, else the file's first
    /// row — mirroring the reference `noteByRow`.
    fn note_callouts(&self) -> HashMap<usize, Vec<String>> {
        let Some(f) = self.current_file() else {
            return HashMap::new();
        };
        let notes = diffuse_core::notes::notes_for_file(&self.notes, &self.repo, &f.path);
        if notes.is_empty() {
            return HashMap::new();
        }
        let file_idx = self.current_file_idx();
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        for n in notes {
            let mut at: Option<usize> = None;
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != file_idx {
                    continue;
                }
                if let Some(c) = Self::row_coord(&item.row)
                    && c >= n.start
                    && c <= n.end
                {
                    at = Some(i);
                }
            }
            let at = at.unwrap_or_else(|| {
                self.flat
                    .iter()
                    .position(|it| it.file_idx == file_idx)
                    .unwrap_or(0)
            });
            let where_str = if n.end == n.start {
                format!("{}", n.start)
            } else {
                format!("{}-{}", n.start, n.end)
            };
            let mut body = String::new();
            for r in diffuse_core::notes::note_runs(&n.text) {
                let color = r.color.as_deref().unwrap_or("warning");
                body.push_str(&self.theme.render_token(color, &r.text, "ctx", None, r.sgr));
            }
            let line = format!("\x1b[38;5;220m   💬 [{}] \x1b[39m{}", where_str, body);
            out.entry(at).or_default().push(line);
        }
        out
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
        lines.push(render_header_bar(&self.repo, f, width));

        // Secondary status bar
        let total_files = self.output.files.len();
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let total_hunks = f.hunks.len().max(1);
        lines.push(render_status_bar(crate::view::StatusBarContext {
            cur_fi,
            total_files,
            cur_hunk,
            total_hunks,
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            num_buffer: &self.num_buffer,
            branch: &self.output.branch,
            width,
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
                    .and_then(|fr| Self::row_coord(&fr.row))
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
        let cursor_vis =
            starts.get(self.row_idx).copied().unwrap_or(0) + hunk_div_offset + self.vis_line;

        if cursor_vis < self.scroll {
            self.scroll = cursor_vis;
        }
        if cursor_vis >= self.scroll + max_rows {
            self.scroll = cursor_vis.saturating_sub(max_rows.saturating_sub(1));
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
            let r = &item.row;

            if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                    let h = &self.output.files[div_fi].hunks[div_hi];
                    let ctx = self
                        .hunk_meta
                        .get(div_fi)
                        .and_then(|v| v.get(div_hi))
                        .map(|m| (m.above, m.below))
                        .unwrap_or((0, 0));
                    let is_cur = div_fi == self.current_file_idx() && cur_hunk == Some(div_hi);
                    lines.push(render_hunk_divider(div_hi, h, ctx, is_cur, content_width));
                }
                vis_i += 1;
            }

            if item.is_file_header {
                if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                    let file_obj = &self.output.files[item.file_idx];
                    let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                    let is_selected = idx == self.row_idx;
                    lines.push(render_file_header(
                        file_obj,
                        is_collapsed,
                        is_selected,
                        in_band,
                        content_width,
                    ));
                }
                vis_i += 1;
            } else {
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
                            let (prefix_str, text_cols) =
                                if r.kind == diffuse_core::parse::RowKind::Sep {
                                    ("", content_width.max(1))
                                } else {
                                    (pfx.as_str(), content_width.saturating_sub(6).max(1))
                                };
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

    pub fn open_file_finder(&mut self) {
        let mut finder =
            crate::finder::FileFinderState::new(&self.output.files, self.row_idx, self.scroll);
        // Default selected match to the current file
        let cur_fi = self.current_file_idx();
        if let Some(pos) = finder.matches.iter().position(|m| m.file_idx == cur_fi) {
            finder.selected_idx = pos;
        }
        self.file_finder = Some(finder);
    }

    pub fn cancel_file_finder(&mut self) {
        if let Some(finder) = self.file_finder.take() {
            self.row_idx = finder.initial_row_idx;
            self.scroll = finder.initial_scroll;
        }
    }

    pub fn confirm_file_finder(&mut self) {
        self.file_finder = None;
    }

    pub fn track_finder_selection(&mut self) {
        if let Some(fi) = self
            .file_finder
            .as_ref()
            .and_then(super::finder::FileFinderState::selected_file_idx)
        {
            self.goto_file_idx(fi);
        }
    }

    pub fn goto_file_idx(&mut self, fi: usize) {
        let first = self.first_row_of_file(fi);
        self.jump_to_row(first, false);
    }

    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        if let Some(item) = self.flat.get(idx) {
            if item.is_file_header {
                return 1;
            }
            let r = &item.row;
            if self.raw_diff {
                use unicode_width::UnicodeWidthStr;
                let text_cols = if r.kind == diffuse_core::parse::RowKind::Sep {
                    content_width.max(1)
                } else {
                    content_width.saturating_sub(6).max(1)
                };
                let len = r.text.width();
                if len == 0 { 1 } else { len.div_ceil(text_cols) }
            } else {
                use unicode_width::UnicodeWidthStr;
                let text_cols = content_width.saturating_sub(6).max(1);
                let disp = display_text(
                    &r.text,
                    r.del_text.as_deref(),
                    r.kind,
                    r.moved,
                    &r.runs,
                    self.col_offset,
                    self.word_diff,
                );
                let len = disp.width();
                if len == 0 { 1 } else { len.div_ceil(text_cols) }
            }
        } else {
            1
        }
    }

    pub fn move_down(&mut self) {
        let seg_count = self.row_seg_count(self.row_idx, self.last_content_width);
        if self.vis_line + 1 < seg_count {
            self.vis_line += 1;
        } else if self.row_idx + 1 < self.flat.len() {
            self.row_idx += 1;
            self.vis_line = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.vis_line > 0 {
            self.vis_line -= 1;
        } else if self.row_idx > 0 {
            self.row_idx -= 1;
            let prev_seg_count = self.row_seg_count(self.row_idx, self.last_content_width);
            self.vis_line = prev_seg_count.saturating_sub(1);
        }
    }

    pub fn next_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let target = self
            .hunk_starts
            .iter()
            .find(|h| h.flat > self.row_idx)
            .map(|h| h.flat)
            .unwrap_or(self.hunk_starts[0].flat);
        self.jump_to_row(target, false);
    }

    /// Computes full visual layout mapping: starts, counts, `seg_counts`, and
    /// total visual lines.
    pub fn visual_geometry(
        &self,
        content_width: usize,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, usize) {
        let callouts = self.note_callouts();
        let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
            HashMap::new()
        } else {
            self.hunk_starts
                .iter()
                .map(|h| (h.flat, (h.file, h.hunk)))
                .collect()
        };

        let mut starts = Vec::with_capacity(self.flat.len());
        let mut counts = Vec::with_capacity(self.flat.len());
        let mut seg_counts = Vec::with_capacity(self.flat.len());
        let mut total_vis_lines = 0;

        for (idx, item) in self.flat.iter().enumerate() {
            starts.push(total_vis_lines);
            let div_lines = if div_at.contains_key(&idx) { 1 } else { 0 };
            let header_lines = if item.is_file_header { 1 } else { 0 };
            let row_segs = if item.is_file_header {
                0
            } else {
                self.row_seg_count(idx, content_width)
            };
            let note_lines = callouts.get(&idx).map_or(0, std::vec::Vec::len);
            let item_total = div_lines + header_lines + row_segs + note_lines;
            counts.push(item_total);
            seg_counts.push(row_segs.max(1));
            total_vis_lines += item_total;
        }

        (starts, counts, seg_counts, total_vis_lines)
    }

    /// Scroll to a specific visual line offset (used by scrollbar dragging and
    /// paging).
    pub fn scroll_to_visual(&mut self, target_vis: usize) {
        let (starts, counts, seg_counts, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let max_rows = self.viewport_height.max(4);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = target_vis.min(max_scroll);

        // Keep cursor in view
        let cursor_flat = self.row_idx;
        let hunk_div_offset = if self.hunk_starts.iter().any(|h| h.flat == cursor_flat) {
            1
        } else {
            0
        };
        let cursor_vis =
            starts.get(cursor_flat).copied().unwrap_or(0) + hunk_div_offset + self.vis_line;

        if cursor_vis < self.scroll {
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > self.scroll {
                    self.row_idx = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = self.scroll.saturating_sub(s + hunk_div);
                    self.vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
        } else if cursor_vis >= self.scroll + max_rows {
            let bottom_vis = (self.scroll + max_rows).saturating_sub(1);
            let mut target_row = self.row_idx;
            let mut target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate().rev() {
                if s <= bottom_vis {
                    target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = bottom_vis.saturating_sub(s + hunk_div);
                    target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let target = self
            .hunk_starts
            .iter()
            .rev()
            .find(|h| h.flat < self.row_idx)
            .map(|h| h.flat)
            .unwrap_or(self.hunk_starts[self.hunk_starts.len() - 1].flat);
        self.jump_to_row(target, false);
    }

    pub fn current_hunk(&self) -> Option<usize> {
        let fi = self.current_file_idx();
        let f = self.output.files.get(fi)?;
        if f.hunks.is_empty() {
            return None;
        }
        let Some(row) = self.current_flat_row() else {
            return Some(0);
        };
        let Some(coord) = Self::row_coord(&row.row) else {
            return Some(0);
        };
        let mut idx = Some(0);
        for (i, h) in f.hunks.iter().enumerate() {
            let hs = if h.new_count > 0 {
                h.new_start
            } else {
                h.old_start
            };
            if coord >= hs {
                idx = Some(i);
            }
        }
        idx
    }

    /// Marks all rows belonging to the current hunk envelope for gutter
    /// highlight.
    fn hunk_ctx_extent_rows(&self) -> Vec<bool> {
        let mut set = vec![false; self.flat.len()];
        let Some(hi) = self.current_hunk() else {
            return set;
        };
        let fi = self.current_file_idx();
        let Some(f) = self.output.files.get(fi) else {
            return set;
        };
        let Some(h) = f.hunks.get(hi) else {
            return set;
        };
        let old_lo = h.old_start;
        let old_hi = h.old_start + h.old_count;
        let new_lo = h.new_start;
        let new_hi = h.new_start + h.new_count;

        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != fi || item.is_file_header {
                continue;
            }
            let r = &item.row;
            if let Some(nl) = r.new_line
                && nl >= new_lo
                && nl < new_hi
            {
                set[i] = true;
                continue;
            }
            if let Some(ol) = r.old_line
                && ol >= old_lo
                && ol < old_hi
            {
                set[i] = true;
            }
        }
        set
    }

    /// Compute the exact visual line position of the cursor in the current
    /// layout.
    pub fn cursor_visual_line(&self) -> usize {
        let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
            HashMap::new()
        } else {
            self.hunk_starts
                .iter()
                .map(|h| (h.flat, (h.file, h.hunk)))
                .collect()
        };
        let callouts = self.note_callouts();
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

        let mut vis_i = 0;
        for (i, item) in self.flat.iter().enumerate() {
            if div_at.contains_key(&i) {
                vis_i += 1;
            }
            if item.is_file_header {
                if i == self.row_idx {
                    return vis_i;
                }
                vis_i += 1;
            } else {
                let seg_count = self.row_seg_count(i, content_width);
                if i == self.row_idx {
                    return vis_i + self.vis_line.min(seg_count.saturating_sub(1));
                }
                vis_i += seg_count;
            }
            vis_i += callouts.get(&i).map_or(0, Vec::len);
        }
        vis_i
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
        let is_sep_row = self
            .current_flat_row()
            .is_some_and(|fr| fr.row.kind == RowKind::Sep);

        let (file, target_new, target_old) = {
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let r = &row.row;
            if r.new_line.is_some() || r.old_line.is_some() {
                (row.file_idx, r.new_line, r.old_line)
            } else {
                let mut neighbor_coord = None;
                for offset in 1..self.flat.len() {
                    if self.row_idx >= offset {
                        let prev = &self.flat[self.row_idx - offset];
                        if prev.file_idx == row.file_idx
                            && !prev.is_file_header
                            && let Some(c) = Self::row_coord(&prev.row)
                        {
                            neighbor_coord = Some(c);
                            break;
                        }
                    }
                    if self.row_idx + offset < self.flat.len() {
                        let next = &self.flat[self.row_idx + offset];
                        if next.file_idx == row.file_idx
                            && !next.is_file_header
                            && let Some(c) = Self::row_coord(&next.row)
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
                        .is_some_and(|fr| fr.row.kind == RowKind::Sep)
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
                if item.file_idx == file && item.row.old_line == Some(target_ol) {
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
                if item.file_idx == file && item.row.new_line == Some(target_nl) {
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
                if item.file_idx == file && item.row.old_line == Some(target_ol) {
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
                        && (item.row.new_line == Some(c) || item.row.old_line == Some(c))
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
                if let Some(c) = Self::row_coord(&item.row) {
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

    pub fn toggle_file_collapse(&mut self, fi: usize) {
        if self.collapsed_files.contains(&fi) {
            self.collapsed_files.remove(&fi);
        } else {
            self.collapsed_files.insert(fi);
        }
        self.rebuild_flat_preserving_cursor(fi);
    }

    pub fn toggle_current_file_collapse(&mut self) {
        let fi = self.current_file_idx();
        self.toggle_file_collapse(fi);
    }

    fn rebuild_flat_preserving_cursor(&mut self, target_fi: usize) {
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // If target file is now collapsed, jump to its header row
        if self.collapsed_files.contains(&target_fi) {
            self.row_idx = self.first_row_of_file(target_fi);
        } else if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }
        self.adjust_scroll();
    }

    fn adjust_scroll(&mut self) {
        let max_rows = self.viewport_height.saturating_sub(2);
        if self.row_idx < self.scroll {
            self.scroll = self.row_idx;
        }
        if self.row_idx >= self.scroll + max_rows {
            self.scroll = self.row_idx.saturating_sub(max_rows.saturating_sub(1));
        }
    }

    pub fn reload(&mut self, output: PipelineOutput) {
        let (file_path, coord, prefer_new) = {
            let cur_path = self
                .current_file()
                .map(|f| f.path.clone())
                .unwrap_or_default();
            let row = self.current_flat_row();
            let c = row.and_then(|r| Self::row_coord(&r.row)).unwrap_or(0);
            let prefer = row.map(|r| r.row.new_line.is_some()).unwrap_or(true);
            (cur_path, c, prefer)
        };

        for (fi, f) in output.files.iter().enumerate() {
            if f.status == "untracked" && !self.output.files.iter().any(|old| old.path == f.path) {
                self.collapsed_files.insert(fi);
            }
        }

        self.output = output;
        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            self.should_quit = true;
            return;
        }

        self.hunk_meta = Self::build_hunk_meta(&self.output);
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.anchor = None;
        self.confirmed_sel = None;

        let target_fi = self
            .output
            .files
            .iter()
            .position(|f| f.path == file_path)
            .unwrap_or(0);

        self.goto_coord(target_fi, coord, prefer_new);
    }

    /// Jump to the first flat row in `file` carrying `coord` (preferring the
    /// given side); fall back to the file's first coordinate row — the
    /// reference's coordinate-resume behavior.
    fn goto_coord(&mut self, file: usize, coord: u32, prefer_new: bool) {
        let mut idx = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file {
                continue;
            }
            let r = &item.row;
            let side_ok = if prefer_new {
                r.new_line.is_some()
            } else {
                r.old_line.is_some()
            };
            if Self::row_coord(r) == Some(coord) && side_ok {
                idx = Some(i);
                break;
            }
        }
        let idx = idx.unwrap_or_else(|| self.first_row_of_file(file));
        self.row_idx = idx;
        self.scroll = 0;
    }

    pub fn next_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in self.row_idx + 1..self.flat.len() {
            let next_fi = self.flat[i].file_idx;
            if next_fi > cur_fi {
                self.goto_file_idx(next_fi);
                return;
            }
        }
    }

    pub fn prev_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in (0..self.row_idx).rev() {
            let prev_fi = self.flat[i].file_idx;
            if prev_fi < cur_fi {
                self.goto_file_idx(prev_fi);
                return;
            }
        }
    }

    pub fn first_row_of_file(&self, fi: usize) -> usize {
        self.flat
            .iter()
            .position(|r| r.file_idx == fi && r.is_file_header)
            .unwrap_or(0)
    }

    pub fn last_row_of_file(&self, fi: usize) -> usize {
        match self.flat.iter().position(|r| r.file_idx > fi) {
            Some(next) => next - 1,
            None => self.flat.len().saturating_sub(1),
        }
    }

    fn next_coord_row(&self, from: usize) -> usize {
        for i in from..self.flat.len() {
            if Self::row_coord(&self.flat[i].row).is_some() {
                return i;
            }
        }
        from
    }

    fn prev_coord_row(&self, from: usize) -> usize {
        let hi = from.min(self.flat.len().saturating_sub(1));
        for i in (0..=hi).rev() {
            if Self::row_coord(&self.flat[i].row).is_some() {
                return i;
            }
        }
        from
    }

    /// Line-coordinate selection over the current file, mirroring the
    /// reference `computeSelection`: anchor..cursor snapped to coordinate
    /// rows and clamped to the file. With no active anchor, the confirmed
    /// selection is returned when it lives in the current file.
    pub fn compute_selection(&self) -> Option<Selection> {
        let Some(anchor) = self.anchor else {
            return self
                .confirmed_sel
                .filter(|s| s.file_idx == self.current_file_idx());
        };
        let c_file = self.current_file_idx();
        let mut lo = anchor.min(self.row_idx);
        let mut hi = anchor.max(self.row_idx);
        if self.flat.get(anchor).map(|r| r.file_idx) != Some(c_file) {
            lo = self.first_row_of_file(c_file);
        }
        let fs = self.first_row_of_file(c_file);
        let fe = self.last_row_of_file(c_file);
        lo = lo.min(fe).max(fs);
        hi = hi.min(fe).max(fs);
        if hi < lo {
            hi = lo;
        }
        lo = self.next_coord_row(lo);
        hi = self.prev_coord_row(hi);
        let first = self.flat.get(lo)?;
        let last = self.flat.get(hi)?;
        let side_is_new = first.row.new_line.is_some() || last.row.new_line.is_some();
        let f = Self::row_coord(&first.row)?;
        let l = Self::row_coord(&last.row)?;
        Some(Selection {
            file_idx: c_file,
            start: f.min(l),
            end: f.max(l),
            side_is_new,
        })
    }

    /// `v`/`V`: start the anchor at the cursor, or confirm the range into a
    /// selection. Mirror of the reference `v` handler.
    pub fn start_or_confirm_selection(&mut self) {
        match self.anchor {
            None => {
                self.anchor = Some(self.row_idx);
                self.confirmed_sel = None;
            }
            Some(_) => {
                self.confirmed_sel = self.compute_selection();
                self.anchor = None;
            }
        }
    }

    /// Set of flat rows inside the visual selection band (anchor active or a
    /// confirmed selection), mirroring `component_render`'s band computation.
    fn band_rows(&self) -> Vec<bool> {
        let mut set = vec![false; self.flat.len()];
        if let Some(anchor) = self.anchor {
            let c_file = self.current_file_idx();
            let lo = anchor.min(self.row_idx);
            let hi = anchor.max(self.row_idx);
            let lo = if self.flat.get(anchor).map(|r| r.file_idx) != Some(c_file) {
                self.first_row_of_file(c_file)
            } else {
                lo
            };
            let fs = self.first_row_of_file(c_file);
            let fe = self.last_row_of_file(c_file);
            for i in fs.max(lo)..=fe.min(hi) {
                if i < set.len() {
                    set[i] = true;
                }
            }
        } else if let Some(conf) = &self.confirmed_sel
            && conf.file_idx == self.current_file_idx()
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != conf.file_idx {
                    continue;
                }
                if let Some(c) = Self::row_coord(&item.row)
                    && c >= conf.start
                    && c <= conf.end
                {
                    set[i] = true;
                }
            }
        }
        set
    }

    pub fn page_down(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let (starts, _counts, seg_counts, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let max_rows = self.viewport_height.max(4);
        let bottom_vis = (self.scroll + max_rows)
            .saturating_sub(1)
            .min(total_vis_lines.saturating_sub(1));

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate().rev() {
            if s <= bottom_vis {
                target_row = i;
                let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                    1
                } else {
                    0
                };
                let offset = bottom_vis.saturating_sub(s + hunk_div);
                target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at bottom -> scroll 1/3 of viewport down and place cursor at new
            // bottom
            let step = ((max_rows * 2 + 3) / 6).max(1);
            self.scroll_to_visual(self.scroll + step);
            let new_bottom_vis = (self.scroll + max_rows)
                .saturating_sub(1)
                .min(total_vis_lines.saturating_sub(1));
            let mut new_target_row = self.row_idx;
            let mut new_target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate().rev() {
                if s <= new_bottom_vis {
                    new_target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = new_bottom_vis.saturating_sub(s + hunk_div);
                    new_target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = new_target_row;
            self.vis_line = new_target_vis_line;
        } else {
            // Jump cursor to bottom of visible page
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn page_up(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let (starts, counts, seg_counts, _total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let top_vis = self.scroll;

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate() {
            if s + counts[i] > top_vis {
                target_row = i;
                let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                    1
                } else {
                    0
                };
                let offset = top_vis.saturating_sub(s + hunk_div);
                target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at top -> scroll 1/3 of viewport up and place cursor at new top
            let max_rows = self.viewport_height.max(4);
            let step = ((max_rows * 2 + 3) / 6).max(1);
            self.scroll_to_visual(self.scroll.saturating_sub(step));
            let new_top_vis = self.scroll;
            let mut new_target_row = self.row_idx;
            let mut new_target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > new_top_vis {
                    new_target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = new_top_vis.saturating_sub(s + hunk_div);
                    new_target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = new_target_row;
            self.vis_line = new_target_vis_line;
        } else {
            // Jump cursor to top of visible page
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn move_top(&mut self) {
        self.jump_to_row(0, false);
    }

    pub fn move_bottom(&mut self) {
        if !self.flat.is_empty() {
            self.jump_to_row(self.flat.len() - 1, true);
        }
    }

    /// Execute a high-level user command to mutate application state.
    pub fn execute(&mut self, cmd: TuiCommand) {
        match cmd {
            TuiCommand::Quit => self.should_quit = true,
            TuiCommand::Dismiss => {
                if self.file_finder.is_some() {
                    self.cancel_file_finder();
                } else if self.note_draft.is_some() {
                    self.note_draft = None;
                } else if self.show_help {
                    self.show_help = false;
                } else if self.anchor.is_some() {
                    self.anchor = None;
                } else {
                    self.should_quit = true;
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
            TuiCommand::ToggleCollapseFile(None) => self.toggle_current_file_collapse(),
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
                    && let Some(c) = Self::row_coord(&row.row)
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
            TuiCommand::MouseClick { row, col } => {
                let header_offset = self.last_viewport_start;
                let vh = self.viewport_height;
                let is_on_scrollbar = col >= self.last_content_width;
                if is_on_scrollbar && row >= header_offset && row < header_offset + vh {
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
                } else if row >= header_offset && row < header_offset + vh {
                    self.is_dragging_scrollbar = false;
                    let target_vis = self.scroll + (row - header_offset);
                    let (starts, counts, _seg, _total_vis) =
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
            TuiCommand::MouseDrag { row, col: _ } => {
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
                    if self.anchor.is_none() {
                        self.anchor = Some(self.row_idx);
                    }
                    let target_vis = self.scroll + (row - header_offset);
                    let (starts, counts, _seg, _total_vis) =
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::truncate_pad;

    #[test]
    fn test_app_state_scroll_through_all_rows() {
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
        let contents = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();

        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            &contents,
            Some(&old_contents),
            None,
            None,
        );
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );

        // Step down through every single row and verify render_frame never panics
        for _ in 0..state.flat.len() + 5 {
            let frame = state.render_frame(100, 30);
            assert!(!frame.is_empty());
            state.move_down();
        }

        // Step up through every single row
        for _ in 0..state.flat.len() + 5 {
            let frame = state.render_frame(80, 24);
            assert!(!frame.is_empty());
            state.move_up();
        }
    }

    fn sample_output() -> PipelineOutput {
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
        let contents = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        diffuse_core::pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None)
    }

    fn sample_output_two_hunks() -> PipelineOutput {
        let diff = r#"diff --git a/foo.rs b/foo.rs
index 1111111..2222222 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn a() {
+    let x = 1;
     return 1;
 }
@@ -12,3 +13,4 @@ fn b() {
 fn b() {
+    let y = 2;
     return 2;
 }
"#;
        let mut old_lines = vec![
            "fn a() {".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 4..=11 {
            old_lines.push(format!("    // filler {i}"));
        }
        old_lines.push("fn b() {".to_string());
        old_lines.push("    return 2;".to_string());
        old_lines.push("}".to_string());

        let mut new_lines = vec![
            "fn a() {".to_string(),
            "    let x = 1;".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 5..=12 {
            new_lines.push(format!("    // filler {i}"));
        }
        new_lines.push("fn b() {".to_string());
        new_lines.push("    let y = 2;".to_string());
        new_lines.push("    return 2;".to_string());
        new_lines.push("}".to_string());

        let contents = [("foo.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_contents = [("foo.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        diffuse_core::pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None)
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

        let notes = diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "check this **carefully**");
        assert!(
            notes[0].start > 0,
            "note must anchor to a real row coordinate"
        );

        let on_disk = diffuse_core::notes::load_notes(&notes_path);
        assert_eq!(
            diffuse_core::notes::notes_for_file(&on_disk, "test_repo", &file).len(),
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
            diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file).is_empty(),
            "delete must remove the note from the DB"
        );
        let on_disk2 = diffuse_core::notes::load_notes(&notes_path);
        assert!(
            diffuse_core::notes::notes_for_file(&on_disk2, "test_repo", &file).is_empty(),
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
        // above-context rows (1020-1024), then the real rows: ctx
        // " let x = 1;" (1025), the mod pair (1026), the add (1027). Walk to
        // the first real row, anchor, extend over the mod.
        for _ in 0..5 {
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
        for _ in 0..5 {
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
        assert_eq!((sel.start, sel.end), (1025, 1027));

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
        assert_eq!(sel.start, 1020, "snap to the first padded ctx row");
        assert_eq!(sel.end, 1020);
    }

    #[test]
    fn test_hunk_starts_and_navigation() {
        let mut state = AppState::new(
            sample_output_two_hunks(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        assert_eq!(state.hunk_starts.len(), 2, "both hunks must be located");
        // Start from file header (row 0), next jumps to hunk 1's start
        state.row_idx = 0;
        state.next_hunk();
        assert_eq!(state.current_hunk(), Some(0));

        state.next_hunk();
        let at = state.row_idx;
        assert_eq!(
            state.current_hunk(),
            Some(1),
            "next_hunk must land in hunk 2"
        );
        assert!(
            at > state.hunk_starts[0].flat,
            "next_hunk must jump past hunk 1"
        );

        state.prev_hunk();
        assert_eq!(state.current_hunk(), Some(0), "prev_hunk returns to hunk 1");
    }

    #[test]
    fn test_hunk_divider_renders_and_extent_lift() {
        let mut state = AppState::new(
            sample_output_two_hunks(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );

        let frame = state.render_frame(120, 40);
        assert!(
            frame.contains("H1"),
            "the first hunk needs a divider: {:.300}",
            frame
        );
        assert!(frame.contains("H2"), "the second hunk needs a divider");
        assert!(
            frame.contains("−1,3 +1,4"),
            "the divider carries the hunk span"
        );
        assert!(
            frame.contains("\x1b[48;5;236m"),
            "the current hunk's divider is the bright bar"
        );
        assert!(
            frame.contains("\x1b[38;5;252m"),
            "number gutter in the current hunk's envelope gets the highlighted color"
        );
        assert!(
            !frame.contains("\x1b[48;5;238m"),
            "ctx rows should not have distracting background tinting"
        );

        state.next_hunk();
        let frame2 = state.render_frame(120, 40);
        assert!(
            frame2.contains("H2"),
            "the second hunk keeps its divider in hunk 2"
        );
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

        let del = state
            .flat
            .iter()
            .find(|fr| fr.row.kind == diffuse_core::parse::RowKind::Del);
        assert!(
            del.is_some() && del.unwrap().row.text.contains("old comment"),
            "the mod pair must un-collapse into a - line"
        );
        let add = state.flat.iter().find(|fr| {
            fr.row.kind == diffuse_core::parse::RowKind::Add && fr.row.text.contains("new comment")
        });
        assert!(add.is_some(), "the mod pair must un-collapse into a + line");

        let sep = state
            .flat
            .iter()
            .find(|fr| fr.row.kind == diffuse_core::parse::RowKind::Sep);
        assert_eq!(
            sep.map(|fr| fr.row.text.as_str()),
            Some("@@ -1025,6 +1025,8 @@")
        );

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
        // flat[7] is the mod pair (new 1026); walk onto it.
        for _ in 0..6 {
            state.move_down();
        }
        let r = &state.flat[state.row_idx].row;
        assert_eq!(r.kind, diffuse_core::parse::RowKind::Mod);
        assert_eq!(r.new_line, Some(1026));

        state.toggle_raw_diff();
        let r = &state.flat[state.row_idx].row;
        assert_eq!(
            r.kind,
            diffuse_core::parse::RowKind::Add,
            "the cursor resolves to the + line of the pair"
        );
        assert_eq!(r.new_line, Some(1026), "same coordinate after the switch");

        state.toggle_raw_diff();
        let r = &state.flat[state.row_idx].row;
        assert_eq!(
            AppState::row_coord(r),
            Some(1026),
            "round-tripping lands back on the same line"
        );
        assert!(!state.raw_diff);
    }

    #[test]
    fn test_untracked_files_collapsed_by_default() {
        let diff = r#"diff --git a/tracked.rs b/tracked.rs
index 1111111..2222222 100644
--- a/tracked.rs
+++ b/tracked.rs
@@ -1 +1,2 @@
 fn main() {}
+fn extra() {}
"#;
        let contents = [
            (
                "tracked.rs".to_string(),
                "fn main() {}\nfn extra() {}\n".to_string(),
            ),
            (
                "untracked.rs".to_string(),
                "fn new_untracked() {}\n".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let mut out = diffuse_core::pipeline::run_pipeline_full(diff, &contents, None, None, None);
        // Add an untracked file to output
        out.files.push(diffuse_core::wire::OutputFile {
            path: "untracked.rs".to_string(),
            status: "untracked".to_string(),
            new_line_count: 1,
            old_line_count: 0,
            new_content: vec!["fn new_untracked() {}".to_string()],
            old_content: Vec::new(),
            hunks: vec![diffuse_core::wire::OutputHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                rows: vec![diffuse_core::wire::OutputRow {
                    kind: diffuse_core::parse::RowKind::Add,
                    new_line: Some(1),
                    old_line: None,
                    text: "fn new_untracked() {}".to_string(),
                    del_text: None,
                    spans: Default::default(),
                    runs: Vec::new(),
                    wholesale: false,
                    moved: false,
                }],
            }],
            raw_hunks: Vec::new(),
            counts: diffuse_core::counts::FileCounts {
                adds: 1,
                dels: 0,
                mods: 0,
                movs: 0,
            },
            fences: Vec::new(),
            notes: Vec::new(),
            content_runs: Vec::new(),
            rows: Vec::new(),
        });

        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-untracked.json"),
        );

        assert!(
            state.collapsed_files.contains(&1),
            "untracked file (index 1) must be collapsed by default"
        );
        assert!(
            !state.collapsed_files.contains(&0),
            "tracked file (index 0) must be expanded by default"
        );

        // The flat sequence must contain rows for file 0, but only the header row for
        // file 1
        let file_1_rows: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
        assert_eq!(
            file_1_rows.len(),
            1,
            "collapsed file only has 1 flat row (header)"
        );
        assert!(file_1_rows[0].is_file_header);

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("▸ untracked.rs (untracked)"),
            "frame shows collapsed indicator"
        );

        // Toggle collapse on file 1 to expand it
        state.toggle_file_collapse(1);
        assert!(
            !state.collapsed_files.contains(&1),
            "untracked file is now expanded"
        );
        let file_1_rows_expanded: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
        assert!(
            file_1_rows_expanded.len() > 1,
            "expanded file has header and content rows"
        );

        let frame_expanded = state.render_frame(100, 30);
        assert!(
            frame_expanded.contains("▾ untracked.rs (untracked)"),
            "frame shows expanded indicator"
        );
    }

    #[test]
    fn test_render_frame_lines_do_not_overflow_width() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-overflow.json"),
        );
        for width in [40, 60, 80, 100, 120] {
            for height in [15, 24, 30, 40] {
                let frame = state.render_frame(width, height);
                for line in frame.lines() {
                    let vis = visible_width(line);
                    assert!(
                        vis <= width,
                        "line visible width {vis} exceeds terminal width {width}: {line:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_word_wrap_and_visual_subline_navigation() {
        let mut out = sample_output();
        // Add a very long line to file 0
        out.files[0].rows[0].text = "A".repeat(150);
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-wrap.json"),
        );
        // content_width = 80 - 1 (sb) = 79. text_cols = 79 - 6 = 73.
        // 150 chars -> 3 segments (73 + 73 + 4).
        let seg_count = state.row_seg_count(state.row_idx, 79);
        assert_eq!(seg_count, 3, "150 chars should produce 3 visual segments");

        state.last_content_width = 79;
        assert_eq!(state.vis_line, 0);

        // move_down advances vis_line within row
        state.move_down();
        assert_eq!(state.vis_line, 1);
        state.move_down();
        assert_eq!(state.vis_line, 2);

        // next move_down moves to next row and resets vis_line
        let start_row = state.row_idx;
        state.move_down();
        assert_eq!(state.row_idx, start_row + 1);
        assert_eq!(state.vis_line, 0);

        // move_up moves back to previous row and lands on its last visual segment
        state.move_up();
        assert_eq!(state.row_idx, start_row);
        assert_eq!(state.vis_line, 2);
    }

    #[test]
    fn test_current_line_highlight_covers_gutter() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-gutter.json"),
        );
        // Context row selected
        state.row_idx = 1;
        let frame_ctx = state.render_frame(80, 24);
        assert!(
            frame_ctx.contains("\x1b[48;5;237m\x1b[38;5;"),
            "selected context row must start with cursor background before the gutter numbers"
        );

        // Unselected addition row starts with addition background before the gutter
        // numbers
        assert!(
            frame_ctx.contains("\x1b[48;5;22m\x1b[38;5;151m"),
            "addition row must start with green tint before gutter numbers"
        );
    }

    #[test]
    fn test_emoji_and_wide_char_visual_width() {
        assert_eq!(visible_width("🦀"), 2);
        assert_eq!(visible_width("\x1b[38;5;120m🦀\x1b[0m"), 2);
        assert_eq!(visible_width("hello 🦀 world"), 14);

        let padded = truncate_pad("🦀", 4);
        assert_eq!(visible_width(&padded), 4);
    }

    #[test]
    fn test_raw_diff_word_wrap_and_marker() {
        let diff = r#"diff --git a/long.rs b/long.rs
index 1111111..2222222 100644
--- a/long.rs
+++ b/long.rs
@@ -1,1 +1,1 @@
-short
+very long line with extensive description that spans well beyond typical width boundaries and needs multiple visual wraps to fit comfortably
"#;
        let contents = [(
            "long.rs".to_string(),
            "very long line with extensive description that spans well beyond typical width boundaries and needs multiple visual wraps to fit comfortably\n".to_string(),
        )]
        .into_iter()
        .collect();

        let out = diffuse_core::pipeline::run_pipeline_full(diff, &contents, None, None, None);
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-raw-wrap.json"),
        );
        state.toggle_raw_diff();
        assert!(state.raw_diff);

        // Find addition row
        let add_row = state
            .flat
            .iter()
            .position(|r| r.row.kind == diffuse_core::parse::RowKind::Add)
            .unwrap();
        state.row_idx = add_row;

        // Content width 50 (scrollbar at 49, text cols = 48)
        let seg_count = state.row_seg_count(state.row_idx, 49);
        assert!(
            seg_count >= 3,
            "long line should produce 3+ visual segments in raw mode"
        );

        let frame = state.render_frame(50, 20);
        assert!(
            frame.contains("\x1b[38;5;120m+\x1b[39m\x1b[23m\x1b[38;5;120mvery long line"),
            "segment 0 must start with gutter line number and + marker followed by green text"
        );
        assert!(
            frame.contains("\x1b[38;5;242m      \x1b[39m\x1b[38;5;120m"),
            "continuation segments must start with 6-space gutter and green foreground"
        );
    }

    #[test]
    fn test_page_down_up_scrolls_one_third_viewport() {
        let diff = r#"diff --git a/many.rs b/many.rs
index 1111111..2222222 100644
--- a/many.rs
+++ b/many.rs
@@ -1,1 +1,40 @@
-old
+line 1
+line 2
+line 3
+line 4
+line 5
+line 6
+line 7
+line 8
+line 9
+line 10
+line 11
+line 12
+line 13
+line 14
+line 15
+line 16
+line 17
+line 18
+line 19
+line 20
+line 21
+line 22
+line 23
+line 24
+line 25
+line 26
+line 27
+line 28
+line 29
+line 30
+line 31
+line 32
+line 33
+line 34
+line 35
+line 36
+line 37
+line 38
+line 39
+line 40
"#;
        let contents = [(
            "many.rs".to_string(),
            (1..=40).map(|i| format!("line {i}\n")).collect::<String>(),
        )]
        .into_iter()
        .collect();

        let out = diffuse_core::pipeline::run_pipeline_full(diff, &contents, None, None, None);
        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-pgdn.json"),
        );
        state.render_frame(80, 20);
        assert_eq!(state.scroll, 0);

        // 1st pg down moves cursor to bottom of current visible page without scrolling
        state.page_down();
        assert_eq!(state.scroll, 0);
        assert!(state.row_idx > 0);

        // 2nd pg down (already at bottom) scrolls 1/3 of vh (5 lines)
        state.page_down();
        assert_eq!(state.scroll, 5);

        // 3rd pg down scrolls another 5 lines
        state.page_down();
        assert_eq!(state.scroll, 10);

        // 1st pg up moves cursor to top of current visible page without scrolling
        state.page_up();
        assert_eq!(state.scroll, 10);

        // 2nd pg up (already at top) scrolls 1/3 of vh (5 lines)
        state.page_up();
        assert_eq!(state.scroll, 5);

        // In raw diff mode: first move cursor to bottom, then scroll
        state.toggle_raw_diff();
        state.render_frame(80, 20);
        state.move_top();
        state.page_down(); // to bottom
        let raw_scroll = state.scroll;
        state.page_down(); // scroll 1/3
        assert_eq!(state.scroll, raw_scroll + 5);
        state.page_up(); // to top
        state.page_up(); // scroll 1/3
        assert_eq!(state.scroll, raw_scroll);
    }

    #[test]
    fn test_raw_diff_exact_input_reproduction() {
        let diff = r#"diff --git a/cache.rs b/cache.rs
index 1111111..2222222 100644
--- a/cache.rs
+++ b/cache.rs
@@ -1,4 +1,4 @@
-fn a() {
-    let x = 1;
-    let y = 2;
+fn a() {
+    let x = 10;
+    let y = 20;
 }
"#;
        let contents = [(
            "cache.rs".to_string(),
            "fn a() {\n    let x = 10;\n    let y = 20;\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        let old_contents = [(
            "cache.rs".to_string(),
            "fn a() {\n    let x = 1;\n    let y = 2;\n}\n".to_string(),
        )]
        .into_iter()
        .collect();

        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            &contents,
            Some(&old_contents),
            None,
            None,
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
            .filter(|r| !r.is_file_header && r.row.kind != diffuse_core::parse::RowKind::Sep)
            .map(|r| r.row.kind)
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
        let contents = [("test.rs".to_string(), lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(&diff, &contents, None, None, None);
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
        let contents = [("test.rs".to_string(), "let short = 2;\n".to_string())]
            .into_iter()
            .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(diff, &contents, None, None, None);
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
        let contents = [(
            "test.rs".to_string(),
            "6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.\n".to_string(),
        )]
        .into_iter()
        .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(diff, &contents, None, None, None);
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
        let contents = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation and\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
        let old_contents = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation.\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
        let out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            &contents,
            Some(&old_contents),
            None,
            None,
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
                r.row.kind == diffuse_core::parse::RowKind::Del && r.row.old_line == Some(82)
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
        assert_eq!(
            current_row.row.old_line,
            Some(82),
            "selected row in visual diff must have old_line 82"
        );
        assert_eq!(
            current_row.row.new_line,
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
        let out = if let Ok(git_res) = diffuse_core::git::load_git_repo(
            std::path::Path::new("/home/daniel/lab/mirage.work"),
            &[],
            &[],
            false,
            6,
        ) {
            diffuse_core::pipeline::run_pipeline_full(
                &git_res.diff_text,
                &git_res.contents,
                Some(&git_res.old_contents),
                None,
                Some("mirage"),
            )
        } else {
            diffuse_core::pipeline::run_pipeline_full(
                diff,
                &std::collections::HashMap::new(),
                None,
                None,
                None,
            )
        };
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
            .position(|r| r.file_idx == 3 && r.row.new_line == Some(115))
            .unwrap_or_else(|| {
                state
                    .flat
                    .iter()
                    .position(|r| r.row.new_line == Some(115))
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
                r.file_idx == state.flat[vis_idx_115].file_idx && r.row.new_line == Some(90)
            })
            .unwrap_or_else(|| panic!("must find line 90"));
        state.scroll = state.visual_geometry(300).0[vis_idx_90];

        let frame_vis = state.render_frame(300, 74);
        let vis_cursor_line = frame_vis
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("115") && l.contains("ResolutionCache"))
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("line 115 not in visual frame"));

        state.toggle_raw_diff();
        let frame_raw = state.render_frame(300, 74);
        let raw_cursor_line = frame_raw
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, l)| l.contains("115") && l.contains("ResolutionCache"))
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
                        r.row.new_line == Some(target_line) || r.row.old_line == Some(target_line)
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
            .position(|r| r.row.new_line == Some(115))
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
    fn test_goto_file_lands_on_banner_with_viewport_margin() {
        let mut lines1 = String::new();
        let mut lines2 = String::new();
        for i in 1..=30 {
            lines1.push_str(&format!("file1 line {i}\n"));
            lines2.push_str(&format!("file2 line {i}\n"));
        }
        let diff = format!(
            "diff --git a/file1.rs b/file1.rs\nindex 000..111 100644\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,30 +1,30 @@\n{lines1}diff --git a/file2.rs b/file2.rs\nindex 222..333 100644\n--- a/file2.rs\n+++ b/file2.rs\n@@ -1,30 +1,30 @@\n{lines2}"
        );
        let contents = [
            ("file1.rs".to_string(), lines1),
            ("file2.rs".to_string(), lines2),
        ]
        .into_iter()
        .collect();

        let mut state = AppState::new(
            diffuse_core::pipeline::run_pipeline_full(&diff, &contents, None, None, None),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.viewport_height = 20;

        // Jump to file 1 (file2.rs)
        state.goto_file_idx(1);
        let header_row_file2 = state.first_row_of_file(1);
        assert_eq!(
            state.row_idx, header_row_file2,
            "goto_file_idx must land directly on the file banner row"
        );
        assert!(
            state.flat[state.row_idx].is_file_header,
            "row must be the file header banner"
        );

        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let target_vis = starts[header_row_file2];
        let expected_margin = state.viewport_margin();
        assert_eq!(
            state.scroll,
            target_vis.saturating_sub(expected_margin),
            "scroll must precede file banner by 1/10 of viewport"
        );
    }

    #[test]
    fn test_goto_line_preserves_viewport_margin_above() {
        let mut lines = String::new();
        for i in 1..=50 {
            lines.push_str(&format!("line {i}\n"));
        }
        let diff = format!(
            "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -1,50 +1,50 @@\n{lines}"
        );
        let contents = [("big.rs".to_string(), lines)].into_iter().collect();

        let mut state = AppState::new(
            diffuse_core::pipeline::run_pipeline_full(&diff, &contents, None, None, None),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.viewport_height = 20;

        state.goto_line(30);
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let target_vis = starts[state.row_idx];
        let margin = state.viewport_margin();
        assert_eq!(
            state.scroll,
            target_vis.saturating_sub(margin),
            "goto_line must precede target line by 1/10 viewport margin"
        );
    }

    #[test]
    fn test_move_bottom_preserves_viewport_margin_below() {
        let mut lines = String::new();
        for i in 1..=50 {
            lines.push_str(&format!("line {i}\n"));
        }
        let diff = format!(
            "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -1,50 +1,50 @@\n{lines}"
        );
        let contents = [("big.rs".to_string(), lines)].into_iter().collect();

        let mut state = AppState::new(
            diffuse_core::pipeline::run_pipeline_full(&diff, &contents, None, None, None),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.viewport_height = 20;

        state.move_bottom();
        assert_eq!(state.row_idx, state.flat.len() - 1);
        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let cursor_vis = starts[state.row_idx];
        let margin = state.viewport_margin();
        let max_rows = state.viewport_height.max(4);
        assert_eq!(
            state.scroll,
            cursor_vis.saturating_sub(max_rows.saturating_sub(1 + margin)),
            "move_bottom must scroll down so bottom line is margin lines above viewport bottom"
        );
    }

    #[test]
    fn test_viewport_margin_rounding_half_up() {
        let mut state = AppState::new(
            diffuse_core::wire::PipelineOutput {
                files: Vec::new(),
                branch: String::new(),
                untracked_skipped: Vec::new(),
            },
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );

        state.viewport_height = 0;
        assert_eq!(state.viewport_margin(), 1, "0 -> min 1");

        state.viewport_height = 4;
        assert_eq!(state.viewport_margin(), 1, "4 -> 1");

        state.viewport_height = 14;
        assert_eq!(state.viewport_margin(), 1, "14 (1.4) -> 1");

        state.viewport_height = 15;
        assert_eq!(state.viewport_margin(), 2, "15 (1.5) -> 2");

        state.viewport_height = 20;
        assert_eq!(state.viewport_margin(), 2, "20 (2.0) -> 2");

        state.viewport_height = 24;
        assert_eq!(state.viewport_margin(), 2, "24 (2.4) -> 2");

        state.viewport_height = 25;
        assert_eq!(state.viewport_margin(), 3, "25 (2.5) -> 3");

        state.viewport_height = 35;
        assert_eq!(state.viewport_margin(), 4, "35 (3.5) -> 4");
    }

    #[test]
    fn test_next_and_prev_file_navigation_centers_banner_with_margin() {
        let mut lines_a = String::new();
        let mut lines_b = String::new();
        let mut lines_c = String::new();
        let mut diff_a = String::new();
        let mut diff_b = String::new();
        let mut diff_c = String::new();
        for i in 1..=30 {
            lines_a.push_str(&format!("a line {i}\n"));
            lines_b.push_str(&format!("b line {i}\n"));
            lines_c.push_str(&format!("c line {i}\n"));
            diff_a.push_str(&format!("+a line {i}\n"));
            diff_b.push_str(&format!("+b line {i}\n"));
            diff_c.push_str(&format!("+c line {i}\n"));
        }
        let diff = format!(
            "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,30 @@\n{diff_b}diff --git a/c.rs b/c.rs\nindex 444..555 100644\n--- a/c.rs\n+++ b/c.rs\n@@ -0,0 +1,30 @@\n{diff_c}"
        );
        let contents = [
            ("a.rs".to_string(), lines_a),
            ("b.rs".to_string(), lines_b),
            ("c.rs".to_string(), lines_c),
        ]
        .into_iter()
        .collect();

        let mut state = AppState::new(
            diffuse_core::pipeline::run_pipeline_full(&diff, &contents, None, None, None),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.viewport_height = 20;

        assert_eq!(state.current_file_idx(), 0, "starts on file 0 (a.rs)");

        // Press 'n' -> next_file()
        state.next_file();
        assert_eq!(state.current_file_idx(), 1, "navigates to file 1 (b.rs)");
        let b_banner = state.first_row_of_file(1);
        assert_eq!(state.row_idx, b_banner, "lands on b.rs banner");
        assert!(state.flat[state.row_idx].is_file_header);

        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let b_vis = starts[b_banner];
        let margin = state.viewport_margin();
        assert_eq!(
            state.scroll,
            b_vis.saturating_sub(margin),
            "next_file must position scroll so banner has 1/10 margin from top"
        );

        // Press 'p' -> prev_file()
        state.prev_file();
        assert_eq!(
            state.current_file_idx(),
            0,
            "navigates back to file 0 (a.rs)"
        );
        let a_banner = state.first_row_of_file(0);
        assert_eq!(state.row_idx, a_banner, "lands on a.rs banner");
        assert_eq!(state.scroll, 0, "a.rs sits at top of screen");
    }
}
