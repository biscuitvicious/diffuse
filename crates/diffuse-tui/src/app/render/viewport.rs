//! Diff grid viewport visual line synthesis, sticky header computation, and
//! scrollbar layout.

use std::{collections::HashMap, time::Instant};

use ratatui::text::Line;

use super::buffer::escape_ansi_for_diagnostic;
use crate::{
    app::{AppState, notes::NoteRowFocus},
    core::parse::RowKind,
    render::{
        continuation_prefix_spans_with_mod_side_and_widths, raw_row_spans, row_segment_spans,
        side_by_side_row_segment_spans, slice_by_visual_width, span::StyledSpan, split_pane_widths,
        styled_prefix_spans_with_widths, styled_split_left_continuation_prefix_spans,
        styled_split_left_prefix_spans, styled_split_right_continuation_prefix_spans,
        styled_split_right_prefix_spans,
    },
    types::{FlatRowKind, StickyHeaderInfo, StickyHunkInfo},
    view::{
        diff_grid::{
            bottom_gap_divider_segment_line, file_header_segment_line, hunk_divider_segment_line,
        },
        span::fit_line,
    },
};

impl AppState {
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

    /// Calculate sticky file and hunk header metadata for the current scroll
    /// position and content width.
    pub(crate) fn sticky_header_info_from_layout(
        &self,
        starts: &[usize],
        seg_counts: &[usize],
        content_width: usize,
    ) -> Option<StickyHeaderInfo> {
        if self.raw_diff || self.flat.is_empty() || self.output.files.is_empty() {
            return None;
        }
        let (active_fi, banner_row) = (0..self.output.files.len())
            .rfind(|&fi| {
                if let Some(r) = self.file_header_row_idx(fi) {
                    starts.get(r).is_some_and(|&s| s <= self.scroll)
                } else {
                    false
                }
            })
            .and_then(|fi| {
                let r = self.file_header_row_idx(fi)?;
                Some((fi, r))
            })?;

        let banner_vis = *starts.get(banner_row)?;
        let banner_len = *seg_counts.get(banner_row).unwrap_or(&1);

        if self.scroll <= banner_vis {
            return None;
        }

        let next_banner_vis = (active_fi + 1..self.output.files.len()).find_map(|fi| {
            let r = self.file_header_row_idx(fi)?;
            starts.get(r).copied()
        });

        let next_screen_row =
            next_banner_vis.map_or(usize::MAX, |next_vis| next_vis.saturating_sub(self.scroll));
        let sticky_lines_count = banner_len.min(next_screen_row);
        if sticky_lines_count == 0 {
            return None;
        }
        let sticky_start_seg = banner_len.saturating_sub(sticky_lines_count);

        let hunk = if self.collapsed_files.contains(&active_fi)
            || self.output.files[active_fi].hunks.is_empty()
        {
            None
        } else {
            let file_hunks: Vec<&crate::types::HunkStart> = self
                .hunk_starts
                .iter()
                .filter(|hs| hs.file == active_fi)
                .collect();
            let active_hunk_opt = file_hunks
                .iter()
                .rfind(|hs| {
                    starts
                        .get(hs.flat)
                        .is_some_and(|&hv| hv <= self.scroll + sticky_lines_count)
                })
                .copied();

            if let Some(cur_hs) = active_hunk_opt {
                let hunk_vis = *starts.get(cur_hs.flat)?;
                let hunk_len = self.hunk_div_seg_count(active_fi, cur_hs.hunk, content_width);
                let next_boundary_vis = if let Some(next_hs) =
                    file_hunks.iter().find(|hs| hs.hunk == cur_hs.hunk + 1)
                {
                    starts.get(next_hs.flat).copied()
                } else {
                    next_banner_vis
                };
                let next_boundary_screen_row =
                    next_boundary_vis.map_or(usize::MAX, |nbv| nbv.saturating_sub(self.scroll));
                let hunk_avail_rows = next_boundary_screen_row.saturating_sub(sticky_lines_count);
                let sticky_hunk_lines_count = hunk_len.min(hunk_avail_rows);
                if sticky_hunk_lines_count > 0 {
                    let sticky_start_seg = hunk_len.saturating_sub(sticky_hunk_lines_count);
                    Some(StickyHunkInfo {
                        file_idx: active_fi,
                        hunk_idx: cur_hs.hunk,
                        flat_row: cur_hs.flat,
                        hunk_vis,
                        hunk_len,
                        sticky_lines_count: sticky_hunk_lines_count,
                        sticky_start_seg,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        Some(StickyHeaderInfo {
            file_idx: active_fi,
            banner_row,
            banner_vis,
            banner_len,
            sticky_lines_count,
            sticky_start_seg,
            hunk,
        })
    }

    pub fn sticky_header_info(&self, cw: usize) -> Option<StickyHeaderInfo> {
        let (starts, _, seg_counts, _) = self.visual_geometry(cw);
        self.sticky_header_info_from_layout(&starts, &seg_counts, cw)
    }

    /// Translate a viewport-relative screen row (0..vh) to its corresponding
    /// visual line offset, taking into account pinned sticky file and hunk
    /// headers.
    pub fn viewport_row_to_visual(&self, row_in_viewport: usize) -> usize {
        if let Some(s) = self.sticky_header_info(self.last_content_width) {
            if row_in_viewport < s.sticky_lines_count {
                s.banner_vis + s.sticky_start_seg + row_in_viewport
            } else if let Some(h) = s.hunk
                && row_in_viewport < s.sticky_lines_count + h.sticky_lines_count
            {
                let hunk_row = row_in_viewport - s.sticky_lines_count;
                h.hunk_vis + h.sticky_start_seg + hunk_row
            } else {
                self.scroll + row_in_viewport
            }
        } else {
            self.scroll + row_in_viewport
        }
    }

    /// Render the diff rows for the current viewport slice as Ratatui
    /// lines: no ANSI round-trip.
    #[allow(clippy::type_complexity)]
    pub fn render_diff_viewport_lines(
        &mut self,
        width: usize,
        vh: usize,
        track_h: usize,
    ) -> (Vec<Line<'static>>, bool, usize, usize, usize) {
        let frame_start = Instant::now();
        let max_rows = vh.max(4);
        let mut lines: Vec<Line<'static>> = Vec::new();
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
        self.assert_content_width_fits_gutter(est_content_width);
        let mut geo = self.cached_geometry(est_content_width);

        let has_scrollbar = !self.raw_diff && geo.total_vis_lines > vh && vh > 2;
        let content_width = if has_scrollbar {
            width.saturating_sub(1)
        } else {
            width
        };
        self.assert_content_width_fits_gutter(content_width);
        self.terminal_width = width;
        self.last_content_width = content_width;

        if !has_scrollbar && content_width != est_content_width {
            geo = self.cached_geometry(content_width);
        }
        let super::super::geometry::CachedGeometry {
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
            let sticky = self.sticky_header_info_from_layout(&starts, &seg_counts, content_width);
            let sticky_count = sticky.map_or(0, |s| s.effective_sticky_lines_for_row(self.row_idx));
            let top_vis = if self.vis_line == 0 {
                min_vis
            } else {
                cursor_vis
            };
            if top_vis < self.scroll + sticky_count {
                self.scroll = top_vis.saturating_sub(sticky_count);
            }
            if cursor_vis >= self.scroll + max_rows {
                self.scroll = cursor_vis.saturating_sub(max_rows.saturating_sub(1));
            }
        }
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);

        let sticky = self.sticky_header_info_from_layout(&starts, &seg_counts, content_width);
        let sticky_count = sticky.map_or(0, |s| s.total_sticky_lines());
        let content_start_vis = self.scroll + sticky_count;
        let content_end_vis = self.scroll + max_rows;
        if let Some(s) = sticky {
            let file_obj = &self.output.files[s.file_idx];
            let is_collapsed = self.collapsed_files.contains(&s.file_idx);
            let is_current = s.file_idx == self.current_file_idx();
            let in_band = band[s.banner_row];
            let common_prefix = self.output.common_directory_prefix();
            let text_cols = content_width.max(1);
            for s_idx in s.sticky_start_seg..s.banner_len {
                let is_selected = s.banner_row == self.row_idx && self.vis_line == s_idx;
                let seg_start = s_idx * text_cols;
                line_src.push(Some(s.banner_row));
                lines.push(file_header_segment_line(
                    file_obj,
                    self.file_badge(s.file_idx),
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
            if let Some(h) = s.hunk {
                let hunk_obj = &self.output.files[h.file_idx].hunks[h.hunk_idx];
                let hidden_gap = self.hunk_leading_gap(h.file_idx, h.hunk_idx);
                let is_cur = h.file_idx == self.current_file_idx() && cur_hunk == Some(h.hunk_idx);
                for s_idx in h.sticky_start_seg..h.hunk_len {
                    let is_selected = h.flat_row == self.row_idx && self.vis_line == s_idx;
                    let seg_start = s_idx * text_cols;
                    line_src.push(Some(h.flat_row));
                    lines.push(hunk_divider_segment_line(
                        crate::render::Badge {
                            idx: h.hunk_idx,
                            total: Some(self.output.files[h.file_idx].hunks.len()),
                        },
                        hunk_obj,
                        hidden_gap,
                        is_cur,
                        is_selected,
                        seg_start,
                        content_width,
                        &self.theme,
                    ));
                }
            }
        }

        // Viewport iteration, seeking the first visible row by binary
        // search over visual prefix sums instead of scanning from the top
        // every frame. Rows tile visual space exactly
        // (`starts[i+1] == starts[i] + counts[i]`), so the row containing
        // `scroll` is the last one starting at or before it.
        let first_idx = if self.flat.is_empty() {
            0
        } else {
            starts
                .partition_point(|&s| s <= content_start_vis)
                .saturating_sub(1)
        };
        let mut vis_i = starts.get(first_idx).copied().unwrap_or(0);
        let gutter_icon_color = self.theme.palette[crate::render::Cursor::Rest]
            [diffuse_render::DiffKind::Ctx]
            .gutter
            .icon
            .color;
        let is_old_active = self.is_old_line_gutter_active(content_width);
        let old_line_w = if is_old_active {
            self.old_line_num.width
        } else {
            0
        };
        let new_line_w = self.new_line_num.width;
        let diff_gutter_w = self.gutter_width_for(content_width);
        let diff_text_cols = content_width.saturating_sub(diff_gutter_w).max(1);
        let (sbs_left_w, sbs_right_w) = split_pane_widths(content_width);
        let sbs_old_w = self.old_line_num.width;
        let sbs_new_w = self.new_line_num.width;
        for (idx, item) in self.flat.iter().enumerate().skip(first_idx) {
            let count = counts[idx];
            if vis_i + count <= content_start_vis {
                vis_i += count;
                continue;
            }
            if vis_i >= content_end_vis {
                break;
            }
            let in_band = band[idx];
            let in_hunk_extent = !in_band && hunk_extent[idx];

            let is_sticky_hunk = sticky
                .and_then(|s| s.hunk)
                .is_some_and(|sh| sh.flat_row == idx);
            let div_segs = if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                let h = &self.output.files[div_fi].hunks[div_hi];
                let hidden_gap = self.hunk_leading_gap(div_fi, div_hi);
                let is_cur = div_fi == self.current_file_idx() && cur_hunk == Some(div_hi);
                let d_segs = self.hunk_div_seg_count(div_fi, div_hi, content_width);
                let text_cols = content_width.max(1);
                for s_idx in 0..d_segs {
                    if !is_sticky_hunk && vis_i >= content_start_vis && vis_i < content_end_vis {
                        let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                        let seg_start = s_idx * text_cols;
                        line_src.push(Some(idx));
                        lines.push(hunk_divider_segment_line(
                            crate::render::Badge {
                                idx: div_hi,
                                total: Some(self.output.files[div_fi].hunks.len()),
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

            match item.kind {
                FlatRowKind::FileHeader => {
                    let file_obj = &self.output.files[item.file_idx];
                    let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                    let seg_count = seg_counts[idx];
                    let common_prefix = self.output.common_directory_prefix();
                    let is_sticky = sticky.is_some_and(|s| s.banner_row == idx);
                    for s_idx in 0..seg_count {
                        if !is_sticky && vis_i >= content_start_vis && vis_i < content_end_vis {
                            let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                            let is_current = item.file_idx == self.current_file_idx();
                            let text_cols = content_width.max(1);
                            let seg_start = s_idx * text_cols;
                            line_src.push(Some(idx));
                            lines.push(file_header_segment_line(
                                file_obj,
                                self.file_badge(item.file_idx),
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
                }
                FlatRowKind::HunkDivider { hunk_idx } => {
                    let seg_count = seg_counts[idx];
                    let hunks = if !self.output.files[item.file_idx].raw_hunks.is_empty() {
                        &self.output.files[item.file_idx].raw_hunks
                    } else {
                        &self.output.files[item.file_idx].hunks
                    };
                    let h = &hunks[hunk_idx];
                    let sep_text = format!(
                        "@@ -{},{} +{},{} @@",
                        h.old_start, h.old_count, h.new_start, h.new_count
                    );
                    for s_idx in 0..seg_count {
                        if vis_i >= content_start_vis && vis_i < content_end_vis {
                            let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                            let text_cols = content_width.max(1);
                            let piece =
                                slice_by_visual_width(&sep_text, s_idx * text_cols, text_cols);
                            let is_last_seg = s_idx + 1 == seg_count;
                            line_src.push(Some(idx));
                            let raw = raw_row_spans(
                                RowKind::Ctx,
                                Vec::new(),
                                &piece,
                                is_selected,
                                in_band && !is_selected,
                                is_last_seg,
                                &self.theme,
                            );
                            lines.push(fit_line(raw.spans, content_width, raw.line_bg));
                        }
                        vis_i += 1;
                    }
                }
                FlatRowKind::BottomGapDivider { .. } => {
                    let gap = self.hunk_trailing_gap(item.file_idx);
                    let bot_segs = seg_counts[idx];
                    let is_cur = item.file_idx == self.current_file_idx() && idx == self.row_idx;
                    let text_cols = content_width.max(1);
                    for s_idx in 0..bot_segs {
                        if vis_i >= content_start_vis && vis_i < content_end_vis {
                            let is_selected = idx == self.row_idx && self.vis_line == s_idx;
                            let seg_start = s_idx * text_cols;
                            line_src.push(Some(idx));
                            lines.push(bottom_gap_divider_segment_line(
                                gap,
                                is_cur,
                                is_selected,
                                seg_start,
                                content_width,
                                &self.theme,
                            ));
                        }
                        vis_i += 1;
                    }
                }
                FlatRowKind::DiffRow { .. } | FlatRowKind::ExpandedMod { .. } => {
                    let Some(r) = self.row_of(item) else {
                        continue;
                    };
                    let seg_count = seg_counts[idx];

                    if self.side_by_side {
                        for s_idx in 0..seg_count {
                            if vis_i >= content_start_vis && vis_i < content_end_vis {
                                let is_selected =
                                    idx == self.row_idx && self.vis_line == div_segs + s_idx;
                                let mut l_pref = if s_idx == 0 {
                                    styled_split_left_prefix_spans(
                                        r.kind,
                                        r.mov_side,
                                        r.group,
                                        r.old_line,
                                        in_hunk_extent,
                                        None,
                                        &self.theme,
                                        sbs_old_w,
                                    )
                                } else {
                                    styled_split_left_continuation_prefix_spans(None, sbs_old_w)
                                };
                                let expand_icon =
                                    self.hunk_expand_icon_for_segment(idx, s_idx, &seg_counts);
                                let icon_glyph =
                                    expand_icon.map_or(" ", crate::types::HunkExpandIcon::as_str);
                                l_pref.insert(
                                    0,
                                    StyledSpan::styled(
                                        icon_glyph.to_string(),
                                        Some(gutter_icon_color),
                                        None,
                                    )
                                    .with_bold(expand_icon.is_some()),
                                );
                                let r_pref = if s_idx == 0 {
                                    styled_split_right_prefix_spans(
                                        r.kind,
                                        r.mov_side,
                                        r.group,
                                        r.new_line,
                                        in_hunk_extent,
                                        None,
                                        &self.theme,
                                        sbs_new_w,
                                    )
                                } else {
                                    styled_split_right_continuation_prefix_spans(None, sbs_new_w)
                                };

                                let row = side_by_side_row_segment_spans(
                                    r,
                                    s_idx,
                                    sbs_left_w,
                                    sbs_right_w,
                                    l_pref,
                                    r_pref,
                                    is_selected,
                                    in_band && !is_selected,
                                    self.col_offset,
                                    self.word_diff,
                                    self.syntax_on,
                                    &self.theme,
                                );
                                line_src.push(Some(idx));
                                lines.push(fit_line(row.spans, content_width, row.line_bg));
                            }
                            vis_i += 1;
                        }
                    } else {
                        let mod_side = item.mod_side();
                        let mod_glyph = item.mod_glyph();
                        let row_text = match mod_side {
                            Some(diffuse_core::ModSide::Del) => r.old_text(),
                            Some(diffuse_core::ModSide::Add) => r.new_text(),
                            None => r.text(),
                        };

                        for s_idx in 0..seg_count {
                            if vis_i >= content_start_vis && vis_i < content_end_vis {
                                let is_selected =
                                    idx == self.row_idx && self.vis_line == div_segs + s_idx;
                                let mut prefix = if s_idx == 0 {
                                    styled_prefix_spans_with_widths(
                                        r.kind,
                                        r.mov_side,
                                        mod_side,
                                        mod_glyph,
                                        r.group,
                                        r.new_line,
                                        r.old_line,
                                        &r.runs,
                                        in_hunk_extent,
                                        None,
                                        &self.theme,
                                        old_line_w,
                                        new_line_w,
                                    )
                                } else {
                                    continuation_prefix_spans_with_mod_side_and_widths(
                                        mod_side,
                                        mod_glyph,
                                        &self.theme,
                                        None,
                                        old_line_w,
                                        new_line_w,
                                    )
                                };
                                let expand_icon =
                                    self.hunk_expand_icon_for_segment(idx, s_idx, &seg_counts);
                                let icon_glyph =
                                    expand_icon.map_or(" ", crate::types::HunkExpandIcon::as_str);
                                prefix.insert(
                                    0,
                                    StyledSpan::styled(
                                        icon_glyph.to_string(),
                                        Some(gutter_icon_color),
                                        None,
                                    )
                                    .with_bold(expand_icon.is_some()),
                                );
                                let rendered_row = if self.raw_diff {
                                    let piece = slice_by_visual_width(
                                        &row_text,
                                        s_idx * diff_text_cols,
                                        diff_text_cols,
                                    );
                                    let is_last_seg = s_idx + 1 == seg_count;
                                    let raw = raw_row_spans(
                                        r.kind,
                                        prefix,
                                        &piece,
                                        is_selected,
                                        in_band && !is_selected,
                                        is_last_seg,
                                        &self.theme,
                                    );
                                    fit_line(raw.spans, content_width, raw.line_bg)
                                } else {
                                    let seg_start = s_idx * diff_text_cols;
                                    let seg_end = seg_start + diff_text_cols;
                                    let row = row_segment_spans(
                                        &row_text,
                                        r.is_aux(),
                                        r.kind,
                                        r.mov_side,
                                        mod_side,
                                        &r.runs,
                                        seg_start,
                                        seg_end,
                                        prefix,
                                        is_selected,
                                        in_band && !is_selected,
                                        self.col_offset,
                                        self.word_diff,
                                        self.syntax_on,
                                        &self.theme,
                                    );
                                    fit_line(row.spans, content_width, row.line_bg)
                                };
                                line_src.push(Some(idx));
                                lines.push(rendered_row);
                            }
                            vis_i += 1;
                        }
                    }
                }
            }

            if callouts.contains_key(&idx) {
                // Focused rows re-render their note block with the cursor
                // note highlighted; other rows use the cached lines.
                let focus = if idx == self.row_idx {
                    match self.visual_target_at(self.row_idx, self.vis_line) {
                        Some(super::super::notes::VisualTarget::Note {
                            note_idx,
                            line_idx,
                            ..
                        }) => {
                            let note_start = div_segs + seg_counts[idx];
                            (self.vis_line >= note_start)
                                .then_some(NoteRowFocus { note_idx, line_idx })
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                let block = if focus.is_some() {
                    self.note_block_lines(idx, content_width, focus)
                } else {
                    callouts.get(&idx).cloned().unwrap_or_default()
                };
                for nline in block {
                    if vis_i >= content_start_vis && vis_i < content_end_vis {
                        line_src.push(Some(idx));
                        lines.push(nline);
                    }
                    vis_i += 1;
                }
            }
        }

        while lines.len() < vh {
            line_src.push(None);
            lines.push(Line::default());
        }

        if cfg!(debug_assertions) {
            for (idx, line) in lines.iter().enumerate() {
                let vw = line.width();
                debug_assert!(
                    vw <= content_width,
                    "render_diff_viewport_lines produced grid line {idx} ({}) with visual width ({vw}) > content_width ({content_width}):\n\n{}",
                    self.viewport_line_origin(line_src.get(idx).copied().flatten()),
                    escape_ansi_for_diagnostic(&line.to_string()),
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
