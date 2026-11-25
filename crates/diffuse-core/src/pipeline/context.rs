//! Review note context synthesis, plain common context, and markdown rendering.

use std::collections::HashMap;

use diffuse_syntax::{fence::fence_regions, profile::LangProfile};

use crate::{
    notes::{NotesDB, ReviewNote, query_notes_for_file},
    parse::{CtxLine, DiffFile, Hunk, Row},
    visible::Side as VisibleSide,
    wire::{OutputRow, OutputRun},
};

use super::{PipelineOptions, classify::build_hunk_output_row, file::process_single_diff_file};

/// Synthesizes context hunks around review notes that fall into gaps between
/// diff hunks or outside existing diff hunks.
pub fn synthesize_note_context_hunks(
    f: &mut DiffFile,
    notes_db: &NotesDB,
    repo_key: Option<&str>,
    rev_key: Option<&str>,
    visual_context: usize,
) {
    if f.is_binary || f.status == crate::parse::FileStatus::Deleted {
        return;
    }
    let notes = query_notes_for_file(notes_db, repo_key, rev_key, &f.path);
    if notes.is_empty() {
        return;
    }

    let n = visual_context as u32;

    // 1. Identify line notes whose target span [start..=end] is uncovered by
    //    existing diff hunks (file notes never anchor to hunk spans).
    // A coordinate resolves to the new side when a new line number exists,
    // otherwise to the old side.
    let uncovered: Vec<(&ReviewNote, VisibleSide, u32, u32)> = notes
        .into_iter()
        .filter_map(|note| {
            let (side, start, end) = crate::notes::note_line_span(&note.location)?;
            let content_lines = match side {
                VisibleSide::New => &f.new_content,
                VisibleSide::Old => &f.old_content,
            };
            if !crate::notes::note_anchors_in_context_lines(note, content_lines) {
                return None;
            }
            let covered = f.hunks.iter().any(|h| match side {
                VisibleSide::New => {
                    let h_start = h.new_start;
                    let h_end = h.new_start + h.new_count.saturating_sub(1);
                    h.new_count > 0 && start >= h_start && end <= h_end
                }
                VisibleSide::Old => {
                    let h_start = h.old_start;
                    let h_end = h.old_start + h.old_count.saturating_sub(1);
                    h.old_count > 0 && start >= h_start && end <= h_end
                }
            });
            (!covered).then_some((note, side, start, end))
        })
        .collect();

    if uncovered.is_empty() {
        return;
    }

    let m = f.hunks.len();

    // Helper: cumulative line offset caused by all hunks preceding gap_idx.
    let gap_delta = |gap_idx: usize| -> i64 {
        if gap_idx == 0 || m == 0 {
            0
        } else {
            let h = &f.hunks[gap_idx - 1];
            (h.new_start + h.new_count) as i64 - (h.old_start + h.old_count) as i64
        }
    };

    struct NoteSpan {
        gap_idx: usize,
        new_start: u32,
        new_end: u32,
    }

    let mut spans: Vec<NoteSpan> = Vec::new();

    for (_note, note_side, raw_start, raw_end) in uncovered {
        let (n_start, n_end) = (raw_start.min(raw_end).max(1), raw_start.max(raw_end).max(1));

        let gap_idx = if m == 0 {
            0
        } else {
            match note_side {
                VisibleSide::New => {
                    let mut found = m;
                    for (i, h) in f.hunks.iter().enumerate() {
                        if n_start < h.new_start {
                            found = i;
                            break;
                        }
                    }
                    found
                }
                VisibleSide::Old => {
                    let mut found = m;
                    for (i, h) in f.hunks.iter().enumerate() {
                        if n_start < h.old_start {
                            found = i;
                            break;
                        }
                    }
                    found
                }
            }
        };

        let delta = gap_delta(gap_idx);

        let (new_s, new_e) = match note_side {
            VisibleSide::New => (n_start, n_end),
            VisibleSide::Old => {
                let ns = ((n_start as i64) + delta).max(1) as u32;
                let ne = ((n_end as i64) + delta).max(1) as u32;
                (ns, ne)
            }
        };

        spans.push(NoteSpan {
            gap_idx,
            new_start: new_s,
            new_end: new_e,
        });
    }

    let mut by_gap: HashMap<usize, Vec<NoteSpan>> = HashMap::new();
    for span in spans {
        by_gap.entry(span.gap_idx).or_default().push(span);
    }

    let total_new_lines = if f.new_line_count > 0 {
        f.new_line_count as u32
    } else {
        f.new_content.len() as u32
    };

    let mut synthetic_hunks: Vec<Hunk> = Vec::new();

    for (gap_idx, mut gap_spans) in by_gap {
        gap_spans.sort_by_key(|s| s.new_start);

        let mut clusters: Vec<(u32, u32)> = Vec::new();
        for span in gap_spans {
            if let Some(last) = clusters.last_mut()
                && span.new_start <= last.1 + 2 * n + 1
            {
                last.1 = last.1.max(span.new_end);
                continue;
            }
            clusters.push((span.new_start, span.new_end));
        }

        let delta = gap_delta(gap_idx);

        let (prev_end_new, prev_exists) = if gap_idx > 0 && m > 0 {
            let hp = &f.hunks[gap_idx - 1];
            (hp.new_start + hp.new_count - 1, true)
        } else {
            (0, false)
        };

        let (next_start_new, next_exists) = if gap_idx < m {
            let hn = &f.hunks[gap_idx];
            (hn.new_start, true)
        } else {
            (0, false)
        };

        for (c_start, c_end) in clusters {
            let synth_new_start = if prev_exists {
                let p_next = prev_end_new + 1;
                let dist_to_prev = c_start.saturating_sub(p_next);
                if dist_to_prev <= 2 * n {
                    p_next
                } else {
                    c_start.saturating_sub(n).max(p_next)
                }
            } else {
                c_start.saturating_sub(n).max(1)
            };

            let synth_new_end = if next_exists {
                let s_prev = next_start_new.saturating_sub(1);
                let dist_to_next = s_prev.saturating_sub(c_end);
                if dist_to_next <= 2 * n {
                    s_prev
                } else {
                    (c_end + n).min(s_prev)
                }
            } else {
                let target = c_end + n;
                if total_new_lines > 0 {
                    target.min(total_new_lines)
                } else {
                    target
                }
            };

            if synth_new_start > synth_new_end || synth_new_start == 0 {
                continue;
            }

            let synth_old_start = ((synth_new_start as i64) - delta).max(1) as u32;
            let count = synth_new_end - synth_new_start + 1;

            let mut rows = Vec::with_capacity(count as usize);
            for i in 0..count {
                let nl = synth_new_start + i;
                let ol = synth_old_start + i;
                let text = if let Some(line) = f.new_content.get((nl as usize).saturating_sub(1)) {
                    line.clone()
                } else if let Some(line) = f.old_content.get((ol as usize).saturating_sub(1)) {
                    line.clone()
                } else {
                    String::new()
                };
                rows.push(Row::ctx(CtxLine::new(ol, nl, text)));
            }

            let hunk = Hunk {
                old_start: synth_old_start,
                old_count: count,
                new_start: synth_new_start,
                new_count: count,
                section: String::new(),
                rows,
                above_ctx: count,
                below_ctx: count,
            };
            synthetic_hunks.push(hunk);
        }
    }

    if !synthetic_hunks.is_empty() {
        f.hunks.extend(synthetic_hunks.clone());
        f.hunks.sort_by_key(|h| (h.old_start, h.new_start));
        crate::shift::merge_adjacent_hunks(f, visual_context);

        f.raw_hunks.extend(synthetic_hunks);
        f.raw_hunks.sort_by_key(|h| (h.old_start, h.new_start));
        crate::shift::merge_adjacent_hunks_list(&mut f.raw_hunks);
    }
}

