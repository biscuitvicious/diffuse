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
        classifier::{ClassifiedToken, classify_line, classify_tokens, markdown_tokens},
        fence::{FenceRegion, fence_regions, region_at},
        profile::LangProfile,
        registry::{fence_lang_ext, lang_profile},
    },
    visible::{Side as VisibleSide, build_visible},
    wire::{OutputFence, OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun, PipelineOutput},
};

/// Input configuration and content buffers for driving the diff pipeline.
pub struct PipelineContext<'a> {
    pub diff: &'a str,
    pub contents: &'a HashMap<String, String>,
    pub old_contents: Option<&'a HashMap<String, String>>,
    pub notes_db: Option<&'a NotesDB>,
    pub repo: Option<&'a str>,
}

impl<'a> PipelineContext<'a> {
    pub fn new(diff: &'a str, contents: &'a HashMap<String, String>) -> Self {
        Self {
            diff,
            contents,
            old_contents: None,
            notes_db: None,
            repo: None,
        }
    }

    pub fn with_old_contents(mut self, old: Option<&'a HashMap<String, String>>) -> Self {
        self.old_contents = old;
        self
    }

    pub fn with_notes(mut self, db: Option<&'a NotesDB>, repo: Option<&'a str>) -> Self {
        self.notes_db = db;
        self.repo = repo;
        self
    }

    /// Execute the full pipeline over the context.
    pub fn run(&self) -> PipelineOutput {
        run_pipeline_full(
            self.diff,
            self.contents,
            self.old_contents,
            self.notes_db,
            self.repo,
        )
    }
}

/// Run the pipeline over a diff + new-file contents; produce the wire-contract
/// output.
pub fn run_pipeline(diff: &str, contents: &HashMap<String, String>) -> PipelineOutput {
    run_pipeline_with_notes(diff, contents, None, None)
}

/// Run the pipeline over a diff + new-file contents + optional notes; produce
/// the wire-contract output.
pub fn run_pipeline_with_notes(
    diff: &str,
    contents: &HashMap<String, String>,
    notes_db: Option<&NotesDB>,
    repo: Option<&str>,
) -> PipelineOutput {
    run_pipeline_full(diff, contents, None, notes_db, repo)
}

