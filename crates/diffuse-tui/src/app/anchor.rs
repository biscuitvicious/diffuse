use std::collections::HashSet;

use super::AppState;

/// Which neighbour row a merged-divider expansion should lock in place.
///
/// `BelowRow`: the row just below the divider (▲▲ fills from the top side,
/// insertions land above the anchor). `AboveRow`: the row just above the
/// divider (▼▼ fills from the bottom side, insertions land below the anchor,
/// so the upper content never moves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DividerFallback {
    BelowRow,
    AboveRow,
}

/// Everything needed to restore the viewport after a hunk-divider expansion:
/// the cursor anchor, the divider's screen position, and both neighbour rows
/// (as coordinates plus their pre-click text screen positions) for merge
/// fallbacks.
pub(crate) struct DividerAnchors {
    pub(crate) cursor: CursorAnchor,
    pub(crate) div_screen_y: usize,
    pub(crate) below: Option<CursorAnchor>,
    pub(crate) below_text_screen_y: Option<usize>,
    pub(crate) above: Option<CursorAnchor>,
    pub(crate) above_text_screen_y: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CursorAnchor {
    FileHeader(usize),
    HunkDivider(usize, usize),
    Coord {
        file_idx: usize,
        new_line: Option<u32>,
        old_line: Option<u32>,
    },
    Index(usize),
}

impl AppState {
    /// Locate the flat index of the content row in `file_idx` matching the
    /// given new/old line coordinate, skipping file headers and hunk
    /// dividers.
    pub(crate) fn capture_cursor_anchor(&self) -> (CursorAnchor, usize) {
        let old_vis = self.cursor_visual_line();
        let screen_y = old_vis.saturating_sub(self.scroll);

        let anchor = if let Some(fr) = self.current_flat_row() {
            if fr.is_file_header {
                CursorAnchor::FileHeader(fr.file_idx)
            } else if fr.is_hunk_divider {
                CursorAnchor::HunkDivider(fr.file_idx, fr.hunk_idx)
            } else if let Some(r) = self.row_of(fr) {
                CursorAnchor::Coord {
                    file_idx: fr.file_idx,
                    new_line: r.new_line,
                    old_line: r.old_line,
                }
            } else {
                CursorAnchor::Index(self.row_idx)
            }
        } else {
            CursorAnchor::Index(self.row_idx)
        };

        (anchor, screen_y)
    }

    pub(crate) fn restore_cursor_anchor(
        &mut self,
        anchor: CursorAnchor,
        screen_y: usize,
        _sep_shift: usize,
    ) {
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        let found_pos = match anchor {
            CursorAnchor::FileHeader(fi) => self
                .flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_file_header),
            CursorAnchor::HunkDivider(fi, hi) => self
                .flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_hunk_divider && fr.hunk_idx == hi),
            CursorAnchor::Coord {
                file_idx,
                new_line,
                old_line,
            } => self.flat.iter().position(|fr| {
                fr.file_idx == file_idx
                    && !fr.is_file_header
                    && !fr.is_hunk_divider
                    && self.row_of(fr).is_some_and(|r| {
                        if let Some(nl) = new_line {
                            r.new_line == Some(nl)
                        } else if let Some(ol) = old_line {
                            r.old_line == Some(ol)
                        } else {
                            false
                        }
                    })
            }),
            CursorAnchor::Index(idx) => Some(idx),
        };

