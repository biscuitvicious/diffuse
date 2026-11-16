use crate::{app::AppState, command::TuiCommand};

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
            TuiCommand::JumpOpposingSide => {
                self.goto_opposing_move();
            }
            TuiCommand::ToggleSyntax => self.syntax_on = !self.syntax_on,
            TuiCommand::ToggleWordDiff => self.word_diff = !self.word_diff,
            TuiCommand::ToggleRawDiff => self.toggle_raw_diff(),
            TuiCommand::ToggleOldLineGutter => {
                let cw = self.effective_content_width();
                let active = self.is_old_line_gutter_active(cw);
                self.show_old_line_gutter = Some(!active);
                self.bump_content();
            }
            TuiCommand::ToggleSideBySide => self.toggle_side_by_side(),
            TuiCommand::ToggleHelp => self.show_help = !self.show_help,
            TuiCommand::ToggleTheme => {
                // Resolve the opposite-mode pair from the theme's asset
                // metadata (data-driven; user themes pair by declaration).
                self.theme = self
                    .theme
                    .paired()
                    .unwrap_or(crate::render::DiffuseTheme::dark());
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
                } else if self.is_cursor_on_mod_row() {
                    self.toggle_current_mod_row();
                } else {
                    self.goto_opposing_move();
                }
            }
            TuiCommand::ToggleCollapseFile(Some(fi)) => self.toggle_file_collapse(fi),
            TuiCommand::ToggleExpandAllMods => self.toggle_expand_all_mods(),
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
                        let seg_counts = &geo.seg_counts;

                        for (i, &s) in starts.iter().enumerate() {
                            if target_vis >= s && target_vis < s + counts[i] {
                                let div_lines = if !self.raw_diff
                                    && let Some(hs) = self.hunk_starts.iter().find(|h| h.flat == i)
                                {
                                    self.hunk_div_seg_count(hs.file, hs.hunk, cw)
                                } else {
                                    0
                                };

                                // Clicking the context expansion arrow in the gutter
                                // (columns 0 and 1) expands context and swallows the click.
                                if target_vis >= s + div_lines {
                                    let r_vis = target_vis - s - div_lines;
                                    let item_info = self
                                        .flat
                                        .get(i)
                                        .filter(|item| item.is_diff_row())
                                        .and_then(|item| {
                                            item.hunk_idx().map(|hi| (item.file_idx, hi))
                                        });
                                    if let Some((fi, hi)) = item_info
                                        && r_vis < seg_counts[i]
                                        && col <= 1
                                    {
                                        let s_idx = r_vis;
                                        if let Some(icon) =
                                            self.hunk_expand_icon_for_segment(i, s_idx, seg_counts)
                                        {
                                            if icon.expands_below() {
                                                self.expand_hunk_below(fi, hi);
                                            }
                                            if icon.expands_above() {
                                                self.expand_hunk_above(fi, hi);
                                            }
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

    /// Toggle between split (side-by-side) and unified diff display modes.
    ///
    /// Preserves the active cursor coordinate and screen Y position. In split
    /// mode, modification row expansion is disabled (each modification is
    /// displayed on a single row with pre- and post-image panes side-by-side).
    /// When switching back to unified mode, any expanded modification rows
    /// previously active are restored.
    pub fn toggle_side_by_side(&mut self) {
        let snapshot = self.capture_cursor_snapshot();
        self.side_by_side = !self.side_by_side;
        self.explicit_side_by_side = Some(self.side_by_side);
        self.rebuild_flat_preserving_snapshot(&snapshot);
        self.bump_content();
    }

    /// Adapt side-by-side mode to the given terminal `width` if mode is not
    /// explicitly specified.
    pub fn adapt_side_by_side_to_width(&mut self, width: usize) {
        if self.explicit_side_by_side.is_none() {
            let should_split = self.is_wide_enough_for_side_by_side(width);
            if self.side_by_side != should_split {
                let snapshot = self.capture_cursor_snapshot();
                self.side_by_side = should_split;
                self.rebuild_flat_preserving_snapshot(&snapshot);
                self.bump_content();
            }
        }
    }
}
