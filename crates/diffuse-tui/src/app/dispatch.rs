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
            TuiCommand::MoveUp(n) => {
                for _ in 0..n {
                    self.move_up_wrap();
                }
            }
            TuiCommand::MouseScrollUp(n) => {
                for _ in 0..n {
                    self.move_up();
                }
            }
            TuiCommand::MoveDown(n) => {
                for _ in 0..n {
                    self.move_down_wrap();
                }
            }
            TuiCommand::MouseScrollDown(n) => {
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
                // The scrollbar owns a full-height column: any click at or
                // past the content width seeks (or grabs the thumb),
                // including alongside the header and ribbon rows.
                let track_h = self.screen_height;
                let is_on_scrollbar = col >= cw && row < track_h;
                let in_viewport = row >= header_offset && row < header_offset + vh;
                // Divider arrows stay clickable across the row's content
                // columns via the divider hit-test below.
                let scrollbar_seek = is_on_scrollbar;
                if scrollbar_seek {
                    // Click and drag share one exact mapping from pointer
                    // row to scroll position.
                    //
                    // The scrollbar widget spans the full terminal height
                    // (screen_height), but the viewport it represents is
                    // only `vh` rows tall. The mapping must therefore use
                    // the viewport-relative row and the viewport height so
                    // the pointer position on the track corresponds to the
                    // visible content area, not the full screen.
                    self.is_dragging_scrollbar = true;
                    let (_starts, _counts, _seg, total_vis) =
                        self.visual_geometry(self.last_content_width);
                    let max_scroll = total_vis.saturating_sub(vh);
                    let row_relative = row.saturating_sub(header_offset);
                    let target_vis = Self::scrollbar_scroll_for_row(vh, max_scroll, row_relative);
                    tracing::debug!(row, col, target_vis, max_scroll, "scrollbar mousedown");
                    self.scroll_to_visual(target_vis);
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
                            let div_lines = if let Some(&(div_fi, div_hi)) = div_at.get(&i) {
                                self.hunk_div_seg_count(div_fi, div_hi, cw)
                            } else {
                                0
                            };

                            if target_vis < s + div_lines {
                                if let Some(&(div_fi, div_hi)) = div_at.get(&i)
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
                                        let txt = hunk_divider_text(div_hi, hunk, hidden_gap);

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
                                break;
                            }

                            // Clicking the collapse/expand triangle on a file
                            // header toggles the file — the click is swallowed
                            // and does not select the row.
                            if let Some(item) = self.flat.get(i)
                                && item.is_file_header
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
                                && i == self.last_row_of_file(item.file_idx)
                                && self.hunk_trailing_gap(item.file_idx) > 0
                            {
                                let row_segs = seg_counts[i];
                                let note_lines = self.note_callouts().get(&i).map_or(0, Vec::len);
                                let bottom_start = s + div_lines + row_segs + note_lines;
                                if target_vis >= bottom_start {
                                    let seg_off = target_vis - bottom_start;
                                    let gap = self.hunk_trailing_gap(item.file_idx);
                                    let txt = crate::view::diff_grid::bottom_gap_divider_text(gap);
                                    if wrapped_glyph_hit(&txt, "▲", seg_off, col, cw) {
                                        let last_hi = self.output.files[item.file_idx]
                                            .hunks
                                            .len()
                                            .saturating_sub(1);
                                        self.expand_hunk_below(item.file_idx, last_hi);
                                        break;
                                    }
                                    break;
                                }
                            }

                            let prev_row = self.row_idx;
                            self.row_idx = i;
                            self.vis_line =
                                (target_vis - (s + div_lines)).min(seg_counts[i].saturating_sub(1));

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
                    // Same exact mapping as click-seek: pointer row to
                    // scroll, no special cases.
                    //
                    // The scrollbar widget spans the full terminal height
                    // (screen_height), but the viewport it represents is
                    // only `vh` rows tall. The mapping must therefore use
                    // the viewport-relative row and the viewport height so
                    // the pointer position on the track corresponds to the
                    // visible content area, not the full screen.
                    let (_, _, _, total_vis) = self.visual_geometry(self.last_content_width);
                    let max_scroll = total_vis.saturating_sub(vh);
                    let row_relative = row.saturating_sub(header_offset);
                    let target_vis = Self::scrollbar_scroll_for_row(vh, max_scroll, row_relative);
                    tracing::debug!(
                        row,
                        target_vis,
                        max_scroll,
                        prev_scroll = self.scroll,
                        "scrollbar drag"
                    );
                    self.scroll_to_visual(target_vis);
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
