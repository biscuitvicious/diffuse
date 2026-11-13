//! Single-file transformation pipeline, move relocation, and stage timing.

use std::{
    fs, mem,
    path::Path,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
    time::Instant,
};

use diffuse_syntax::{
    fence::fence_regions,
    registry::{fence_lang_ext, lang_profile},
};

use crate::{
    collapse::collapse_hunk_rows,
    counts::file_counts,
    lcs::word_spans_and_runs_with_profile,
    merge::{RunKind, is_wholesale_replacement},
    moves::{
        absorb_mov_tails, mark_cross_cluster_moves_with_profile, mark_moves_with_profile,
        resolve_mov_pairs_by_line,
    },
    notes::{NoteStatus, query_notes_for_file},
    parse::{DiffFile, Row, RowKind},
    visible::Side as VisibleSide,
    wire::{OutputFence, OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun},
};

use super::{
    PipelineOptions,
    classify::{build_hunk_output_row, for_each_ctx_line_token, token_cow},
    context::{
        note_runs, render_plain_common_context, synthesize_note_context_hunks,
        synthesize_plain_common_context_hunk,
    },
    split_content_lines,
};

/// Whether a diff file is collapsed by default upon initial ingestion.
pub fn is_initially_collapsed(f: &DiffFile) -> bool {
    f.is_binary
        || f.status == crate::parse::FileStatus::Untracked
        || f.status == crate::parse::FileStatus::Deleted
        || ((f.status == crate::parse::FileStatus::Renamed
            || f.status == crate::parse::FileStatus::ModeChanged)
            && f.raw_hunks.is_empty())
}

pub static T_STAGE: [AtomicU64; 6] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
pub static T_RENDER_HUNKS: AtomicU64 = AtomicU64::new(0);
pub static T_RENDER_ROWS: AtomicU64 = AtomicU64::new(0);

