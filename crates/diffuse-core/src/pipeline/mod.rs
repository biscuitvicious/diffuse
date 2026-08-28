//! The Diffuse Core Engine Pipeline Orchestrator.
//!
//! Transforms unified diffs and source buffers into structured,
//! syntax-classified token runs and wire models:
//!
//! 1. Diff parsing & content reconstruction (`src/parse/`,
//!    `src/old_content.rs`)
//! 2. Greedy file relocation & move marking (`src/moves/`)
//! 3. DP cluster transformation & collapse passes (`src/cluster/`,
//!    `src/collapse/`)
//! 4. Fence syntax region detection (`src/syntax/`)
//! 5. Semantic token classification & subword coloring (`src/syntax/`,
//!    `src/merge/`)

use std::collections::HashMap;

use crate::{
    collapse::collapse_hunk_rows,
    counts::file_counts,
    lcs::word_spans,
    merge::{RunKind, is_wholesale_mod, merged_runs},
    moves::mark_moves,
    notes::{NotesDB, note_runs, notes_for_file},
    parse::{DiffFile, Row},
    syntax::{
        classifier::{
            ClassifiedToken, for_each_classified_token, for_each_line_classified_token,
            for_each_markdown_token,
        },
        fence::{FenceRegion, fence_regions, region_at},
        profile::LangProfile,
        registry::{fence_lang_ext, lang_profile},
    },
    visible::{Side as VisibleSide, build_visible},
    wire::{OutputFence, OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun, PipelineOutput},
};

