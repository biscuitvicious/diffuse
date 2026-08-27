use std::collections::HashMap;

use crate::{command::TuiCommand, view::hunk_divider_text};

use super::AppState;

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
