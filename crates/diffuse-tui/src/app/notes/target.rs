//! Target resolution and anchoring of review notes to rows and visual line
//! indices.

use crate::{
    app::AppState,
    core::notes::{NoteLocation, ReviewNote, note_anchors_in_rows},
    types::FlatRowKind,
};

/// Target entity addressed by cursor coordinate `(row_idx, vis_line)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualTarget<'a> {
    FileHeader,
    HunkDivider {
        file_idx: usize,
        hunk_idx: usize,
    },
    DiffRow {
        seg_idx: usize,
        total_segs: usize,
    },
    Note {
        note: &'a ReviewNote,
        note_idx: usize,
        line_idx: usize,
        total_note_lines: usize,
    },
    BottomGapDivider {
        file_idx: usize,
    },
}

impl AppState {
    /// Determine which flat row a note anchors to.
    ///
    /// Anchoring is coordinate-exact: the note must anchor against the file's
    /// output rows (exact both-side coordinate match plus complete hash), and
    /// the flat row whose coordinates exactly match the note's end coordinate
    /// receives the callout. Otherwise, the note falls back to the file
    /// header banner row.
    pub fn note_target_row(&self, file_idx: usize, note: &ReviewNote) -> usize {
        let banner_idx = self.first_row_of_file(file_idx);
        let Some(f) = self.output.files.get(file_idx) else {
            return banner_idx;
        };
        // The end coordinate of the location: where the callout attaches.
        let end_coord = match &note.location {
            NoteLocation::File => return banner_idx,
            NoteLocation::Line { coord } => *coord,
            NoteLocation::Range { end, .. } => *end,
        };
        // Collect the file's visible output rows for anchor verification —
        // the same side-projected rows the user sees and the note's rows
        // were captured from (render's `partition_file_notes` uses the same
        // view). Hunks are the fallback when no visible rows exist.
        let file_rows: Vec<_> = if f.rows.is_empty() {
            f.hunks.iter().flat_map(|h| h.rows.iter()).collect()
        } else {
            f.rows.iter().collect()
        };
        if !note_anchors_in_rows(note, file_rows.iter().copied()) {
            return banner_idx;
        }
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file_idx {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            if r.old_line == end_coord.old_line_num.map(|n| n.get())
                && r.new_line == end_coord.new_line_num.map(|n| n.get())
            {
                return i;
            }
        }
        banner_idx
    }

    /// Map a coordinate `(row_idx, vis_line)` to its semantic visual target.
    pub fn visual_target_at(&self, row_idx: usize, vis_line: usize) -> Option<VisualTarget<'_>> {
        let item = self.flat.get(row_idx)?;
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let div_segs = if !self.raw_diff
            && let Some(hs) = self.hunk_starts.iter().find(|h| h.flat == row_idx)
        {
            self.hunk_div_seg_count(hs.file, hs.hunk, cw)
        } else {
            0
        };

        if vis_line < div_segs
            && let Some(hs) = self.hunk_starts.iter().find(|h| h.flat == row_idx)
        {
            return Some(VisualTarget::HunkDivider {
                file_idx: hs.file,
                hunk_idx: hs.hunk,
            });
        }

        match item.kind {
            FlatRowKind::FileHeader => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::FileHeader);
                }
            }
            FlatRowKind::HunkDivider { hunk_idx } => {
                return Some(VisualTarget::HunkDivider {
                    file_idx: item.file_idx,
                    hunk_idx,
                });
            }
            FlatRowKind::BottomGapDivider { .. } => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::BottomGapDivider {
                        file_idx: item.file_idx,
                    });
                }
            }
            FlatRowKind::DiffRow { .. } => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::DiffRow {
                        seg_idx: vis_line.saturating_sub(div_segs),
                        total_segs: segs,
                    });
                }
            }
        }

        let segs = self.row_seg_count(row_idx, cw);
        let note_start = div_segs + segs;
        if vis_line >= note_start {
            let mut note_rel = vis_line - note_start;
            let note_layout = self.note_callout_layout_for_row(row_idx, cw);
            for (n_idx, (note, note_total_lines)) in note_layout.into_iter().enumerate() {
                if note_rel < note_total_lines {
                    return Some(VisualTarget::Note {
                        note,
                        note_idx: n_idx,
                        line_idx: note_rel,
                        total_note_lines: note_total_lines,
                    });
                }
                note_rel -= note_total_lines;
            }
        }

        None
    }
}
