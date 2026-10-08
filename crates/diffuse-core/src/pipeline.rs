//! The Diffuse Core Engine Pipeline Orchestrator.
//!
//! Transforms unified diffs and source buffers into structured,
//! syntax-classified token runs and wire models through a lean orchestrator:
//!
//! $$\text{Parse} \longrightarrow \text{Intern} \longrightarrow
//! \text{Algorithmic Optimization} \longrightarrow \text{Wire/Render}$$
//!
//! 1. Diff parsing & content reconstruction (`src/parse/`,
//!    `src/old_content.rs`)
//! 2. Symbol interning & token indexing (`src/algo/interner.rs`,
//!    `src/similarity.rs`)
//! 3. Algorithmic sequence optimization & cluster transformations:
//!    - Bipartite move relocation (`src/algo/bipartite_matching.rs`,
//!      `src/moves.rs`)
//!    - Container Dyck balancing & phrase landmarks (`src/algo/dyck.rs`,
//!      `src/algo/landmarks.rs`, `src/token.rs`)
//!    - 2D multi-objective dynamic programming & collapse
//!      (`src/algo/block_dp.rs`, `src/cluster/`, `src/collapse.rs`)
//! 4. Fence syntax region detection & classification (`src/syntax/`)
//! 5. Wire contract projection & streaming (`src/wire/`, `src/streaming.rs`)

use std::{
    borrow::Cow,
    collections::HashMap,
    fs, mem,
    num::NonZero,
    path::Path,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
    time::Instant,
};

use crate::{
    collapse::collapse_hunk_rows,
    counts::file_counts,
    lcs::word_spans_and_runs_with_profile,
    merge::{RunKind, is_wholesale_mod, is_wholesale_replacement, merged_runs_with_profile},
    moves::mark_moves_with_profile,
    notes::{NotesDB, ReviewNote, Side, query_notes_for_file},
    parse::{CtxLine, DiffFile, Hunk, LineNum, Row, RowKind},
    syntax::{
        classifier::{
            ClassifiedToken, for_each_classified_token, for_each_classified_token_with_header,
            for_each_line_classified_token_with_next, for_each_markdown_token_with_header,
            is_line_alignment_row,
        },
        color::TokenColor,
        fence::{FenceRegion, fence_regions, region_at},
        profile::LangProfile,
        registry::{fence_lang_ext, lang_profile},
    },
    visible::Side as VisibleSide,
    wire::{
        MovSide, OutputFence, OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun,
        PipelineOutput,
    },
};

/// Options and configuration controlling the pipeline execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOptions {
    pub notes_db: Option<NotesDB>,
    pub repo: Option<String>,
    pub visual_context: usize,
    pub expand_lines: usize,
    pub token_product_cap: usize,
    pub cluster_product_cap: usize,
    pub cluster_bandwidth_min: usize,
    pub cluster_bandwidth_slack: usize,
    pub lazy_collapsed: bool,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            notes_db: None,
            repo: None,
            visual_context: crate::git::DEFAULT_CONTEXT_LINES,
            expand_lines: crate::git::DEFAULT_EXPAND_LINES,
            token_product_cap: crate::lcs::DEFAULT_TOKEN_PRODUCT_CAP,
            cluster_product_cap: crate::cluster::dp::DEFAULT_CLUSTER_PRODUCT_CAP,
            cluster_bandwidth_min: crate::cluster::dp::DEFAULT_CLUSTER_BANDWIDTH_MIN,
            cluster_bandwidth_slack: crate::cluster::dp::DEFAULT_CLUSTER_BANDWIDTH_SLACK,
            lazy_collapsed: false,
        }
    }
}

impl PipelineOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_notes(mut self, notes_db: Option<NotesDB>, repo: Option<String>) -> Self {
        self.notes_db = notes_db;
        self.repo = repo;
        self
    }

    pub fn with_repo(mut self, repo: Option<String>) -> Self {
        self.repo = repo;
        self
    }

    pub fn with_visual_context(mut self, visual_context: usize) -> Self {
        self.visual_context = visual_context;
        self
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
        self
    }

    pub fn with_token_product_cap(mut self, token_product_cap: usize) -> Self {
        self.token_product_cap = token_product_cap;
        self
    }

    pub fn with_cluster_product_cap(mut self, cluster_product_cap: usize) -> Self {
        self.cluster_product_cap = cluster_product_cap;
        self
    }

    pub fn with_cluster_bandwidth_min(mut self, min: usize) -> Self {
        self.cluster_bandwidth_min = min;
        self
    }

    pub fn with_cluster_bandwidth_slack(mut self, slack: usize) -> Self {
        self.cluster_bandwidth_slack = slack;
        self
    }

    pub fn with_lazy_collapsed(mut self, lazy_collapsed: bool) -> Self {
        self.lazy_collapsed = lazy_collapsed;
        self
    }
}

