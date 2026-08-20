//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use crate::{
    bin_pipeline::{OutputFile, OutputRow, PipelineOutput},
    notes::{NotesDB, Side},
    render::{
        DiffuseTheme, S_ADD, S_DEL, S_MOD, display_text, render_raw_row, render_row_segment,
        styled_prefix,
    },
};
use std::{collections::HashMap, path::PathBuf};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file_idx: usize,
    pub start: u32,
    pub end: u32,
    pub side_is_new: bool,
}

/// The first flat row of a hunk's content region (the jump target for hunk
/// navigation, mirroring the reference `hunkStarts`).
#[derive(Clone, Copy, Debug)]
pub struct HunkStart {
    pub flat: usize,
    pub file: usize,
    pub hunk: usize,
}

/// A hunk's allocated context envelope (above/below counts),
/// used for the divider `ctx N▴ M▾` counters.
#[derive(Clone, Copy, Debug)]
pub struct HunkMeta {
    pub above: usize,
    pub below: usize,
}

pub struct FlatRow {
    pub file_idx: usize,
    pub hunk_idx: usize,
    pub is_file_header: bool,
    pub row: OutputRow,
}

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
    pub pending_editor: Option<crate::tui::editor::EditorRequest>,
    pub file_finder: Option<crate::tui::finder::FileFinderState>,
    pub collapsed_files: std::collections::HashSet<usize>,
    pub last_content_width: usize,
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
            notes: crate::notes::load_notes(&notes_path),
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
    /// hunk an `@@` header row and its rows with mod/move pairs un-collapsed
    /// into separate `-`/`+` lines. Hunk navigation jumps to the `@@` rows.
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
            for (hi, h) in f.hunks.iter().enumerate() {
                flat.push(FlatRow {
                    file_idx: fi,
                    hunk_idx: hi,
                    is_file_header: false,
                    row: OutputRow {
                        kind: crate::parse::RowKind::Sep,
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
                    for r in Self::raw_row_expansion(r) {
                        flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            is_file_header: false,
                            row: r,
                        });
                    }
                }
            }
        }
        (flat, hunk_starts)
    }

    /// Split a collapsed hunk row into raw unified-diff lines: a mod or move
    /// becomes a `-` del line plus a `+` add line; everything else passes
    /// through.
    fn raw_row_expansion(r: &OutputRow) -> Vec<OutputRow> {
        let del_line = |old_line: Option<u32>, text: String| OutputRow {
            kind: crate::parse::RowKind::Del,
            new_line: None,
            old_line,
            text,
            del_text: None,
            spans: Default::default(),
            runs: Vec::new(),
            wholesale: false,
            moved: false,
        };
        let add_line = |new_line: Option<u32>, text: String| OutputRow {
            kind: crate::parse::RowKind::Add,
            new_line,
            old_line: None,
            text,
            del_text: None,
            spans: Default::default(),
            runs: Vec::new(),
            wholesale: false,
            moved: false,
        };
        match r.kind {
            crate::parse::RowKind::Mod => {
                let old = r.del_text.clone().unwrap_or_default();
                vec![
                    del_line(r.old_line, old),
                    add_line(r.new_line, r.text.clone()),
                ]
            }
            crate::parse::RowKind::Add if r.moved => {
                let old = r.del_text.clone().unwrap_or_default();
                vec![del_line(None, old), add_line(r.new_line, r.text.clone())]
            }
            _ => vec![r.clone()],
        }
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
                kind: crate::parse::RowKind::File,
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

    /// Locate each hunk's first content row in the flat sequence, matching by
    /// (`kind`, `new_line`, `old_line`, `text`) — the visible rows and the
    /// hunk rows are separate `OutputRow` conversions, so value equality is
    /// not stable.
    fn build_hunk_starts(output: &PipelineOutput, flat: &[FlatRow]) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            for (hi, h) in f.hunks.iter().enumerate() {
                let Some(first) = h.rows.first() else {
                    continue;
                };
                let Some(idx) = flat.iter().position(|fr| {
                    !fr.is_file_header
                        && fr.file_idx == fi
                        && fr.row.kind == first.kind
                        && fr.row.new_line == first.new_line
                        && fr.row.old_line == first.old_line
                        && fr.row.text == first.text
                }) else {
                    continue;
                };
                starts.push(HunkStart {
                    flat: idx,
                    file: fi,
                    hunk: hi,
                });
            }
        }
        starts
    }

    /// Per-file per-hunk context allocation. The wire carries no ctx counts,
    /// so recompute them over the hunks the same way `f.rows` was built
    /// (forced 6/6 above/below).
    fn build_hunk_meta(output: &PipelineOutput) -> Vec<Vec<HunkMeta>> {
        use crate::visible::allocate_context;
        output
            .files
            .iter()
            .map(|f| {
                let mut hunks: Vec<crate::parse::Hunk> = f
                    .hunks
                    .iter()
                    .map(crate::bin_pipeline::OutputHunk::reconstruct)
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
        let created_at = crate::notes::now_millis();
        let note = crate::notes::ReviewNote {
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
        crate::notes::add_note(&mut self.notes, &self.repo, note);
        let _ = crate::notes::save_notes(&self.notes_path, &self.notes);
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
        let new_removed =
            crate::notes::remove_notes(&mut self.notes, &self.repo, &file, Side::New, start, end);
        let old_removed =
            crate::notes::remove_notes(&mut self.notes, &self.repo, &file, Side::Old, start, end);
        let removed = new_removed + old_removed;
        let _ = crate::notes::save_notes(&self.notes_path, &self.notes);
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

    /// Jump to the line in the current file closest to `line_num`.
    pub fn goto_line(&mut self, line_num: u32) {
        let cur_fi = self.current_file_idx();
        let mut best: Option<(usize, u32)> = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != cur_fi {
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
            self.row_idx = idx;
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
        let notes = crate::notes::notes_for_file(&self.notes, &self.repo, &f.path);
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
            for r in crate::notes::note_runs(&n.text) {
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
        let counts = &f.counts;
        let note_count = f.notes.len();
        let note_s = format!(
            "{note_count} note{}",
            if note_count == 1 { "" } else { "s" }
        );
        let counts_str = format!(
            "+{}{}{}{} · {}",
            counts.adds,
            if counts.dels > 0 {
                format!(" −{}", counts.dels)
            } else {
                String::new()
            },
            if counts.mods > 0 {
                format!(" ~{}", counts.mods)
            } else {
                String::new()
            },
            if counts.movs > 0 {
                format!(" ≈{} moved", counts.movs)
            } else {
                String::new()
            },
            note_s
        );

        let head_raw = format!(
            " Review: {} · {} ({}) · {}",
            self.repo, f.path, f.status, counts_str
        );
        let head_str = truncate_pad(&head_raw, width);
        lines.push(format!(
            "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{head_str}\x1b[0m"
        ));

        // Secondary status bar
        let total_files = self.output.files.len();
        let cur_hunk = self.current_hunk().unwrap_or(0);
        let total_hunks = f.hunks.len().max(1);
        let mode_txt = if self.raw_diff {
            " · raw diff".to_string()
        } else {
            String::new()
        };
        let go_txt = if !self.num_buffer.is_empty() {
            format!(" · go {}", self.num_buffer)
        } else {
            String::new()
        };
        let col_txt = if self.col_offset > 0 {
            format!(" · col {}+", self.col_offset)
        } else {
            String::new()
        };
        let branch_txt = if !self.output.branch.is_empty() {
            format!(" · branch: {}", self.output.branch)
        } else {
            String::new()
        };
        let status_raw = format!(
            " {}/{} files · hunk {}/{}{}{}{}{}",
            cur_fi + 1,
            total_files,
            cur_hunk + 1,
            total_hunks,
            mode_txt,
            col_txt,
            go_txt,
            branch_txt
        );
        let status_str = truncate_pad(&status_raw, width);
        lines.push(format!("\x1b[38;5;244m{status_str}\x1b[39m"));

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
        let est_content_width = width.saturating_sub(1);
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
                self.row_seg_count(idx, est_content_width)
            };
            let note_lines = callouts.get(&idx).map_or(0, std::vec::Vec::len);
            let item_total = div_lines + header_lines + row_segs + note_lines;
            counts.push(item_total);
            seg_counts.push(row_segs.max(1));
            total_vis_lines += item_total;
        }

        let has_scrollbar = total_vis_lines > vh && vh > 2;
        let content_width = if has_scrollbar {
            width.saturating_sub(1)
        } else {
            width
        };
        self.last_content_width = content_width;

        if !has_scrollbar && content_width != est_content_width {
            total_vis_lines = 0;
            starts.clear();
            counts.clear();
            seg_counts.clear();
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
                    let span = format!(
                        "−{},{} +{},{}",
                        h.old_start, h.old_count, h.new_start, h.new_count
                    );
                    let txt = format!(
                        "───── H{} · {span} · ctx {}▴ {}▾ ─────",
                        div_hi + 1,
                        ctx.0,
                        ctx.1
                    );
                    let txt_str = truncate_pad(&txt, content_width);
                    let styled = if is_cur {
                        format!("\x1b[48;5;236m\x1b[1m\x1b[38;5;231m{txt_str}\x1b[0m")
                    } else {
                        format!("\x1b[38;5;240m{txt_str}\x1b[39m")
                    };
                    lines.push(styled);
                }
                vis_i += 1;
            }

            if item.is_file_header {
                if vis_i >= self.scroll && vis_i < self.scroll + max_rows {
                    let file_obj = &self.output.files[item.file_idx];
                    let f_counts = &file_obj.counts;
                    let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                    let indicator = if is_collapsed { "▸" } else { "▾" };
                    let collapse_tag = if is_collapsed { " [collapsed]" } else { "" };
                    let bar = format!(
                        "─── {} {} ({}) +{} -{} ~{}{} ───",
                        indicator,
                        file_obj.path,
                        file_obj.status,
                        f_counts.adds,
                        f_counts.dels,
                        f_counts.mods,
                        collapse_tag
                    );
                    let bar_str = truncate_pad(&bar, content_width);
                    let is_selected = idx == self.row_idx;
                    let styled_bar = if is_selected {
                        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{bar_str}\x1b[0m")
                    } else if in_band {
                        format!("\x1b[48;5;240m\x1b[1m\x1b[38;5;231m{bar_str}\x1b[0m")
                    } else {
                        format!("\x1b[38;5;244m{bar_str}\x1b[39m")
                    };
                    lines.push(styled_bar);
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
                        let mut rendered_row = if self.raw_diff {
                            let text_cols = content_width.max(1);
                            let seg_start = s_idx * text_cols;
                            let graphemes: Vec<&str> = r.text.graphemes(true).collect();
                            let piece = if seg_start < graphemes.len() {
                                let seg_end = (seg_start + text_cols).min(graphemes.len());
                                graphemes[seg_start..seg_end].concat()
                            } else {
                                String::new()
                            };
                            render_raw_row(r.kind, &piece, is_selected, in_band && !is_selected)
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
                        if is_selected {
                            let sel_bg = match r.kind {
                                crate::parse::RowKind::Del => S_DEL.cursor_bg(),
                                crate::parse::RowKind::Add if !r.moved => S_ADD.cursor_bg(),
                                _ => S_MOD.cursor_bg(),
                            };
                            rendered_row.push_str(&sel_bg);
                        } else if in_band {
                            rendered_row.push_str("\x1b[48;5;240m");
                        }
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

        // Visual Scrollbar overlay on viewport lines
        if has_scrollbar {
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

            for row_i in 0..vh {
                let line_idx = viewport_start_line + row_i;
                if line_idx < lines.len() {
                    let sb_char = if row_i >= thumb_offset && row_i < thumb_offset + thumb_size {
                        "\x1b[38;5;248m█\x1b[0m"
                    } else {
                        "\x1b[38;5;236m│\x1b[0m"
                    };
                    let vis_w = visible_width(&lines[line_idx]);
                    if vis_w < content_width {
                        lines[line_idx].push_str(&" ".repeat(content_width - vis_w));
                    }
                    lines[line_idx].push_str("\x1b[0m");
                    lines[line_idx].push_str(sb_char);
                }
            }
        }

        lines.push(format!(
            "\x1b[38;5;244m {}\x1b[39m",
            "─".repeat(width.saturating_sub(2).min(60))
        ));
        let ribbon = truncate_pad(
            " [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help",
            width,
        );
        lines.push(format!("\x1b[38;5;244m{ribbon}\x1b[39m"));

        // Help modal overlays the frame when toggled on.
        if self.show_help {
            let modal = Self::render_help_modal(width);
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
            let modal = self.render_finder_modal(width, finder);
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
            out.push_str("\x1b[K\x1b[0m");
        }
        out
    }

    fn render_finder_modal(
        &self,
        width: usize,
        finder: &crate::tui::finder::FileFinderState,
    ) -> Vec<String> {
        let w = width.saturating_sub(4).clamp(36, 76);
        let mut lines = Vec::new();

        let prompt = format!(" @ {}█", finder.query);
        lines.push(format!(
            "\x1b[48;5;238m\x1b[1m\x1b[38;5;231m {:<w$}\x1b[0m",
            prompt,
            w = w.saturating_sub(1)
        ));

        let header_info = format!(
            " FIND FILE ({}/{}) · ↑/↓ nav · Enter jump · Esc close",
            if finder.matches.is_empty() {
                0
            } else {
                finder.selected_idx + 1
            },
            finder.matches.len()
        );
        lines.push(format!(
            "\x1b[48;5;236m\x1b[38;5;244m {:<w$}\x1b[0m",
            header_info,
            w = w.saturating_sub(1)
        ));

        if finder.matches.is_empty() {
            lines.push(format!(
                "\x1b[48;5;235m\x1b[38;5;240m {:<w$}\x1b[0m",
                "  No matching files in diff",
                w = w.saturating_sub(1)
            ));
        } else {
            let max_visible = 8;
            let total = finder.matches.len();
            let start = if total <= max_visible || finder.selected_idx < max_visible / 2 {
                0
            } else if finder.selected_idx + max_visible / 2 >= total {
                total - max_visible
            } else {
                finder.selected_idx - max_visible / 2
            };
            let end = (start + max_visible).min(total);

            for (i, m) in finder.matches[start..end].iter().enumerate() {
                let actual_idx = start + i;
                let is_sel = actual_idx == finder.selected_idx;
                let f = &self.output.files[m.file_idx];
                let prefix = if is_sel { "▶ " } else { "  " };
                let stats = format!("+{} -{} ~{}", f.counts.adds, f.counts.dels, f.counts.mods);

                let path_display = Self::highlight_path_matches(&m.path, &m.match_indices, is_sel);
                let bg_code = if is_sel {
                    "\x1b[48;5;239m"
                } else {
                    "\x1b[48;5;235m"
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

                lines.push(format!(
                    "{bg_code}{prefix_color}{prefix}\x1b[0m{bg_code}{path_display} {stats_color}{stats:<8}\x1b[0m"
                ));
            }
        }

        lines
    }

    fn highlight_path_matches(path: &str, indices: &[usize], selected: bool) -> String {
        let mut out = String::new();
        let default_color = if selected {
            "\x1b[1m\x1b[38;5;231m"
        } else {
            "\x1b[38;5;250m"
        };
        let match_color = "\x1b[1m\x1b[38;5;51m";

        for (i, ch) in path.char_indices() {
            if indices.contains(&i) {
                out.push_str(match_color);
            } else {
                out.push_str(default_color);
            }
            out.push(ch);
        }
        out.push_str("\x1b[0m");
        out
    }

    pub fn open_file_finder(&mut self) {
        let mut finder =
            crate::tui::finder::FileFinderState::new(&self.output.files, self.row_idx, self.scroll);
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
        let target = self.next_coord_row(first);
        self.row_idx = target;
    }

    /// Render the `?` key-bindings modal as full-width overlay lines.
    fn render_help_modal(width: usize) -> Vec<String> {
        let entries: &[(&str, &str)] = &[
            ("j / k / ↑ / ↓", "move"),
            ("J / K / } / {", "prev / next hunk"),
            ("Space · PgDn/PgUp · Ctrl+d f u b", "page up/down"),
            ("g / G / Home / End", "top / bottom"),
            ("n / p / ] / [", "next / prev file"),
            ("@", "find file in diff"),
            ("h / l / ← / →", "horizontal scroll"),
            ("w", "toggle word diff"),
            ("y", "toggle syntax highlighting"),
            ("R", "toggle raw unified-diff view"),
            ("m", "new note (ctrl+enter save, esc cancel)"),
            ("?", "toggle help"),
            ("q / Ctrl+c", "quit"),
        ];
        let mut boxed = vec![String::from(" KEY BINDINGS ")];
        for (key, desc) in entries {
            boxed.push(format!("   {key:<32} {desc}"));
        }
        boxed.push(String::from("   ? or Esc to close"));
        let w = width.saturating_sub(2).min(64);
        boxed
            .into_iter()
            .map(|l| format!("\x1b[48;5;236m\x1b[38;5;255m{:<w$}\x1b[0m", l))
            .collect()
    }

    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        if let Some(item) = self.flat.get(idx) {
            if item.is_file_header {
                return 1;
            }
            let r = &item.row;
            if self.raw_diff {
                let cols = content_width.max(1);
                let len = r.text.graphemes(true).count();
                if len == 0 { 1 } else { len.div_ceil(cols) }
            } else {
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
                let len = disp.graphemes(true).count();
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
        self.row_idx = target;
        self.vis_line = 0;
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
        self.row_idx = target;
        self.vis_line = 0;
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
                && nl >= new_lo && nl < new_hi {
                    set[i] = true;
                    continue;
                }
            if let Some(ol) = r.old_line
                && ol >= old_lo && ol < old_hi {
                    set[i] = true;
                }
        }
        set
    }

    /// Switch between the visual (reflowed) and raw unified-diff
    /// representations, anchoring the cursor at its exact line: capture
    /// `(file, coord, side)`, rebuild the other view, then jump to the
    /// equivalent row (same coordinate, preferring the same side).
    pub fn toggle_raw_diff(&mut self) {
        let (file, coord, prefer_new) = {
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let r = &row.row;
            (
                row.file_idx,
                Self::row_coord(r).unwrap_or(0),
                r.new_line.is_some(),
            )
        };
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.raw_diff = !self.raw_diff;
        self.anchor = None;
        self.goto_coord(file, coord, prefer_new);
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
            if self.flat[i].file_idx > cur_fi {
                self.row_idx = i;
                return;
            }
        }
    }

    pub fn prev_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in (0..self.row_idx).rev() {
            if self.flat[i].file_idx < cur_fi {
                // Find the first row of that previous file
                let prev_fi = self.flat[i].file_idx;
                let first_row = self
                    .flat
                    .iter()
                    .position(|r| r.file_idx == prev_fi)
                    .unwrap_or(i);
                self.row_idx = first_row;
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
        let max_idx = self.flat.len() - 1;
        let bottom_of_viewport =
            (self.scroll + self.viewport_height.saturating_sub(1)).min(max_idx);

        // If cursor is not at the bottom of the current visible page, jump to bottom
        if self.row_idx < bottom_of_viewport {
            self.row_idx = bottom_of_viewport;
        } else {
            // Otherwise, scroll down 1/3 of the viewport height with half-up rounding
            let step = ((self.viewport_height * 2 + 3) / 6).max(1);
            self.row_idx = (self.row_idx + step).min(max_idx);
        }
        self.vis_line = 0;
    }

    pub fn page_up(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let top_of_viewport = self.scroll;

        // If cursor is not at the top of the current visible page, jump to top
        if self.row_idx > top_of_viewport {
            self.row_idx = top_of_viewport;
        } else {
            // Otherwise, scroll up 1/3 of the viewport height with half-up rounding
            let step = ((self.viewport_height * 2 + 3) / 6).max(1);
            self.row_idx = self.row_idx.saturating_sub(step);
        }
        self.vis_line = 0;
    }

    pub fn move_top(&mut self) {
        self.row_idx = 0;
        self.vis_line = 0;
        self.scroll = 0;
    }

    pub fn move_bottom(&mut self) {
        if !self.flat.is_empty() {
            self.row_idx = self.flat.len() - 1;
            self.vis_line = 0;
        }
    }
}

/// Calculate the visible column width of a string by ignoring ANSI escape
/// sequences.
fn visible_width(s: &str) -> usize {
    let mut in_escape = false;
    let mut count = 0;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            count += 1;
        }
    }
    count
}

/// Truncate or right-pad a string with spaces to exactly `target_width`
/// graphemes.
fn truncate_pad(s: &str, target_width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    if graphemes.len() >= target_width {
        graphemes[..target_width].concat()
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target_width - graphemes.len()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let out = crate::bin_pipeline::run_pipeline_full(
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
        crate::bin_pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None)
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
        crate::bin_pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None)
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

        let notes = crate::notes::notes_for_file(&state.notes, "test_repo", &file);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "check this **carefully**");
        assert!(
            notes[0].start > 0,
            "note must anchor to a real row coordinate"
        );

        let on_disk = crate::notes::load_notes(&notes_path);
        assert_eq!(
            crate::notes::notes_for_file(&on_disk, "test_repo", &file).len(),
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
            crate::notes::notes_for_file(&state.notes, "test_repo", &file).is_empty(),
            "delete must remove the note from the DB"
        );
        let on_disk2 = crate::notes::load_notes(&notes_path);
        assert!(
            crate::notes::notes_for_file(&on_disk2, "test_repo", &file).is_empty(),
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
        assert_eq!(state.current_hunk(), Some(0));

        // From above hunk 1, next jumps to hunk 1's start (reference
        // gotoHunk = first hunk start after the cursor).
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
            .find(|fr| fr.row.kind == crate::parse::RowKind::Del);
        assert!(
            del.is_some() && del.unwrap().row.text.contains("old comment"),
            "the mod pair must un-collapse into a - line"
        );
        let add = state.flat.iter().find(|fr| {
            fr.row.kind == crate::parse::RowKind::Add && fr.row.text.contains("new comment")
        });
        assert!(add.is_some(), "the mod pair must un-collapse into a + line");

        let sep = state
            .flat
            .iter()
            .find(|fr| fr.row.kind == crate::parse::RowKind::Sep);
        assert_eq!(
            sep.map(|fr| fr.row.text.as_str()),
            Some("@@ -1025,6 +1025,8 @@")
        );

        let frame = state.render_frame(120, 40);
        assert!(frame.contains("@@ -1025,6 +1025,8 @@"), "raw @@ header");
        assert!(
            frame.contains("\x1b[38;5;203m- "),
            "raw deleted lines render red"
        );
        assert!(
            frame.contains("\x1b[38;5;120m+ "),
            "raw added lines render green"
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
        assert_eq!(r.kind, crate::parse::RowKind::Mod);
        assert_eq!(r.new_line, Some(1026));

        state.toggle_raw_diff();
        let r = &state.flat[state.row_idx].row;
        assert_eq!(
            r.kind,
            crate::parse::RowKind::Add,
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

        let mut out = crate::bin_pipeline::run_pipeline_full(diff, &contents, None, None, None);
        // Add an untracked file to output
        out.files.push(crate::bin_pipeline::OutputFile {
            path: "untracked.rs".to_string(),
            status: "untracked".to_string(),
            new_line_count: 1,
            old_line_count: 0,
            new_content: vec!["fn new_untracked() {}".to_string()],
            old_content: Vec::new(),
            hunks: vec![crate::bin_pipeline::OutputHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                rows: vec![crate::bin_pipeline::OutputRow {
                    kind: crate::parse::RowKind::Add,
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
            counts: crate::counts::FileCounts {
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
}