/// Synthesizes a plain common context hunk from line strings.
///
/// Used for pure renames, mode changes, and note documents where the entire
/// content is presented as common context rows.
pub fn synthesize_plain_common_context_hunk(lines: &[&str]) -> crate::parse::Hunk {
    let mut ctx_rows = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let pos = (i + 1) as u32;
        ctx_rows.push(crate::parse::Row::ctx(crate::parse::CtxLine::new(
            pos, pos, *line,
        )));
    }
    let count = ctx_rows.len() as u32;
    crate::parse::Hunk {
        old_start: 1,
        old_count: count,
        new_start: 1,
        new_count: count,
        section: String::new(),
        rows: ctx_rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

/// Generalized super-function for plain common context rendering.
///
/// Renders a slice of line strings into classified `OutputRow`s using the given
/// language profile and fence regions detection.
pub fn render_plain_common_context(
    lines: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<OutputRow> {
    let hunk = synthesize_plain_common_context_hunk(lines);
    let regions = fence_regions(lines, profile);
    hunk.rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let next_line = hunk.rows.get(i + 1).map(|nr| nr.text());
            build_hunk_output_row(r, next_line, profile, &regions)
        })
        .collect()
}

/// Tokenizes and classifies a complete markdown document into `OutputRow`s
/// using the standard diffuse pipeline and note file status (fence detection,
/// nested language syntax highlighting, and note styling).
pub fn markdown_file_rows(text: &str) -> Vec<OutputRow> {
    if text.is_empty() {
        return Vec::new();
    }
    let f = crate::parse::DiffFile {
        path: "note.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: crate::parse::FileStatus::Unmodified,
        is_binary: false,
        has_file_headers: false,
        hunks: vec![],
        raw_hunks: vec![],
        new_line_count: 0,
        old_line_count: 0,
        new_bytes: Some(text.len()),
        old_bytes: None,
        new_content: vec![],
        old_content: vec![],
        no_content: false,
    };
    let opts = PipelineOptions::default().with_lazy_collapsed(false);
    let output_file = process_single_diff_file(f, Some(text), None, None, &opts);
    output_file.rows
}

/// Tokenizes a single line of note text using the shared markdown file pipeline
/// with note styling.
pub fn note_runs(text: &str) -> Vec<OutputRun> {
    let norm = crate::notes::normalize_note_text(text);
    markdown_file_rows(&norm)
        .into_iter()
        .flat_map(|r| r.runs)
        .collect()
}