/// Builder for diff pipeline inputs: diff text, optional new/old content
/// buffers, and pipeline options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineInput {
    pub diff: String,
    pub new_content: Option<HashMap<String, String>>,
    pub old_content: Option<HashMap<String, String>>,
    pub options: PipelineOptions,
}

impl PipelineInput {
    pub fn new(diff: impl Into<String>) -> Self {
        Self {
            diff: diff.into(),
            new_content: None,
            old_content: None,
            options: PipelineOptions::default(),
        }
    }

    pub fn with_new_content(mut self, new_content: HashMap<String, String>) -> Self {
        self.new_content = Some(new_content);
        self
    }

    pub fn with_old_content(mut self, old_content: HashMap<String, String>) -> Self {
        self.old_content = Some(old_content);
        self
    }

    pub fn with_options(mut self, options: PipelineOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_notes(mut self, db: Option<NotesDB>, repo: Option<String>) -> Self {
        self.options.notes_db = db;
        self.options.repo = repo;
        self
    }

    pub fn with_visual_context(mut self, visual_context: usize) -> Self {
        self.options.visual_context = visual_context;
        self
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.options.expand_lines = expand_lines;
        self
    }

    /// Execute the full pipeline over the input.
    pub fn run(self) -> PipelineOutput {
        run_pipeline_full(self.diff, self.new_content, self.old_content, self.options)
    }
}

/// Run the pipeline over a diff + optional new-file contents; produce the
/// wire-contract output.
pub fn run_pipeline(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
) -> PipelineOutput {
    run_pipeline_with_notes(diff, new_content, None, None)
}

/// Run the pipeline over a diff with no file contents; produce the
/// wire-contract output.
pub fn run_pipeline_diff(diff: impl Into<String>) -> PipelineOutput {
    run_pipeline_full(diff, None, None, PipelineOptions::default())
}

/// Run the pipeline over a diff + optional new-file contents + optional notes;
/// produce the wire-contract output.
pub fn run_pipeline_with_notes(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    notes_db: Option<NotesDB>,
    repo: Option<String>,
) -> PipelineOutput {
    run_pipeline_full(
        diff,
        new_content,
        None,
        PipelineOptions::default().with_notes(notes_db, repo),
    )
}

/// Run the pipeline over Arc<String> inputs without string duplication.
#[tracing::instrument(level = "trace", ret, skip(diff, new_content, old_content, opts), fields(diff_bytes = diff.len(), new_file_count = new_content.as_ref().map(|m| m.len()), old_file_count = old_content.as_ref().map(|m| m.len()), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn run_pipeline_arc(
    diff: Arc<String>,
    new_content: Option<HashMap<String, Arc<String>>>,
    old_content: Option<HashMap<String, Arc<String>>>,
    opts: PipelineOptions,
) -> PipelineOutput {
    let out_files =
        crate::streaming::stream_diff_pipeline_arc(diff, new_content, old_content, opts, None)
            .collect();

    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

/// Run the pipeline over a diff + optional new-file contents + optional
/// old-file contents + pipeline options; produce the wire-contract output.
#[tracing::instrument(level = "trace", ret, skip(diff, new_content, old_content, opts), fields(new_file_count = new_content.as_ref().map(|m| m.len()), old_file_count = old_content.as_ref().map(|m| m.len()), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn run_pipeline_full(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    old_content: Option<HashMap<String, String>>,
    opts: PipelineOptions,
) -> PipelineOutput {
    crate::lcs::set_token_product_cap(opts.token_product_cap);
    crate::cluster::dp::set_cluster_product_cap(opts.cluster_product_cap);
    crate::cluster::dp::set_cluster_bandwidth_min(opts.cluster_bandwidth_min);
    crate::cluster::dp::set_cluster_bandwidth_slack(opts.cluster_bandwidth_slack);
    let out_files: Vec<OutputFile> =
        crate::streaming::stream_diff_pipeline(diff, new_content, old_content, opts, None)
            .collect();

    tracing::debug!(file_count = out_files.len(), "Completed diff pipeline");

    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

fn split_content_lines(s: &str) -> Vec<&str> {
    let stripped = s.strip_suffix('\r').unwrap_or(s);
    let stripped = stripped.strip_suffix('\n').unwrap_or(stripped);
    let stripped = stripped.strip_suffix('\r').unwrap_or(stripped);
    stripped
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

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
    let raw_source = if let Some(fb) = diff_fallback {
        Some(crate::wire::RawFileSource::Raw {
            path: f.path.clone(),
            diff: fb,
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
            crate::parse::FileStatus::Note => "note".to_string(),
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
                dels: 0,
                mods: 0,
                movs: 0,
                notes: 0,
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
                adds: 0,
                dels: line_count,
                mods: 0,
                movs: 0,
                notes: 0,
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
    let profile = if f.status == crate::parse::FileStatus::Note {
        lang_profile("md")
    } else {
        let ext = f.path.rsplit('.').next().unwrap_or("");
        lang_profile(ext)
    };
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

    let new_bytes: Option<usize> = if f.status == crate::parse::FileStatus::Deleted {
        None
    } else if let Some(b) = f.new_bytes {
        Some(b)
    } else if let Some(c) = new_content {
        Some(c.len())
    } else if f.is_binary {
        if let Ok(meta) = fs::metadata(&f.path) {
            Some(meta.len() as usize)
        } else if let Some(ref r) = opts.repo
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
    mark_moves_with_profile(&mut f, profile);
    let t3 = Instant::now();
    T_STAGE[2].fetch_add(
        t3.duration_since(t2).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Span-fill for Mov rows that need subword diff spans.
    let fill_spans = |h: &mut crate::parse::Hunk| {
        for r in &mut h.rows {
            if let Row::MovTgt(a) = r
                && let Some(ref dt) = a.del_text
                && dt.as_str() != a.text
                && a.spans.is_empty()
            {
                let (spans, runs) = word_spans_and_runs_with_profile(dt, &a.text, profile);
                a.spans = spans;
                a.cached_runs = runs;
            } else if let Row::Mod { old_line, new_line } = r
                && old_line.text != new_line.text
                && new_line.spans.is_empty()
            {
                let (spans, runs) =
                    word_spans_and_runs_with_profile(&old_line.text, &new_line.text, profile);
                new_line.spans = spans;
                new_line.cached_runs = runs;
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
        synthesize_note_context_hunks(&mut f, db, opts.repo.as_deref(), opts.visual_context);
    }

    let is_plain_common_context = !f.is_binary
        && (f.status == crate::parse::FileStatus::Renamed
            || f.status == crate::parse::FileStatus::ModeChanged
            || f.status == crate::parse::FileStatus::Note)
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
            if let Some(pos) = NonZero::new((i + 1) as u32) {
                del_rows.push(crate::parse::Row::Del(crate::parse::DelLine::new(
                    pos, *line,
                )));
            }
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
                    if let Some(del) = r.del_text() {
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

    let notes_out = match &opts.notes_db {
        Some(db) => query_notes_for_file(db, opts.repo.as_deref(), &f.path)
            .into_iter()
            .map(|n| OutputNote {
                id: n.id.clone(),
                file: n.file.clone(),
                side: n.side,
                start: n.start,
                end: n.end,
                text: n.text.clone(),
                created_at: n.created_at,
                runs: note_runs(&n.text),
                line_hash: n.line_hash,
            })
            .collect(),
        None => Vec::new(),
    };

    let status = match f.status {
        crate::parse::FileStatus::Added => "added".to_string(),
        crate::parse::FileStatus::Deleted => "deleted".to_string(),
        crate::parse::FileStatus::Modified => "modified".to_string(),
        crate::parse::FileStatus::Renamed => "renamed".to_string(),
        crate::parse::FileStatus::Untracked => "untracked".to_string(),
        crate::parse::FileStatus::ModeChanged => "mode changed".to_string(),
        crate::parse::FileStatus::Note => "note".to_string(),
    };

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
                .any(|r| matches!(r, Row::Del(_) | Row::MovSrc(_)));
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
                                row.old_line = None;
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

    counts.notes = notes_out.len();

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
            || res.status == "note"
            || res.is_binary,
        "OutputFile must contain raw hunks or be renamed/untracked/deleted/mode changed/note/binary"
    );
    crate::ensure!(
        f.hunks.iter().all(|h| h
            .rows
            .iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !is_wholesale_replacement(r))),
        "DiffFile hunks must not contain wholesale replacement Mod rows"
    );

    res
}

/// Render a file diff on demand from a [`crate::wire::RawFileSource`].
pub fn render_file_from_source(
    source: &crate::wire::RawFileSource,
    repo_root: Option<&Path>,
    git_args: &[String],
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
                    git_args,
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

#[inline]
pub fn token_cow(s: &str) -> Cow<'static, str> {
    match s {
        " " => Cow::Borrowed(" "),
        "  " => Cow::Borrowed("  "),
        "   " => Cow::Borrowed("   "),
        "    " => Cow::Borrowed("    "),
        "        " => Cow::Borrowed("        "),
        "(" => Cow::Borrowed("("),
        ")" => Cow::Borrowed(")"),
        "{" => Cow::Borrowed("{"),
        "}" => Cow::Borrowed("}"),
        "[" => Cow::Borrowed("["),
        "]" => Cow::Borrowed("]"),
        ";" => Cow::Borrowed(";"),
        "," => Cow::Borrowed(","),
        ":" => Cow::Borrowed(":"),
        "::" => Cow::Borrowed("::"),
        "." => Cow::Borrowed("."),
        "=" => Cow::Borrowed("="),
        "==" => Cow::Borrowed("=="),
        "!=" => Cow::Borrowed("!="),
        "<" => Cow::Borrowed("<"),
        ">" => Cow::Borrowed(">"),
        "+" => Cow::Borrowed("+"),
        "-" => Cow::Borrowed("-"),
        "*" => Cow::Borrowed("*"),
        "/" => Cow::Borrowed("/"),
        "&" => Cow::Borrowed("&"),
        "|" => Cow::Borrowed("|"),
        "!" => Cow::Borrowed("!"),
        "?" => Cow::Borrowed("?"),
        "->" => Cow::Borrowed("->"),
        "=>" => Cow::Borrowed("=>"),
        "\n" => Cow::Borrowed("\n"),
        "" => Cow::Borrowed(""),
        _ => Cow::Owned(s.to_string()),
    }
}

/// The merged runs with the semantic syntax colors on the common/add runs.
pub fn colored_runs(
    r: &Row,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<OutputRun> {
    colored_runs_with_next(r, None, profile, regions)
}

pub fn colored_runs_with_next(
    r: &Row,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    let new_line = r.new_line();
    let region = new_line.and_then(|n| region_at(regions, n));

    let use_merged = (matches!(r, Row::Mod { .. })
        || (r.is_moved() && r.del_text() != Some(r.text()))
        || (matches!(r, Row::Add(_)) && r.del_text().is_some()))
        && r.del_text().is_some();

    if !use_merged {
        if matches!(r, Row::Add(_) | Row::MovTgt(_) | Row::Ctx(_)) {
            let kind = if matches!(r, Row::Add(_) | Row::MovTgt(_)) {
                RunKind::Add
            } else {
                RunKind::Ctx
            };
            for_each_ctx_line_token(r.text(), new_line, next_line, profile, regions, |t| {
                out.push(OutputRun {
                    kind,
                    color: t.color,
                    sgr: t.sgr,
                    text: token_cow(t.text),
                });
            });
        } else {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: TokenColor::Default,
                sgr: None,
                text: token_cow(r.text()),
            });
        }
        return out;
    }

    thread_local! {
        static CTX_SCRATCH: std::cell::RefCell<Vec<ClassifiedToken<'static>>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    CTX_SCRATCH.with(|cell| {
        let mut full_tokens = cell.borrow_mut();
        full_tokens.clear();
        for_each_ctx_line_token(r.text(), new_line, next_line, profile, regions, |t| {
            // Safety: tokens are only accessed within this block while r.text()
            // is alive.
            let tok_static =
                unsafe { mem::transmute::<ClassifiedToken<'_>, ClassifiedToken<'static>>(t) };
            full_tokens.push(tok_static);
        });

        let mut add_pos = 0usize;
        for run in merged_runs_with_profile(r, profile) {
            if run.kind == RunKind::Del {
                push_output_run(
                    &mut out,
                    OutputRun {
                        kind: RunKind::Del,
                        color: TokenColor::Default,
                        sgr: None,
                        text: token_cow(run.text),
                    },
                );
            } else {
                let run_len = if run.text.is_ascii() {
                    run.text.len()
                } else {
                    run.text.chars().count()
                };
                let end_pos = add_pos + run_len;
                let mut had_frags = false;
                for_each_sliced_token(&full_tokens, add_pos, end_pos, |t| {
                    had_frags = true;
                    push_output_run(
                        &mut out,
                        OutputRun {
                            kind: run.kind,
                            color: t.color,
                            sgr: t.sgr,
                            text: token_cow(t.text),
                        },
                    );
                });
                add_pos = end_pos;
                if !had_frags {
                    let push_t = |t: ClassifiedToken| {
                        push_output_run(
                            &mut out,
                            OutputRun {
                                kind: run.kind,
                                color: t.color,
                                sgr: t.sgr,
                                text: token_cow(t.text),
                            },
                        );
                    };
                    match region {
                        Some(rg) => for_each_classified_token(
                            run.text,
                            rg.lang.as_deref().and_then(lang_profile),
                            push_t,
                        ),
                        None => {
                            if profile.is_some_and(|p| p.markdown) {
                                for_each_markdown_token_with_header(
                                    run.text,
                                    next_line.is_some_and(is_line_alignment_row),
                                    push_t,
                                );
                            } else {
                                for_each_classified_token_with_header(
                                    run.text,
                                    profile,
                                    next_line.is_some_and(is_line_alignment_row),
                                    push_t,
                                );
                            }
                        }
                    }
                }
            }
        }
        full_tokens.clear();
    });
    out
}

#[inline(always)]
fn push_output_run(out: &mut Vec<OutputRun>, run: OutputRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.kind == run.kind
        && last.color == run.color
        && last.sgr == run.sgr
    {
        last.text.to_mut().push_str(&run.text);
        return;
    }
    out.push(run);
}

fn for_each_ctx_line_token<'a>(
    text: &'a str,
    new_line: Option<u32>,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    let is_delimiter = if regions.is_empty() {
        false
    } else if let Some(n) = new_line {
        let idx = regions.partition_point(|rg| rg.end + 1 < n);
        (idx < regions.len()
            && (n + 1 == regions[idx].start || (n > 0 && n - 1 == regions[idx].end)))
            || (idx + 1 < regions.len()
                && (n + 1 == regions[idx + 1].start || (n > 0 && n - 1 == regions[idx + 1].end)))
    } else {
        false
    };
    if is_delimiter {
        if let Some(strip) = profile.and_then(|p| p.fence_strip.as_ref())
            && let Some(m) = strip.find(text)
            && m.start() == 0
            && m.end() > 0
        {
            f(ClassifiedToken {
                color: TokenColor::Comment,
                sgr: None,
                text: &text[..m.end()],
            });
            f(ClassifiedToken {
                color: TokenColor::FenceBorder,
                sgr: None,
                text: &text[m.end()..],
            });
            return;
        }
        f(ClassifiedToken {
            color: TokenColor::FenceBorder,
            sgr: None,
            text,
        });
        return;
    }
    let region = new_line.and_then(|n| region_at(regions, n));
    for_each_line_classified_token_with_next(text, next_line, profile, region, f);
}

#[cfg(test)]
fn classify_ctx_line<'a>(
    text: &'a str,
    new_line: Option<u32>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_ctx_line_token(text, new_line, None, profile, regions, |t| out.push(t));
    out
}

fn for_each_sliced_token<'a>(
    tokens: &[ClassifiedToken<'a>],
    start_char: usize,
    end_char: usize,
    mut f: impl FnMut(ClassifiedToken<'a>),
) -> bool {
    let mut found = false;
    let mut pos = 0;
    for tok in tokens {
        let tok_len = if tok.text.is_ascii() {
            tok.text.len()
        } else {
            tok.text.chars().count()
        };
        let tok_start = pos;
        let tok_end = pos + tok_len;
        pos = tok_end;
        if tok_end <= start_char || tok_start >= end_char {
            continue;
        }
        let s = start_char.saturating_sub(tok_start);
        let e = end_char.min(tok_end) - tok_start;
        let piece: &'a str = if tok.text.is_ascii() {
            &tok.text[s..e]
        } else {
            crate::lcs::char_slice(tok.text, s, e)
        };
        if !piece.is_empty() {
            found = true;
            f(ClassifiedToken {
                color: tok.color,
                sgr: tok.sgr,
                text: piece,
            });
        }
    }
    found
}

/// Tokenizes and classifies a single context line into styled `OutputRun`s
/// based on file extension and line number.
pub fn context_line_runs(line_text: &str, line_num: Option<u32>, ext: &str) -> Vec<OutputRun> {
    let profile = lang_profile(ext);
    let mut runs = Vec::new();
    for_each_ctx_line_token(line_text, line_num, None, profile, &[], |tk| {
        runs.push(OutputRun {
            kind: crate::merge::RunKind::Ctx,
            color: tk.color,
            sgr: tk.sgr,
            text: tk.text.to_string().into(),
        });
    });
    runs
}

fn build_hunk_output_row(
    r: &Row,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> OutputRow {
    let (kind, mov_side, new_line, old_line) = match r {
        Row::MovSrc(del) => (RowKind::Mov, Some(MovSide::Src), None, Some(del.num.get())),
        Row::MovTgt(add) => (RowKind::Mov, Some(MovSide::Tgt), Some(add.num.get()), None),
        _ => (r.kind(), None, r.new_line(), r.old_line()),
    };
    OutputRow {
        kind,
        mov_side,
        new_line,
        old_line,
        runs: colored_runs_with_next(r, next_line, profile, regions),
        wholesale: matches!(r, Row::Mod { .. }) && is_wholesale_mod(r),
        group: r.group(),
    }
}

/// Synthesizes context hunks around review notes that fall into gaps between
/// diff hunks or outside existing diff hunks.
pub fn synthesize_note_context_hunks(
    f: &mut DiffFile,
    notes_db: &NotesDB,
    repo: Option<&str>,
    visual_context: usize,
) {
    if f.is_binary || f.status == crate::parse::FileStatus::Deleted {
        return;
    }
    let notes = query_notes_for_file(notes_db, repo, &f.path);
    if notes.is_empty() {
        return;
    }

    let n = visual_context as u32;

    // 1. Identify notes whose target span [start..=end] is uncovered by existing
    //    diff hunks.
    let uncovered: Vec<&ReviewNote> = notes
        .into_iter()
        .filter(|note| {
            !f.hunks.iter().any(|h| match note.side {
                Side::New => {
                    let h_start = h.new_start;
                    let h_end = h.new_start + h.new_count.saturating_sub(1);
                    h.new_count > 0 && note.start >= h_start && note.end <= h_end
                }
                Side::Old => {
                    let h_start = h.old_start;
                    let h_end = h.old_start + h.old_count.saturating_sub(1);
                    h.old_count > 0 && note.start >= h_start && note.end <= h_end
                }
            })
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

    for note in uncovered {
        let (n_start, n_end) = (
            note.start.min(note.end).max(1),
            note.start.max(note.end).max(1),
        );

        let gap_idx = if m == 0 {
            0
        } else {
            match note.side {
                Side::New => {
                    let mut found = m;
                    for (i, h) in f.hunks.iter().enumerate() {
                        if n_start < h.new_start {
                            found = i;
                            break;
                        }
                    }
                    found
                }
                Side::Old => {
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

        let (new_s, new_e) = match note.side {
            Side::New => (n_start, n_end),
            Side::Old => {
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
                let old_pos = LineNum::new(ol).unwrap_or(LineNum::MIN);
                let new_pos = LineNum::new(nl).unwrap_or(LineNum::MIN);
                rows.push(Row::Ctx(CtxLine::new(old_pos, new_pos, text)));
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
        if let Some(pos) = NonZero::new((i + 1) as u32) {
            ctx_rows.push(crate::parse::Row::Ctx(crate::parse::CtxLine::new(
                pos, pos, *line,
            )));
        }
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
        status: crate::parse::FileStatus::Note,
        is_binary: false,
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

#[cfg(test)]
mod tests;