/// Run the pipeline over a diff + new-file contents + optional old-file
/// contents + optional notes; produce the wire-contract output.
pub fn run_pipeline_full(
    diff: &str,
    contents: &HashMap<String, String>,
    old_contents: Option<&HashMap<String, String>>,
    notes_db: Option<&NotesDB>,
    repo: Option<&str>,
) -> PipelineOutput {
    let out_files = crate::streaming::stream_diff_pipeline(
        diff,
        contents,
        old_contents,
        notes_db.cloned(),
        repo.map(String::from),
        None,
    )
    .collect();

    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

/// Run pipeline diffing, alignment, move detection, and syntax classification
/// on a single parsed diff file.
pub fn process_single_diff_file(
    mut f: DiffFile,
    new_content: Option<&str>,
    old_content: Option<&str>,
    diff_fallback: Option<&str>,
    notes_db: Option<&NotesDB>,
    repo: Option<&str>,
) -> OutputFile {
    let ext = f.path.rsplit('.').next().unwrap_or("");
    let profile = lang_profile(ext);
    f.new_content = new_content
        .map(|c| c.split('\n').map(String::from).collect())
        .unwrap_or_default();
    f.new_line_count = f.new_content.len();

    if let Some(oc) = old_content
        && !oc.is_empty()
    {
        f.old_content = oc.split('\n').map(String::from).collect();
        f.old_line_count = f.old_content.len();
    } else if f.old_content.is_empty()
        && let Some(diff) = diff_fallback
    {
        f.old_content = crate::old_content::old_content_from_diff(diff, &f.path);
        f.old_line_count = f.old_content.len();
    }

    mark_moves(&mut f);

    for h in &mut f.hunks {
        h.rows = collapse_hunk_rows(
            h,
            Some(f.old_content.as_slice()),
            Some(f.new_content.as_slice()),
            profile,
        );
    }

    // Bake word spans for moved lines with indentation reflows
    for h in &mut f.hunks {
        for r in &mut h.rows {
            if let Row::Mov { old_line, new_line } = r
                && old_line.text != new_line.text
                && new_line.spans.is_empty()
            {
                new_line.spans = word_spans(&old_line.text, &new_line.text);
            }
        }
    }

    let counts = file_counts(&f);
    let regions = fence_regions(&f.new_content, profile);

    let mut hunks_out = Vec::new();
    for h in &f.hunks {
        let mut rows_out = Vec::new();
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
        hunks_out.push(OutputHunk {
            old_start: h.old_start,
            old_count: h.old_count,
            new_start: h.new_start,
            new_count: h.new_count,
            rows: rows_out,
        });
    }

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

    let notes_out = match (notes_db, repo) {
        (Some(db), Some(r)) => notes_for_file(db, r, &f.path)
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
            })
            .collect(),
        _ => Vec::new(),
    };

    let content_to_classify = if !f.new_content.is_empty() {
        &f.new_content
    } else {
        &f.old_content
    };
    let content_runs: Vec<Vec<OutputRun>> = content_to_classify
        .iter()
        .enumerate()
        .map(|(idx, line_text)| {
            let line_num = (idx + 1) as u32;
            classify_ctx_line(line_text, Some(line_num), profile, &regions)
                .into_iter()
                .map(|tk| OutputRun {
                    kind: RunKind::Ctx,
                    color: Some(tk.color),
                    sgr: tk.sgr,
                    text: tk.text,
                })
                .collect()
        })
        .collect();

    OutputFile {
        path: f.path.clone(),
        status: format!("{:?}", f.status).to_lowercase(),
        new_line_count: f.new_line_count,
        old_line_count: f.old_line_count,
        new_content: f.new_content.clone(),
        old_content: f.old_content.clone(),
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
        rows: {
            let mut hunks_with_ctx = f.hunks.clone();
            for h in &mut hunks_with_ctx {
                if h.above_ctx == 0 {
                    h.above_ctx = 6;
                }
                if h.below_ctx == 0 {
                    h.below_ctx = 6;
                }
            }
            build_visible(
                &hunks_with_ctx,
                f.new_line_count,
                f.old_line_count,
                &f.new_content,
                &f.old_content,
                f.new_content.is_empty() && f.old_content.is_empty(),
            )
        }
        .into_iter()
        .map(|vis| {
            let r = vis.row;
            let runs = if matches!(
                r,
                Row::Mod { .. } | Row::Add(_) | Row::Mov { .. } | Row::Del(_)
            ) {
                colored_runs(&r, profile, &regions)
            } else {
                classify_ctx_line(
                    r.text(),
                    match vis.side {
                        Some(VisibleSide::New) => r.new_line(),
                        Some(VisibleSide::Old) => None,
                        None => r.new_line().or_else(|| r.old_line()),
                    },
                    profile,
                    &regions,
                )
                .into_iter()
                .map(|tk| OutputRun {
                    kind: RunKind::Ctx,
                    color: Some(tk.color),
                    sgr: tk.sgr,
                    text: tk.text,
                })
                .collect()
            };
            let (new_line, old_line) = match (&r, vis.side) {
                (Row::Ctx(_), Some(VisibleSide::New)) | (Row::Add(_) | Row::Mov { .. }, _) => {
                    (r.new_line(), None)
                }
                (Row::Ctx(_), Some(VisibleSide::Old)) | (Row::Del(_), _) => (None, r.old_line()),
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
    let classify_full = |text: &str| -> Vec<ClassifiedToken> {
        classify_ctx_line(text, new_line, profile, regions)
    };
    let region = new_line.and_then(|n| region_at(regions, n));
    let classify_frag = |text: &str| -> Vec<ClassifiedToken> {
        match region {
            Some(rg) => classify_tokens(text, rg.lang.as_deref().and_then(lang_profile)),
            None => {
                if profile.is_some_and(|p| p.markdown) {
                    markdown_tokens(text)
                } else {
                    classify_tokens(text, profile)
                }
            }
        }
    };

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
            for t in classify_full(r.text()) {
                out.push(OutputRun {
                    kind,
                    color: Some(t.color),
                    sgr: t.sgr,
                    text: t.text,
                });
            }
        } else {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: None,
                sgr: None,
                text: r.text().to_string(),
            });
        }
        return out;
    }

    let full_tokens = classify_full(r.text());
    let mut add_pos = 0usize;
    for run in merged_runs(r) {
        if run.kind == RunKind::Del {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: None,
                sgr: None,
                text: run.text,
            });
        } else {
            let run_len = run.text.chars().count();
            let end_pos = add_pos + run_len;
            let frags = slice_classified_tokens(&full_tokens, add_pos, end_pos);
            add_pos = end_pos;
            if frags.is_empty() {
                for t in classify_frag(&run.text) {
                    out.push(OutputRun {
                        kind: run.kind,
                        color: Some(t.color),
                        sgr: t.sgr,
                        text: t.text,
                    });
                }
            } else {
                for t in frags {
                    out.push(OutputRun {
                        kind: run.kind,
                        color: Some(t.color),
                        sgr: t.sgr,
                        text: t.text,
                    });
                }
            }
        }
    }
    coalesce_output_runs(out)
}

fn classify_ctx_line(
    text: &str,
    new_line: Option<u32>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<ClassifiedToken> {
    let is_delimiter = new_line.is_some_and(|n| {
        regions
            .iter()
            .any(|rg| n + 1 == rg.start || (n > 0 && n - 1 == rg.end))
    });
    if is_delimiter {
        if let Some(strip) = profile.and_then(|p| p.fence_strip.as_ref())
            && let Some(m) = strip.find(text)
            && m.start() == 0
            && m.end() > 0
        {
            return vec![
                ClassifiedToken {
                    color: "syntaxComment".into(),
                    sgr: None,
                    text: text[..m.end()].into(),
                },
                ClassifiedToken {
                    color: "muted".into(),
                    sgr: None,
                    text: text[m.end()..].into(),
                },
            ];
        }
        return vec![ClassifiedToken {
            color: "muted".into(),
            sgr: None,
            text: text.into(),
        }];
    }
    let region = new_line.and_then(|n| region_at(regions, n));
    classify_line(text, profile, region)
}

fn coalesce_output_runs(runs: Vec<OutputRun>) -> Vec<OutputRun> {
    let mut out: Vec<OutputRun> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.kind == run.kind
            && last.color == run.color
            && last.sgr == run.sgr
        {
            last.text.push_str(&run.text);
            continue;
        }
        out.push(run);
    }
    out
}

fn slice_classified_tokens(
    tokens: &[ClassifiedToken],
    start_char: usize,
    end_char: usize,
) -> Vec<ClassifiedToken> {
    let mut out = Vec::new();
    let mut pos = 0;
    for tok in tokens {
        let tok_len = tok.text.chars().count();
        let tok_start = pos;
        let tok_end = pos + tok_len;
        pos = tok_end;
        if tok_end <= start_char || tok_start >= end_char {
            continue;
        }
        let s = start_char.saturating_sub(tok_start);
        let e = end_char.min(tok_end) - tok_start;
        let piece: String = tok.text.chars().skip(s).take(e - s).collect();
        if !piece.is_empty() {
            out.push(ClassifiedToken {
                color: tok.color.clone(),
                sgr: tok.sgr,
                text: piece,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