/// Run pipeline diffing, alignment, move detection, and syntax classification
/// on a single parsed diff file using the specified pipeline options.
#[tracing::instrument(
    level = "debug",
    skip(f, new_content, old_content, diff_fallback, opts),
    fields(
        path = %f.path,
        status = ?f.status,
        hunks = f.hunks.len(),
        new_bytes = new_content.map(|s| s.len()),
        old_bytes = old_content.map(|s| s.len())
    )
)]
pub fn process_single_diff_file(
    mut f: DiffFile,
    new_content: Option<&str>,
    old_content: Option<&str>,
    diff_fallback: Option<Arc<String>>,
    opts: &PipelineOptions,
) -> OutputFile {
    // The reload source must always be populated: collapsing a file releases
    // its rendered rows, and expanding it back re-renders from `raw_source`
    // (sync fallback included). Gating this on lazy stubs would leave
    // user-collapsed files unloadable — the `Git` descriptor is ~100 bytes,
    // and for non-git inputs the embedded `Raw` diff is the only reload path.
    let raw_source = if let Some(ref fb) = diff_fallback {
        Some(crate::wire::RawFileSource::Raw {
            path: f.path.clone(),
            diff: Arc::clone(fb),
            new_content: new_content.map(String::from),
            old_content: old_content.map(String::from),
        })
    } else {
        let status_str = match f.status {
            crate::parse::FileStatus::Added => "added".to_string(),
            crate::parse::FileStatus::Deleted => "deleted".to_string(),
            crate::parse::FileStatus::Modified => "modified".to_string(),
            crate::parse::FileStatus::Renamed => "renamed".to_string(),
            crate::parse::FileStatus::Untracked => "untracked".to_string(),
            crate::parse::FileStatus::ModeChanged => "mode changed".to_string(),
            crate::parse::FileStatus::Unmodified => "unmodified".to_string(),
        };
        Some(crate::wire::RawFileSource::Git {
            path: f.path.clone(),
            old_path: f.old_path.clone(),
            status: status_str,
        })
    };

    if opts.lazy_collapsed && is_initially_collapsed(&f) {
        let counts = if f.status == crate::parse::FileStatus::Untracked {
            let line_count = if f.new_line_count > 0 {
                f.new_line_count
            } else if let Some(c) = new_content {
                c.lines().count()
            } else {
                0
            };
            crate::counts::FileCounts {
                adds: line_count,
                ..Default::default()
            }
        } else if f.status == crate::parse::FileStatus::Deleted {
            let line_count = if f.old_line_count > 0 {
                f.old_line_count
            } else if let Some(oc) = old_content {
                oc.lines().count()
            } else {
                crate::counts::file_counts(&f).dels
            };
            crate::counts::FileCounts {
                dels: line_count,
                ..Default::default()
            }
        } else if f.is_binary {
            crate::counts::FileCounts::default()
        } else {
            crate::counts::file_counts(&f)
        };
        return OutputFile::collapsed_stub(&f, counts, raw_source);
    }

    crate::lcs::set_token_product_cap(opts.token_product_cap);
    crate::cluster::dp::set_cluster_product_cap(opts.cluster_product_cap);
    crate::cluster::dp::set_cluster_bandwidth_min(opts.cluster_bandwidth_min);
    crate::cluster::dp::set_cluster_bandwidth_slack(opts.cluster_bandwidth_slack);
    let t_start = Instant::now();
    if new_content.is_none() && old_content.is_none() {
        f.no_content = true;
    }
    let ext = f.path.rsplit('.').next().unwrap_or("");
    let profile = lang_profile(ext);
    let new_lines_fallback: Vec<String>;
    let new_lines: Vec<&str> = if let Some(c) = new_content
        && !c.is_empty()
    {
        let lines = split_content_lines(c);
        f.new_line_count = lines.len();
        lines
    } else {
        new_lines_fallback = crate::old_content::new_content_from_hunks(&f.raw_hunks);
        f.new_line_count = new_lines_fallback.len();
        new_lines_fallback.iter().map(|s| s.as_str()).collect()
    };
    f.new_content = new_lines.iter().map(|s| s.to_string()).collect();

    let old_lines_fallback: Vec<String>;
    let old_lines: Vec<&str> = if let Some(oc) = old_content
        && !oc.is_empty()
    {
        let lines = split_content_lines(oc);
        f.old_line_count = lines.len();
        lines
    } else {
        old_lines_fallback = crate::old_content::old_content_from_hunks(&f.raw_hunks);
        f.old_line_count = old_lines_fallback.len();
        old_lines_fallback.iter().map(|s| s.as_str()).collect()
    };
    f.old_content = old_lines.iter().map(|s| s.to_string()).collect();

    // Surface whitespace-suppressed changes (see `resurface`): requires both
    // content snapshots and is a strict no-op on default-generated tapes.
    if old_content.is_some_and(|c| !c.is_empty())
        && new_content.is_some_and(|c| !c.is_empty())
        && !f.is_binary
        && crate::resurface::resurface_hunks(&mut f.hunks, &old_lines, &new_lines)
    {
        f.raw_hunks = f.hunks.clone();
    }

    let new_bytes: Option<usize> = if f.status == crate::parse::FileStatus::Deleted {
        None
    } else if let Some(b) = f.new_bytes {
        Some(b)
    } else if let Some(c) = new_content {
        Some(c.len())
    } else if f.is_binary {
        if let Some(ref r) = opts.target_repo_key
            && let Ok(meta) = fs::metadata(Path::new(r).join(&f.path))
        {
            Some(meta.len() as usize)
        } else {
            None
        }
    } else {
        None
    };

    let old_bytes: Option<usize> = if f.status == crate::parse::FileStatus::Added
        || f.status == crate::parse::FileStatus::Untracked
    {
        None
    } else if let Some(b) = f.old_bytes {
        Some(b)
    } else {
        old_content.map(|oc| oc.len())
    };
    f.old_bytes = old_bytes;
    f.new_bytes = new_bytes;

    use rayon::prelude::*;

    let t1 = Instant::now();
    T_STAGE[0].fetch_add(
        t1.duration_since(t_start).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Pre-pass: mark cross-cluster strong move groups before hunk collapse so
    // intra-cluster DP does not steal relocated lines into spurious Mod rows.
    let pre_locked = mark_cross_cluster_moves_with_profile(&mut f, profile);
    // Snapshot locked line numbers: collapse shifts row indices, but line
    // numbers are stable identifiers for re-seeding the post-collapse
    // absorption with the pre-pass-locked blocks.
    let mut pre_lines: Vec<(u32, u32)> = pre_locked
        .iter()
        .filter_map(|&(s, t)| {
            let old_line = f.hunks.get(s.0)?.rows.get(s.1)?.old_line()?;
            let new_line = f.hunks.get(t.0)?.rows.get(t.1)?.new_line()?;
            Some((old_line, new_line))
        })
        .collect();
    // Extend the pre-pass shield to uniform-Δ tails before collapse can
    // pair the trailing leftovers into spurious Mods. No Ctx breaking
    // yet: keep collapse input stable and create no orphans for it.
    // Absorbed line pairs join the snapshot: the shield's own extensions
    // are Mov rows too, and the post-collapse pass must see their pairs.
    let pre_absorbed = absorb_mov_tails(&mut f, &pre_locked, profile, false);
    pre_lines.extend(pre_absorbed);

    // Collapse runs first on pre-move rows (source dels survive the collapse).
    let collapse_hunk = |h: &mut crate::parse::Hunk| {
        h.rows = collapse_hunk_rows(
            h,
            Some(old_lines.as_slice()),
            Some(new_lines.as_slice()),
            profile,
        );
    };
    if f.hunks.len() > 1 {
        f.hunks.par_iter_mut().for_each(collapse_hunk);
    } else {
        f.hunks.iter_mut().for_each(collapse_hunk);
    }
    let t2 = Instant::now();
    T_STAGE[1].fetch_add(
        t2.duration_since(t1).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Mark moves after collapse: creates Row::Mov pairs.
    let main_locked = mark_moves_with_profile(&mut f, profile);
    // Sweep uniform-Δ trailing rows (Del/Add leftovers and misassigned
    // Ctx) into the locked blocks; orphans stay as plain leftovers.
    // Pre-pass-locked blocks are re-seeded by line number (their recorded
    // positions went stale across collapse); the main pass skips
    // already-moved rows, so without this they would never be extended.
    let mut seeded = resolve_mov_pairs_by_line(&f, &pre_lines);
    seeded.extend(main_locked);
    let _ = absorb_mov_tails(&mut f, &seeded, profile, true);
    // The post-collapse return value (newly absorbed pairs) is not used: a
    // subsequent index-shifting pass does not follow, so there is no caller
    // that needs to re-resolve positions. The side-effect on `f` is what matters.
    let t3 = Instant::now();
    T_STAGE[2].fetch_add(
        t3.duration_since(t2).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Span-fill for Mod rows that need subword diff spans.
    let fill_spans = |h: &mut crate::parse::Hunk| {
        for r in &mut h.rows {
            if let Row::Mod {
                old_line,
                new_line,
                spans,
                cached_runs,
                ..
            } = r
                && old_line.text != new_line.text
                && spans.is_empty()
            {
                let (s, runs) =
                    word_spans_and_runs_with_profile(&old_line.text, &new_line.text, profile);
                *spans = s;
                *cached_runs = runs;
            }
        }
    };
    if f.hunks.len() > 1 {
        f.hunks.par_iter_mut().for_each(fill_spans);
    } else {
        f.hunks.iter_mut().for_each(fill_spans);
    }
    let t4 = Instant::now();
    T_STAGE[3].fetch_add(
        t4.duration_since(t3).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Invariant: DiffFile hunks must never contain wholesale replacement Mod
    // rows.
    crate::invariant!(
        f.hunks.iter().all(|h| h
            .rows
            .iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !is_wholesale_replacement(r))),
        "DiffFile hunks must never contain wholesale replacement Mod rows"
    );

    // Final step: normalize the carried context to the requested visual width.
    crate::shift::normalize_context(&mut f, opts.visual_context);

    let has_snapshots = new_content.is_some() || old_content.is_some();
    if has_snapshots && let Some(db) = &opts.notes_db {
        synthesize_note_context_hunks(
            &mut f,
            db,
            opts.target_repo_key.as_deref(),
            opts.target_rev_key.as_deref(),
            opts.visual_context,
        );
    }

    let is_plain_common_context = !f.is_binary
        && (f.status == crate::parse::FileStatus::Renamed
            || f.status == crate::parse::FileStatus::ModeChanged
            || f.status == crate::parse::FileStatus::Unmodified)
        && f.hunks.is_empty()
        && !new_lines.is_empty();

    if is_plain_common_context {
        let hunk = synthesize_plain_common_context_hunk(&new_lines);
        f.hunks = vec![hunk.clone()];
        f.raw_hunks = vec![hunk];
    } else if !f.is_binary
        && f.status == crate::parse::FileStatus::Deleted
        && f.hunks.is_empty()
        && !old_lines.is_empty()
    {
        let mut del_rows = Vec::with_capacity(old_lines.len());
        for (i, line) in old_lines.iter().enumerate() {
            let pos = (i + 1) as u32;
            del_rows.push(crate::parse::Row::Del(crate::parse::DelLine::new(
                pos, *line,
            )));
        }
        let count = del_rows.len() as u32;
        let hunk = crate::parse::Hunk {
            old_start: 1,
            old_count: count,
            new_start: 0,
            new_count: 0,
            section: String::new(),
            rows: del_rows,
            above_ctx: 0,
            below_ctx: 0,
        };
        f.hunks = vec![hunk.clone()];
        f.raw_hunks = vec![hunk];
    }

    let mut counts = file_counts(&f);
    let regions = fence_regions(&new_lines, profile);
    let t5 = Instant::now();
    T_STAGE[4].fetch_add(
        t5.duration_since(t4).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    let t_hunks_start = Instant::now();
    let hunks_out: Vec<OutputHunk> = if is_plain_common_context {
        let rows = render_plain_common_context(&new_lines, profile);
        vec![OutputHunk {
            old_start: 1,
            old_count: rows.len() as u32,
            new_start: 1,
            new_count: rows.len() as u32,
            rows,
        }]
    } else {
        let map_hunk = |h: &crate::parse::Hunk| {
            let resolve_next_line = |i: usize, r: &Row| {
                r.new_line()
                    .and_then(|nl| new_lines.get(nl as usize).copied())
                    .or_else(|| {
                        h.rows[i + 1..]
                            .iter()
                            .find(|nr| nr.new_line().is_some())
                            .map(|nr| nr.text())
                    })
                    .or_else(|| h.rows.get(i + 1).map(|nr| nr.text()))
            };
            let rows_out: Vec<OutputRow> = if h.rows.len() > 64 {
                h.rows
                    .par_iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let next_line = resolve_next_line(i, r);
                        build_hunk_output_row(r, next_line, profile, &regions)
                    })
                    .collect()
            } else {
                h.rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let next_line = resolve_next_line(i, r);
                        build_hunk_output_row(r, next_line, profile, &regions)
                    })
                    .collect()
            };
            OutputHunk {
                old_start: h.old_start,
                old_count: h.old_count,
                new_start: h.new_start,
                new_count: h.new_count,
                rows: rows_out,
            }
        };
        if f.hunks.len() > 1 {
            f.hunks.par_iter().map(map_hunk).collect()
        } else {
            f.hunks.iter().map(map_hunk).collect()
        }
    };
    let t_hunks_end = Instant::now();
    T_RENDER_HUNKS.fetch_add(
        t_hunks_end.duration_since(t_hunks_start).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    let mut raw_hunks_out = Vec::new();
    for h in &f.raw_hunks {
        let mut rows_out = Vec::new();
        for r in &h.rows {
            let runs = match r.kind() {
                RowKind::Ctx => vec![OutputRun::new(RunKind::Ctx, r.text().to_string())],
                RowKind::Add => vec![OutputRun::new(RunKind::Add, r.text().to_string())],
                RowKind::Del => vec![OutputRun::new(RunKind::Del, r.text().to_string())],
                RowKind::Mod => {
                    let mut runs = Vec::new();
                    if let Some(del) = r.old_text() {
                        runs.push(OutputRun::new(RunKind::Del, del.to_string()));
                    }
                    runs.push(OutputRun::new(RunKind::Add, r.text().to_string()));
                    runs
                }
                _ => Vec::new(),
            };
            rows_out.push(OutputRow {
                kind: r.kind(),
                mov_side: None,
                new_line: r.new_line(),
                old_line: r.old_line(),
                runs,
                wholesale: false,
                group: r.group(),
            });
        }
        raw_hunks_out.push(OutputHunk {
            old_start: h.old_start,
            old_count: h.old_count,
            new_start: h.new_start,
            new_count: h.new_count,
            rows: rows_out,
        });
    }

    let raw_notes = match &opts.notes_db {
        Some(db) => query_notes_for_file(
            db,
            opts.target_repo_key.as_deref(),
            opts.target_rev_key.as_deref(),
            &f.path,
        ),
        None => Vec::new(),
    };

    let mut open_notes = 0;
    let mut in_progress_notes = 0;
    let mut resolved_notes = 0;
    for n in &raw_notes {
        match n.status {
            NoteStatus::Open => open_notes += 1,
            NoteStatus::InProgress => in_progress_notes += 1,
            NoteStatus::Resolved => resolved_notes += 1,
        }
    }

    let notes_out: Vec<OutputNote> = raw_notes
        .into_iter()
        .filter(|n| opts.show_resolved || !n.status.is_resolved())
        .map(|n| OutputNote {
            id: n.id.clone(),
            file: n.file.clone(),
            location: n.location,
            text: n.text.clone(),
            created_at: n.created_at,
            runs: note_runs(&n.text),
            content_hash: n.content_hash,
            status: n.status,
            original_source_rows: n.original_source_rows.clone(),
        })
        .collect();

    let status = f.status;

    let t_rows_start = Instant::now();
    let rows: Vec<OutputRow> = if is_plain_common_context {
        hunks_out[0].rows.clone()
    } else {
        let total_rows_count: usize = hunks_out.iter().map(|h| h.rows.len()).sum();
        let mut rows: Vec<OutputRow> = Vec::with_capacity(total_rows_count);
        for (h, h_out) in f.hunks.iter().zip(&hunks_out) {
            let hunk_has_dels = h
                .rows
                .iter()
                .any(|r| matches!(r, Row::Del(_) | Row::MovSrc { .. } | Row::AuxDel { .. }));
            let hunk_has_adds = crate::visible::hunk_has_adds(h);
            let is_pure_context = !hunk_has_adds && !hunk_has_dels;
            let side = if hunk_has_adds || is_pure_context {
                VisibleSide::New
            } else {
                VisibleSide::Old
            };
            for (i, (r, r_out)) in h.rows.iter().zip(&h_out.rows).enumerate() {
                if matches!(r, Row::Ctx(_)) {
                    let mut row = r_out.clone();
                    if !is_pure_context {
                        match side {
                            VisibleSide::New => {
                                // Keep old_line intact for dual-gutter
                                // rendering. In
                                // single-gutter mode, new_line.or(old_line)
                                // still
                                // yields new_line.
                            }
                            VisibleSide::Old => {
                                row.new_line = None;
                                let next_line = r
                                    .old_line()
                                    .and_then(|ol| old_lines.get(ol as usize).copied())
                                    .or_else(|| {
                                        h.rows[i + 1..]
                                            .iter()
                                            .find(|nr| nr.old_line().is_some())
                                            .map(|nr| nr.text())
                                    })
                                    .or_else(|| h.rows.get(i + 1).map(|nr| nr.text()));
                                let mut runs = Vec::new();
                                for_each_ctx_line_token(
                                    r.text(),
                                    None,
                                    next_line,
                                    profile,
                                    &regions,
                                    |tk| {
                                        runs.push(OutputRun {
                                            kind: RunKind::Ctx,
                                            color: tk.color,
                                            sgr: tk.sgr,
                                            text: token_cow(tk.text),
                                        });
                                    },
                                );
                                row.runs = runs;
                            }
                        }
                    }
                    rows.push(row);
                } else {
                    rows.push(r_out.clone());
                }
            }
        }
        rows
    };
    let t_rows_end = Instant::now();
    T_RENDER_ROWS.fetch_add(
        t_rows_end.duration_since(t_rows_start).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    tracing::debug!(
        path = %f.path,
        hunk_count = hunks_out.len(),
        row_count = rows.len(),
        "Finished processing diff file"
    );

    counts.notes = open_notes;
    counts.in_progress_notes = in_progress_notes;
    counts.resolved_notes = resolved_notes;

    let res = OutputFile {
        path: f.path.clone(),
        old_path: f.old_path.clone(),
        old_mode: f.old_mode.clone(),
        new_mode: f.new_mode.clone(),
        status,
        is_binary: f.is_binary,
        old_bytes: f.old_bytes,
        new_bytes: f.new_bytes,
        new_line_count: f.new_line_count,
        old_line_count: f.old_line_count,
        new_content: mem::take(&mut f.new_content),
        old_content: mem::take(&mut f.old_content),
        hunks: hunks_out,
        raw_hunks: raw_hunks_out,
        counts,
        fences: regions
            .iter()
            .map(|r| {
                let raw_lang = r.lang.as_deref().unwrap_or_default();
                let norm = fence_lang_ext(raw_lang);
                let lang = if lang_profile(norm).is_some() {
                    norm.to_string()
                } else {
                    String::new()
                };
                OutputFence {
                    start: r.start,
                    end: r.end,
                    lang,
                }
            })
            .collect(),
        notes: notes_out,
        content_runs: Vec::new(),
        rows,
        raw_source,
    };
    let t6 = Instant::now();
    T_STAGE[5].fetch_add(
        t6.duration_since(t5).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    crate::ensure!(!res.path.is_empty(), "OutputFile path must not be empty");
    crate::ensure!(
        !res.raw_hunks.is_empty()
            || res.status == "renamed"
            || res.status == "untracked"
            || res.status == "deleted"
            || res.status == "mode changed"
            || res.status == "unmodified"
            || res.is_binary,
        "OutputFile must contain raw hunks or be renamed/untracked/deleted/mode changed/unmodified/binary: path='{}', status='{}', is_binary={}",
        res.path,
        res.status,
        res.is_binary
    );
    crate::ensure!(
        f.hunks.iter().all(|h| h
            .rows
            .iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !is_wholesale_replacement(r))),
        "DiffFile hunks must not contain wholesale replacement Mod rows"
    );
    crate::ensure!(
        res.validate_invariants().is_ok(),
        "OutputFile invariants violated for {}: {:?}",
        res.path,
        res.validate_invariants().err()
    );

    res
}

/// Render a file diff on demand from a [`crate::wire::RawFileSource`].
pub fn render_file_from_source(
    source: &crate::wire::RawFileSource,
    repo_root: Option<&Path>,
    git_revs: &[String],
    context: usize,
    opts: &PipelineOptions,
) -> Option<OutputFile> {
    let active_opts = opts.clone().with_lazy_collapsed(false);
    match source {
        crate::wire::RawFileSource::Git {
            path,
            old_path,
            status,
        } => {
            if let Some(root) = repo_root {
                crate::git::load_single_file_diff(
                    root,
                    git_revs,
                    path,
                    old_path.as_deref(),
                    status,
                    context,
                    &active_opts,
                )
            } else {
                None
            }
        }
        crate::wire::RawFileSource::Raw {
            path,
            diff,
            new_content,
            old_content,
        } => {
            let parsed = crate::parse::parse_diff(diff);
            let f = parsed
                .into_iter()
                .find(|item| &item.path == path || item.old_path.as_ref() == Some(path))?;
            let mut rendered = process_single_diff_file(
                f,
                new_content.as_deref(),
                old_content.as_deref(),
                Some(Arc::clone(diff)),
                &active_opts,
            );
            rendered.raw_source = Some(source.clone());
            Some(rendered)
        }
    }
}