/// Options and configuration controlling the pipeline execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOptions {
    pub notes_db: Option<NotesDB>,
    pub repo: Option<String>,
    pub visual_context: usize,
    pub expand_lines: usize,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            notes_db: None,
            repo: None,
            visual_context: crate::git::DEFAULT_CONTEXT_LINES,
            expand_lines: crate::git::DEFAULT_EXPAND_LINES,
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

    pub fn with_visual_context(mut self, visual_context: usize) -> Self {
        self.visual_context = visual_context;
        self
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
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
pub fn run_pipeline_arc(
    diff: std::sync::Arc<String>,
    new_content: Option<HashMap<String, std::sync::Arc<String>>>,
    old_content: Option<HashMap<String, std::sync::Arc<String>>>,
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
pub fn run_pipeline_full(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    old_content: Option<HashMap<String, String>>,
    opts: PipelineOptions,
) -> PipelineOutput {
    let out_files =
        crate::streaming::stream_diff_pipeline(diff, new_content, old_content, opts, None)
            .collect();

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
    stripped.split('\n').collect()
}

/// Run pipeline diffing, alignment, move detection, and syntax classification
/// on a single parsed diff file using the specified pipeline options.
pub fn process_single_diff_file(
    mut f: DiffFile,
    new_content: Option<&str>,
    old_content: Option<&str>,
    _diff_fallback: Option<&str>,
    opts: &PipelineOptions,
) -> OutputFile {
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

    mark_moves(&mut f);

    f.hunks.iter_mut().for_each(|h| {
        h.rows = collapse_hunk_rows(
            h,
            Some(old_lines.as_slice()),
            Some(new_lines.as_slice()),
            profile,
        );
        for r in &mut h.rows {
            if let Row::Mov { old_line, new_line } = r
                && old_line.text != new_line.text
                && new_line.spans.is_empty()
            {
                new_line.spans = word_spans(&old_line.text, &new_line.text);
            }
        }
    });

    // Final step: normalize the carried context to the requested visual width.
    crate::shift::normalize_context(&mut f, opts.visual_context);

    let counts = file_counts(&f);
    let regions = fence_regions(&new_lines, profile);

    let hunks_out: Vec<OutputHunk> = f
        .hunks
        .iter()
        .map(|h| {
            let mut rows_out = Vec::with_capacity(h.rows.len());
            for r in &h.rows {
                rows_out.push(OutputRow {
                    kind: r.kind(),
                    new_line: r.new_line(),
                    old_line: if matches!(r, Row::Add(_) | Row::Mov { .. }) {
                        None
                    } else {
                        r.old_line()
                    },
                    text: r.text().to_string(),
                    del_text: r.del_text().map(String::from),
                    spans: r.spans().cloned().unwrap_or_default(),
                    runs: colored_runs(r, profile, &regions),
                    wholesale: matches!(r, Row::Mod { .. }) && is_wholesale_mod(r),
                    moved: r.is_moved(),
                });
            }
            OutputHunk {
                old_start: h.old_start,
                old_count: h.old_count,
                new_start: h.new_start,
                new_count: h.new_count,
                rows: rows_out,
            }
        })
        .collect();

    let mut raw_hunks_out = Vec::new();
    for h in &f.raw_hunks {
        let mut rows_out = Vec::new();
        for r in &h.rows {
            rows_out.push(OutputRow {
                kind: r.kind(),
                new_line: r.new_line(),
                old_line: r.old_line(),
                text: r.text().to_string(),
                del_text: r.del_text().map(String::from),
                spans: r.spans().cloned().unwrap_or_default(),
                runs: Vec::new(),
                wholesale: false,
                moved: false,
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

    let notes_out = match (&opts.notes_db, &opts.repo) {
        (Some(db), Some(r)) => notes_for_file(db, r, &f.path)
            .map(|n| OutputNote {
                id: n.id.clone(),
                file: n.file.clone(),
                side: n.side,
                start: n.start,
                end: n.end,
                text: n.text.clone(),
                created_at: n.created_at,
                runs: note_runs(&n.text),
            })
            .collect(),
        _ => Vec::new(),
    };

    let lines_for_runs = if !new_lines.is_empty() {
        &new_lines
    } else {
        &old_lines
    };
    let content_runs: Vec<Vec<OutputRun>> = lines_for_runs
        .iter()
        .enumerate()
        .map(|(li, line_text)| {
            let mut runs = Vec::new();
            for_each_ctx_line_token(line_text, Some((li + 1) as u32), profile, &regions, |tk| {
                runs.push(OutputRun {
                    kind: RunKind::Ctx,
                    color: Some(std::borrow::Cow::Borrowed(tk.color)),
                    sgr: tk.sgr,
                    text: tk.text.to_string().into(),
                });
            });
            runs
        })
        .collect();

    let status = match f.status {
        crate::parse::FileStatus::Added => "added".to_string(),
        crate::parse::FileStatus::Deleted => "deleted".to_string(),
        crate::parse::FileStatus::Modified => "modified".to_string(),
        crate::parse::FileStatus::Renamed => "renamed".to_string(),
        crate::parse::FileStatus::Untracked => "untracked".to_string(),
    };

    OutputFile {
        path: f.path.clone(),
        status,
        new_line_count: f.new_line_count,
        old_line_count: f.old_line_count,
        new_content: std::mem::take(&mut f.new_content),
        old_content: std::mem::take(&mut f.old_content),
        hunks: hunks_out,
        raw_hunks: raw_hunks_out,
        counts,
        fences: regions
            .iter()
            .map(|r| OutputFence {
                start: r.start,
                end: r.end,
                lang: fence_lang_ext(&r.lang.clone().unwrap_or_default()).to_string(),
            })
            .collect(),
        notes: notes_out,
        content_runs,
        rows: build_visible(&f.hunks, opts.expand_lines)
            .into_iter()
            .map(|vis| {
                let r = vis.row;
                let runs = if matches!(
                    r,
                    Row::Mod { .. } | Row::Add(_) | Row::Mov { .. } | Row::Del(_)
                ) {
                    colored_runs(&r, profile, &regions)
                } else {
                    let mut runs = Vec::new();
                    for_each_ctx_line_token(
                        r.text(),
                        match vis.side {
                            Some(VisibleSide::New) => r.new_line(),
                            Some(VisibleSide::Old) => None,
                            None => r.new_line().or_else(|| r.old_line()),
                        },
                        profile,
                        &regions,
                        |tk| {
                            runs.push(OutputRun {
                                kind: RunKind::Ctx,
                                color: Some(std::borrow::Cow::Borrowed(tk.color)),
                                sgr: tk.sgr,
                                text: tk.text.to_string().into(),
                            });
                        },
                    );
                    runs
                };
                let (new_line, old_line) = match (&r, vis.side) {
                    (Row::Ctx(_), Some(VisibleSide::New)) | (Row::Add(_) | Row::Mov { .. }, _) => {
                        (r.new_line(), None)
                    }
                    (Row::Ctx(_), Some(VisibleSide::Old)) | (Row::Del(_), _) => {
                        (None, r.old_line())
                    }
                    (Row::Ctx(_), None) | (Row::Mod { .. }, _) => (r.new_line(), r.old_line()),
                    (Row::Sep(_), _) => (None, None),
                };
                let wholesale = matches!(r, Row::Mod { .. }) && is_wholesale_mod(&r);
                OutputRow {
                    kind: r.kind(),
                    new_line,
                    old_line,
                    text: r.text().to_string(),
                    del_text: r.del_text().map(String::from),
                    spans: r.spans().cloned().unwrap_or_default(),
                    runs,
                    wholesale,
                    moved: r.is_moved(),
                }
            })
            .collect(),
    }
}

/// The merged runs with the semantic syntax colors on the common/add runs.
pub fn colored_runs(
    r: &Row,
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
        if matches!(r, Row::Add(_) | Row::Mov { .. } | Row::Ctx(_)) {
            let kind = if matches!(r, Row::Add(_) | Row::Mov { .. }) {
                RunKind::Add
            } else {
                RunKind::Ctx
            };
            for_each_ctx_line_token(r.text(), new_line, profile, regions, |t| {
                out.push(OutputRun {
                    kind,
                    color: Some(std::borrow::Cow::Borrowed(t.color)),
                    sgr: t.sgr,
                    text: t.text.to_string().into(),
                });
            });
        } else {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: None,
                sgr: None,
                text: r.text().to_string().into(),
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
        for_each_ctx_line_token(r.text(), new_line, profile, regions, |t| {
            // Safety: tokens are only accessed within this block while r.text() is alive.
            let tok_static =
                unsafe { std::mem::transmute::<ClassifiedToken<'_>, ClassifiedToken<'static>>(t) };
            full_tokens.push(tok_static);
        });

        let mut add_pos = 0usize;
        for run in merged_runs(r) {
            if run.kind == RunKind::Del {
                push_output_run(
                    &mut out,
                    OutputRun {
                        kind: RunKind::Del,
                        color: None,
                        sgr: None,
                        text: run.text.to_string().into(),
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
                            color: Some(std::borrow::Cow::Borrowed(t.color)),
                            sgr: t.sgr,
                            text: t.text.to_string().into(),
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
                                color: Some(std::borrow::Cow::Borrowed(t.color)),
                                sgr: t.sgr,
                                text: t.text.to_string().into(),
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
                                for_each_markdown_token(run.text, push_t)
                            } else {
                                for_each_classified_token(run.text, profile, push_t)
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
                color: "syntaxComment",
                sgr: None,
                text: &text[..m.end()],
            });
            f(ClassifiedToken {
                color: "muted",
                sgr: None,
                text: &text[m.end()..],
            });
            return;
        }
        f(ClassifiedToken {
            color: "muted",
            sgr: None,
            text,
        });
        return;
    }
    let region = new_line.and_then(|n| region_at(regions, n));
    for_each_line_classified_token(text, profile, region, f);
}

#[cfg(test)]
fn classify_ctx_line<'a>(
    text: &'a str,
    new_line: Option<u32>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_ctx_line_token(text, new_line, profile, regions, |t| out.push(t));
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
    for_each_ctx_line_token(line_text, line_num, profile, &[], |tk| {
        runs.push(OutputRun {
            kind: crate::merge::RunKind::Ctx,
            color: Some(std::borrow::Cow::Borrowed(tk.color)),
            sgr: tk.sgr,
            text: tk.text.to_string().into(),
        });
    });
    runs
}

#[cfg(test)]
mod tests;
