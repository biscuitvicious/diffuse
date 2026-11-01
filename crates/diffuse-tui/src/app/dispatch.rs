use std::collections::HashMap;

use crate::{
    command::TuiCommand,
    view::{hunk_divider_text, wrapped_glyph_hit},
};

use super::AppState;

impl AppState {
    /// Execute a high-level user command to mutate application state.
    pub fn execute(&mut self, cmd: TuiCommand) {
        // New user input invalidates a viewport pin and any pending auto-scroll
        // on streaming load — except the trailing mouse-up of the very
        // click that created it (crossterm delivers Down and Up, and
        // both trigger a render).
        if !matches!(cmd, TuiCommand::EndMouseDrag) {
            self.scroll_pin = None;
            self.auto_scroll_on_load = None;
        }
        match cmd {
            TuiCommand::Quit => self.should_quit = true,
            TuiCommand::Dismiss => {
                if !self.num_buffer.is_empty() {
                    self.num_buffer.clear();
                } else if self.file_finder.is_some() {
                    self.cancel_file_finder();
                } else if self.note_draft.is_some() || self.note_editor.is_some() {
                    self.note_draft = None;
                    self.note_editor = None;
                    self.editing_note_id = None;
                } else if self.show_help {
                    self.show_help = false;
                } else if self.anchor.is_some() || self.confirmed_sel.is_some() {
                    self.anchor = None;
                    self.confirmed_sel = None;
                    self.sel_pivot = None;
                }
            }
            TuiCommand::MoveUp(n) => {
                for _ in 0..n {
                    self.move_up_wrap();
                }
            }
            TuiCommand::MouseScrollUp { n, row, col } => {
                self.on_mouse_scroll(true, n, row, col);
            }
            TuiCommand::MoveDown(n) => {
                for _ in 0..n {
                    self.move_down_wrap();
                }
            }
            TuiCommand::MouseScrollDown { n, row, col } => {
                self.on_mouse_scroll(false, n, row, col);
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
            TuiCommand::ToggleTheme => {
                self.theme = if self.theme == crate::render::DiffuseTheme::LIGHT {
                    crate::render::DiffuseTheme::DEFAULT
                } else {
                    crate::render::DiffuseTheme::LIGHT
                };
                if let Some(ref mut editor) = self.note_editor {
                    editor.set_highlighter(Some(crate::editor::markdown_highlighter(self.theme)));
                }
                self.bump_content();
            }
            TuiCommand::ToggleStaged => {
                let repo_root = self.repo_root.clone();
                let git_revs = self.git_revs.clone();
                match crate::actions::toggle_staged_action(self, &repo_root, &git_revs) {
                    Ok(()) => self.status_message = None,
                    Err(e) => self.set_status_message(e, false),
                }
            }
            TuiCommand::ForceReload => {
                self.pending_force_reload = true;
            }
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
                    .is_some_and(|fr| fr.is_file_header())
                {
                    self.toggle_current_file_collapse();
                } else if let Some((fi, hi)) = self.is_cursor_on_hunk_divider() {
                    if hi == 0 {
                        let _ = self.expand_hunk_above(fi, 0);
                    } else {
                        let _ = self.expand_hunk_below(fi, hi - 1);
                        let _ = self.expand_hunk_above(fi, hi);
                    }
                } else if let Some(fi) = self.is_cursor_on_bottom_divider() {
                    let last_hi = self.output.files[fi].hunks.len().saturating_sub(1);
                    let _ = self.expand_hunk_below(fi, last_hi);
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
                        // Editor ranges use new-side line numbers (the post-image).
                        let s = sel.start.new_line_num.or(sel.start.old_line_num)?;
                        let e = sel.end.new_line_num.or(sel.end.old_line_num)?;
                        // Only pass a range if the selection is on the new side.
                        sel.start.new_line_num?;
                        Some((s.get(), e.get()))
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
                let git_revs = self.git_revs.clone();
                match crate::actions::stage_action(self, &repo_root, false) {
                    Ok(()) => {
                        self.set_status_message("staged hunk".into(), true);
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_revs);
                    }
                    Err(e) => self.set_status_message(e, false),
                }
            }
            TuiCommand::Unstage => {
                let repo_root = self.repo_root.clone();
                let git_revs = self.git_revs.clone();
                match crate::actions::unstage_action(self, &repo_root, false) {
                    Ok(()) => {
                        self.set_status_message("unstaged hunk".into(), true);
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_revs);
                    }
                    Err(e) => self.set_status_message(e, false),
                }
            }
            TuiCommand::Discard => {
                let repo_root = self.repo_root.clone();
                let git_revs = self.git_revs.clone();
                match crate::actions::discard_action(self, &repo_root, false) {
                    Ok(stash_ref) => {
                        let msg = if let Some(sr) = stash_ref {
                            format!("discarded hunk (stash: {sr})")
                        } else {
                            "discarded hunk".into()
                        };
                        self.set_status_message(msg, true);
                        let _ = crate::actions::reload_diff(self, &repo_root, &git_revs);
                    }
                    Err(e) => self.set_status_message(e, false),
                }
            }
            TuiCommand::StartNoteDraft => self.start_note_draft(),
            TuiCommand::CommitNoteDraft => match self.commit_note_draft() {
                Ok(_) => {}
                Err(e) => self.set_status_message(e.to_string(), false),
            },
            TuiCommand::CancelNoteDraft => {
                self.note_draft = None;
                self.note_editor = None;
                self.editing_note_id = None;
            }
            TuiCommand::DeleteNote => self.delete_note(),
            TuiCommand::CycleNoteStatus => {
                let _ = self.cycle_note_status();
            }
            TuiCommand::ToggleShowResolvedNotes => self.toggle_show_resolved_notes(),
            TuiCommand::ToggleShowNoteSource => self.toggle_show_note_source(),
            TuiCommand::MouseClick { row, col, shift } => {
                let header_offset = self.last_viewport_start;
                let vh = self.viewport_height;
                let cw = self.last_content_width;
                // The scrollbar owns a full-height column: any click at or
                // past the content width seeks (or grabs the thumb),
                // including alongside the header and ribbon rows.
                let track_h = self.screen_height;
                let is_on_scrollbar = col >= cw && row < track_h;
                let in_viewport = row >= header_offset && row < header_offset + vh;
                tracing::debug!(
                    row,
                    col,
                    shift,
                    cw,
                    track_h,
                    is_on_scrollbar,
                    in_viewport,
                    "MouseClick"
                );
                // Divider arrows stay clickable across the row's content
                // columns via the divider hit-test below.
                if is_on_scrollbar {
                    self.on_scrollbar_click(row, col);
                } else {
                    self.is_dragging_scrollbar = false;
                    self.scrollbar_drag_grab = None;
                    if in_viewport {
                        let row_in_viewport = row - header_offset;
                        let target_vis = self.viewport_row_to_visual(row_in_viewport);
                        let geo = self.cached_geometry(self.last_content_width);
                        let starts = &geo.starts;
                        let counts = &geo.counts;
                        let nav_counts = &geo.nav_counts;
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
                                let div_lines = if let Some(&(div_fi, div_hi)) = div_at.get(&i) {
                                    self.hunk_div_seg_count(div_fi, div_hi, cw)
                                } else {
                                    0
                                };

                                if target_vis < s + div_lines
                                    && let Some(&(div_fi, div_hi)) = div_at.get(&i)
                                    && let Some(f) = self.output.files.get(div_fi)
                                    && let Some(hunk) = f.hunks.get(div_hi)
                                {
                                    let hidden_gap = self.hunk_leading_gap(div_fi, div_hi);
                                    if hidden_gap > 0 {
                                        // The divider may wrap across several
                                        // visual segments on narrow screens;
                                        // the
                                        // hit area is the glyph's screen cells
                                        // after wrapping.
                                        let seg_off = target_vis - s;
                                        let txt = hunk_divider_text(
                                            crate::render::Badge {
                                                idx: div_hi,
                                                total: f.hunks.len(),
                                            },
                                            hunk,
                                            hidden_gap,
                                        );

                                        let hit = |pat: &str| {
                                            wrapped_glyph_hit(&txt, pat, seg_off, col, cw)
                                        };

                                        let mut handled = true;
                                        if div_hi == 0 && hit("▼") {
                                            self.expand_hunk_above(div_fi, 0);
                                        } else if div_hi > 0 && hit("⬍") {
                                            self.expand_hunk_both(div_fi, div_hi - 1);
                                        } else if div_hi > 0 && hit("▲") {
                                            self.expand_hunk_below(div_fi, div_hi - 1);
                                        } else if div_hi > 0 && hit("▼") {
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
                                    && item.is_file_header()
                                {
                                    let is_binary = self
                                        .output
                                        .files
                                        .get(item.file_idx)
                                        .is_some_and(|f| f.is_binary);
                                    if !is_binary {
                                        let seg_off = target_vis - s;
                                        let eff_col = seg_off * cw + col;
                                        let c = crate::view::diff_grid::FILE_HEADER_INDICATOR_COL;
                                        let hit_indicator =
                                            eff_col >= c.saturating_sub(1) && eff_col <= c + 1;
                                        if hit_indicator {
                                            let fi = item.file_idx;
                                            self.execute(TuiCommand::ToggleCollapseFile(Some(fi)));
                                            break;
                                        }
                                    }
                                }

                                if let Some(item) = self.flat.get(i)
                                    && !self.raw_diff
                                    && item.is_bottom_gap_divider()
                                {
                                    let seg_off = target_vis - s;
                                    let gap = self.hunk_trailing_gap(item.file_idx);
                                    let txt = crate::view::diff_grid::bottom_gap_divider_text(gap);
                                    if wrapped_glyph_hit(&txt, "▲", seg_off, col, cw) {
                                        let last_hi = item.hunk_idx().unwrap_or(0);
                                        self.expand_hunk_below(item.file_idx, last_hi);
                                        break;
                                    }
                                }

                                let prev_row = self.row_idx;
                                self.row_idx = i;
                                self.vis_line =
                                    (target_vis - s).min(nav_counts[i].saturating_sub(1));

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
            }
            TuiCommand::MouseDrag { row, col: _, shift } => {
                let header_offset = self.last_viewport_start;
                let vh = self.viewport_height;
                tracing::debug!(
                    row,
                    shift,
                    is_dragging_scrollbar = self.is_dragging_scrollbar,
                    "MouseDrag"
                );
                if self.is_dragging_scrollbar {
                    self.on_scrollbar_drag(row);
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
                    let row_in_viewport = row.saturating_sub(header_offset);
                    let target_vis = self.viewport_row_to_visual(row_in_viewport);
                    let (starts, counts, _seg_counts, _total_vis) =
                        self.cached_geometry_tuple(self.last_content_width);
                    for (i, &s) in starts.iter().enumerate() {
                        if target_vis >= s && target_vis < s + counts[i] {
                            self.row_idx = i;
                            self.vis_line = 0;
                            break;
                        }
                    }
                }
            }
            TuiCommand::EndMouseDrag => self.on_mouse_drag_end(),
        }
    }

    fn on_mouse_scroll(&mut self, up: bool, n: usize, row: usize, col: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        tracing::debug!(
            up,
            row,
            col,
            n,
            is_dragging_scrollbar = self.is_dragging_scrollbar,
            "on_mouse_scroll"
        );
        let (_, _, _, total_vis) = self.cached_geometry_tuple(cw);
        let has_scrollbar = !self.raw_diff && total_vis > vh && vh > 2;
        if has_scrollbar && col >= cw.saturating_sub(1) && row < track_h {
            let step = Self::scrollbar_step(track_h, vh, total_vis, n);
            if up {
                self.scroll_to_visual(self.scroll + step);
            } else {
                self.scroll_to_visual(self.scroll.saturating_sub(step));
            }
        } else if up {
            self.scroll_to_visual(self.scroll.saturating_sub(n));
        } else {
            self.scroll_to_visual(self.scroll + n);
        }
    }

    fn on_scrollbar_click(&mut self, row: usize, col: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        self.is_dragging_scrollbar = true;
        let (_starts, counts, _seg, total_vis) = self.cached_geometry_tuple(cw);
        let header_vis = counts.first().copied().unwrap_or(1);
        if let Some((thumb_off, thumb_size)) =
            Self::scrollbar_thumb_for(track_h, vh, total_vis, self.scroll, header_vis)
        {
            if row >= thumb_off && row < thumb_off + thumb_size {
                // Clicked directly on the thumb: preserve current scroll position
                // and record the relative grab offset within the thumb.
                self.scrollbar_drag_grab = Some(row - thumb_off);
                tracing::debug!(
                    row,
                    col,
                    thumb_off,
                    thumb_size,
                    grab = row - thumb_off,
                    "scrollbar thumb grab (no snap)"
                );
            } else {
                // Clicked on the track outside the thumb: snap immediately.
                let target_vis =
                    Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row);
                tracing::debug!(row, col, target_vis, "scrollbar track seek");
                self.scroll_to_visual(target_vis);
                if let Some((new_off, new_size)) =
                    Self::scrollbar_thumb_for(track_h, vh, total_vis, target_vis, header_vis)
                {
                    let grab = row.saturating_sub(new_off).min(new_size.saturating_sub(1));
                    self.scrollbar_drag_grab = Some(grab);
                } else {
                    self.scrollbar_drag_grab = None;
                }
            }
        } else {
            let target_vis =
                Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row);
            tracing::debug!(row, col, target_vis, "scrollbar mousedown");
            self.scroll_to_visual(target_vis);
            self.scrollbar_drag_grab = None;
        }
    }

    fn on_scrollbar_drag(&mut self, row: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        let (_, counts, _, total_vis) = self.cached_geometry_tuple(cw);
        let header_vis = counts.first().copied().unwrap_or(1);
        let target_vis = if let Some(grab) = self.scrollbar_drag_grab {
            let target_offset = row.saturating_sub(grab);
            Self::scrollbar_scroll_for_offset(track_h, vh, total_vis, header_vis, target_offset)
        } else {
            Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row)
        };
        tracing::debug!(
            row,
            target_vis,
            grab = self.scrollbar_drag_grab,
            prev_scroll = self.scroll,
            "scrollbar drag"
        );
        self.scroll_to_visual(target_vis);
    }

    fn on_mouse_drag_end(&mut self) {
        tracing::debug!(
            is_dragging_scrollbar = self.is_dragging_scrollbar,
            "EndMouseDrag"
        );
        self.is_dragging_scrollbar = false;
        self.scrollbar_drag_grab = None;
        if let Some(anchor) = self.anchor {
            if anchor != self.row_idx {
                self.confirmed_sel = self.compute_selection();
            }
            self.anchor = None;
        }
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