        if let Some(pos) = found_pos {
            self.row_idx = pos;
        } else if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }

        let new_vis = self.cursor_visual_line();
        let (_starts, _counts, _seg, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = new_vis.saturating_sub(screen_y).min(max_scroll);
    }

    pub(crate) fn capture_divider_anchor(&self, fi: usize, hi: usize) -> DividerAnchors {
        let (cursor, _cursor_screen_y) = self.capture_cursor_anchor();
        let (starts, counts, seg_counts, _) = self.visual_geometry(self.last_content_width);
        let div_start = self
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == hi)
            .map(|hs| hs.flat);
        let div_vis = div_start
            .map(|flat| starts[flat])
            .unwrap_or_else(|| self.cursor_visual_line());
        let div_screen_y = div_vis.saturating_sub(self.scroll);

        // Flat indices that carry leading hunk-divider segments.
        let div_flats: HashSet<usize> = self.hunk_starts.iter().map(|hs| hs.flat).collect();
        // Visual offset of a flat entry's text from its block start (leading
        // divider segments, if any).
        let text_off = |pos: usize| -> usize {
            if div_flats.contains(&pos) {
                counts[pos].saturating_sub(seg_counts[pos])
            } else {
                0
            }
        };
        let coord_of = |pos: usize| -> Option<CursorAnchor> {
            let fr = self.flat.get(pos)?;
            if fr.is_file_header || fr.is_hunk_divider {
                return None;
            }
            let r = self.row_of(fr)?;
            Some(CursorAnchor::Coord {
                file_idx: fr.file_idx,
                new_line: r.new_line,
                old_line: r.old_line,
            })
        };

        // The divider renders as leading segments of the hunk's first row
        // entry, so that entry *is* the row just below the divider. The row
        // above is the previous flat entry (the last row of the previous hunk).
        let below = div_start.and_then(coord_of);
        let below_text_screen_y = div_start
            .map(|flat| starts[flat] + text_off(flat))
            .map(|vis| vis.saturating_sub(self.scroll));
        let above = div_start
            .filter(|flat| *flat > 0)
            .and_then(|flat| coord_of(flat - 1));
        let above_text_screen_y = div_start
            .filter(|flat| *flat > 0)
            .map(|flat| starts[flat - 1] + text_off(flat - 1))
            .map(|vis| vis.saturating_sub(self.scroll));

        DividerAnchors {
            cursor,
            div_screen_y,
            below,
            below_text_screen_y,
            above,
            above_text_screen_y,
        }
    }

    pub(crate) fn restore_divider_pivot(
        &mut self,
        anchors: DividerAnchors,
        fi: usize,
        hi: usize,
        fallback: DividerFallback,
    ) {
        let cursor_anchor = anchors.cursor;
        let div_screen_y = anchors.div_screen_y;
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        let found_pos = match cursor_anchor {
            CursorAnchor::FileHeader(f) => self
                .flat
                .iter()
                .position(|fr| fr.file_idx == f && fr.is_file_header),
            CursorAnchor::HunkDivider(f, h) => self
                .flat
                .iter()
                .position(|fr| fr.file_idx == f && fr.is_hunk_divider && fr.hunk_idx == h),
            CursorAnchor::Coord {
                file_idx,
                new_line,
                old_line,
            } => self.flat_pos_of_coord(file_idx, new_line, old_line),
            CursorAnchor::Index(idx) => Some(idx),
        };

        // When an expansion merged the hunks, the divider no longer exists;
        // lock the chosen neighbour row (below for top-side fills, above for
        // bottom-side fills) at its pre-click screen position.
        let divider_gone = !self
            .hunk_starts
            .iter()
            .any(|hs| hs.file == fi && hs.hunk == hi);
        let (fallback_anchor, fallback_screen_y) = match fallback {
            DividerFallback::BelowRow => (anchors.below, anchors.below_text_screen_y),
            DividerFallback::AboveRow => (anchors.above, anchors.above_text_screen_y),
        };
        let fallback_pos = fallback_anchor.and_then(|a| match a {
            CursorAnchor::Coord {
                file_idx,
                new_line,
                old_line,
            } => self.flat_pos_of_coord(file_idx, new_line, old_line),
            _ => None,
        });

        match (found_pos, divider_gone, fallback_pos) {
            (Some(pos), _, _) => self.row_idx = pos,
            (None, true, Some(fpos)) => self.row_idx = fpos,
            (None, _, _) if self.row_idx >= self.flat.len() => {
                self.row_idx = self.flat.len().saturating_sub(1);
            }
            (None, _, _) => {}
        }

        // Pivot screen view around the divider line — or, when the merge
        // removed it, around the locked neighbour row.
        let (starts, counts, seg_counts, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let div_flats: HashSet<usize> = self.hunk_starts.iter().map(|hs| hs.flat).collect();
        let text_vis = |pos: usize| -> usize {
            let off = if div_flats.contains(&pos) {
                counts[pos].saturating_sub(seg_counts[pos])
            } else {
                0
            };
            starts[pos] + off
        };
        let div_vis = self
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == hi)
            .map(|hs| starts[hs.flat])
            .or_else(|| {
                if divider_gone {
                    fallback_pos.map(text_vis)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.cursor_visual_line());
        let pivot_screen_y = if divider_gone {
            fallback_screen_y.unwrap_or(div_screen_y)
        } else {
            div_screen_y
        };

        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        let new_scroll = div_vis.saturating_sub(pivot_screen_y).min(max_scroll);
        self.scroll = new_scroll;
        self.scroll_pin = Some((new_scroll, self.row_idx));
    }
}
