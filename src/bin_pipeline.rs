//! The engine pipeline for the bin: parse → reconstruct the old side →
//! mark moves → collapse → merged runs → counts. The output shape is the
//! wire contract (PLAN.md §2).

use crate::{
    collapse::collapse_hunk_rows,
    counts::{FileCounts, file_counts},
    lcs::word_spans,
    merge::{RunKind, is_wholesale_mod, merged_runs},
    moves::mark_moves,
    notes::{NotesDB, Side as NoteSide, note_runs, notes_for_file},
    old_content::old_content_from_diff,
    parse::{
        AddLine, CtxLine, DelLine, DiffFile, FileStatus, Hunk, LineNum, Row, RowKind, Spans,
        parse_diff,
    },
    syntax::{
        ClassifiedToken, FenceRegion, LangProfile, classify_line, classify_tokens, fence_lang_ext,
        fence_regions, lang_profile, markdown_tokens, region_at,
    },
    visible::{Side as VisibleSide, build_visible},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A row in the wire-contract output: the collapse row + its merged runs.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRow {
    pub kind: RowKind,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    pub spans: Spans,
    pub runs: Vec<OutputRun>,
    /// The mod is wholesale (the del block + the add block render as two
    /// separate blocks, never merged) — the renderer's old isWholesaleMod
    /// decision, baked by the engine so the TS merge machinery can go.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wholesale: bool,
    pub moved: bool,
}

impl From<&OutputRow> for Row {
    fn from(r: &OutputRow) -> Self {
        match r.kind {
            RowKind::Ctx => {
                let old_pos = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_pos = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                Row::Ctx(CtxLine::new(old_pos, new_pos, &r.text))
            }
            RowKind::Del => {
                let num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                Row::Del(DelLine::new(num, &r.text))
            }
            RowKind::Add if r.moved => {
                let old_num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let del_text = r.del_text.as_deref().unwrap_or(&r.text);
                Row::Mov {
                    old_line: DelLine::new(old_num, del_text),
                    new_line: AddLine::with_spans(new_num, &r.text, r.spans.clone()),
                }
            }
            RowKind::Add => {
                let num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                if let Some(ref dt) = r.del_text {
                    Row::Add(AddLine::with_donor(num, &r.text, dt, r.spans.clone()))
                } else {
                    Row::Add(AddLine::with_spans(num, &r.text, r.spans.clone()))
                }
            }
            RowKind::Mod => {
                let old_num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let del_text = r.del_text.as_deref().unwrap_or(&r.text);
                Row::Mod {
                    old_line: DelLine::new(old_num, del_text),
                    new_line: AddLine::with_spans(new_num, &r.text, r.spans.clone()),
                }
            }
            RowKind::Sep | RowKind::File => Row::Sep(r.text.clone()),
        }
    }
}

impl OutputRow {
    pub fn reverse(&self) -> Self {
        OutputRow {
            kind: self.kind.reverse(),
            text: self.del_text.clone().unwrap_or_else(|| self.text.clone()),
            new_line: self.old_line,
            old_line: self.new_line,
            del_text: if self.kind == RowKind::Mod || self.del_text.is_some() {
                Some(self.text.clone())
            } else {
                None
            },
            spans: self.spans.reverse(),
            runs: self.runs.iter().map(OutputRun::reverse).collect(),
            wholesale: self.wholesale,
            moved: self.moved,
        }
    }

    #[inline]
    pub fn reconstruct(&self) -> Row {
        self.into()
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRun {
    pub kind: RunKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The SGR attribute (the markdown emphasis — bold/italic/strike/
    /// underline) the renderer applies with the color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgr: Option<u32>,
    pub text: String,
}

impl OutputRun {
    pub fn reverse(&self) -> Self {
        OutputRun {
            kind: self.kind.reverse(),
            color: self.color.clone(),
            sgr: self.sgr,
            text: self.text.clone(),
        }
    }
}

/// A fenced syntax region in a file: the span of lines whose content
/// classifies with a different language than the file's. `lang` empty =
/// the plain "code" style. The delimiter lines (start-1 / end+1) render
/// in the file's default, muted.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFence {
    pub start: u32,
    pub end: u32,
    pub lang: String,
}

/// Classify a full line as the renderer's ctx/token source: a fence
/// delimiter line renders muted; otherwise the line classifies in its
/// region's language (or the file's default — md prose, the rust doc
/// fence's strip-then-classify, the plain comment-aware tokenizer). This
/// is the SINGLE engine-side ctx classifier — the renderer no longer
/// re-tokenizes ctx rows (they ride the wire's `runs`).
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

/// The merged runs with the semantic syntax colors on the common/add runs
/// (the del runs stay plain — the strike is the renderer's business). A
/// mod row — or a moved row whose delText differs (the movedDiff inline
/// subword — an indentation reflow, say) — gets the merged runs; anything
/// else is a single whole-line run of its kind (an add classifies its
/// syntax; a del stays plain). The classification is REGION-AWARE: the
/// row's new-side line's language — a fence region's language, or the
/// file's default (md → the markdown tokens) — and a fence DELIMITER line
/// renders muted.
pub fn colored_runs(
    r: &Row,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    let new_line = r.new_line();
    // The FULL-line classifier (an add row — the strip + the delimiter
    // + the region/md default all apply), and the FRAGMENT classifier (a
    // merged common/add run — the strip is a full-line concern, the
    // fragments classify as-is).
    let classify_full = |text: &str| -> Vec<ClassifiedToken> {
        classify_ctx_line(text, new_line, profile, regions)
    };
    // A merged common/add fragment classifies in the row's region (no
    // delimiter-muted path — a fragment is never a whole delimiter line).
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
            // Add AND ctx rows carry the engine's classified runs — the
            // renderer reads them straight off the wire for both row
            // kinds (no TS-side re-tokenization). A del stays plain (the
            // strike is the renderer's business). The RUN kind is the
            // renderer's strike/fill hint: add → Add, ctx → Ctx (a ctx
            // row is neither struck nor filled — Ctx falls through to
            // the line tint in styleTokens).
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

/// Run the pipeline over a diff + the new-file contents + optional notes;
/// produce the wire-contract output.
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
    let mut files = parse_diff(diff);
    let mut out_files = Vec::new();
    for f in &mut files {
        let ext = f.path.rsplit('.').next().unwrap_or("");
        let profile = lang_profile(ext);
        f.new_content = contents
            .get(&f.path)
            .map(|c| c.split('\n').map(String::from).collect())
            .unwrap_or_default();
        f.new_line_count = f.new_content.len();
        if let Some(old_map) = old_contents
            && let Some(oc) = old_map.get(&f.path)
            && !oc.is_empty()
        {
            f.old_content = oc.split('\n').map(String::from).collect();
            f.old_line_count = f.old_content.len();
        } else if f.old_content.is_empty() {
            f.old_content = old_content_from_diff(diff, &f.path);
            f.old_line_count = f.old_content.len();
        }
        mark_moves(f);
        let old_content = f.old_content.clone();
        let new_content = f.new_content.clone();
        for h in &mut f.hunks {
            h.rows = collapse_hunk_rows(h, Some(&old_content), Some(&new_content), profile);
        }
        // A moved row's own spans: the collapse marks the move (delText =
        // the paired del) but bakes no spans — the renderer needs the
        // indentation subword (8 → 12 spaces is a semantic move, not
        // invisible). Bake word_spans(delText, text) so the wire carries
        // them (the TS engine computed them on the fly).
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
        let counts = file_counts(f);
        // The fence regions: the file's syntax-region spans (generic — md's
        // ``` blocks, rust's doc-comment examples, ...).
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

        out_files.push(OutputFile {
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
                    // Normalize the raw fence tag to its profile KEY (e.g.
                    // `rust,no_run`→`rs`, `edition2021`→`rs`, `python`→`py`)
                    // so the wire carries the canonical form — the
                    // renderer's `langProfile(region.lang)` resolves it via
                    // the simple alias map, never needing the doctest-
                    // attribute grammar (a single source of truth: the
                    // engine owns language resolution, the renderer just
                    // consumes the key).
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
        });
    }
    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

/// Run the pipeline over a diff + the new-file contents; produce the
/// wire-contract output.
pub fn run_pipeline(diff: &str, contents: &HashMap<String, String>) -> PipelineOutput {
    run_pipeline_with_notes(diff, contents, None, None)
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputNote {
    pub id: String,
    pub file: String,
    pub side: NoteSide,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub created_at: u64,
    pub runs: Vec<OutputRun>,
}

impl OutputNote {
    pub fn reverse(&self) -> Self {
        OutputNote {
            id: self.id.clone(),
            file: self.file.clone(),
            side: self.side.reverse(),
            start: self.start,
            end: self.end,
            text: self.text.clone(),
            created_at: self.created_at,
            runs: self.runs.iter().map(OutputRun::reverse).collect(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub rows: Vec<OutputRow>,
}

impl OutputHunk {
    pub fn reverse(&self) -> Self {
        OutputHunk {
            old_start: self.new_start,
            old_count: self.new_count,
            new_start: self.old_start,
            new_count: self.old_count,
            rows: self.rows.iter().map(OutputRow::reverse).collect(),
        }
    }

    pub fn reconstruct(&self) -> Hunk {
        Hunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            section: String::new(),
            rows: self.rows.iter().map(OutputRow::reconstruct).collect(),
            above_ctx: 0,
            below_ctx: 0,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub path: String,
    pub status: String,
    pub new_line_count: usize,
    pub old_line_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_content: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub old_content: Vec<String>,
    pub hunks: Vec<OutputHunk>,
    #[serde(default, skip_serializing)]
    pub raw_hunks: Vec<OutputHunk>,
    pub counts: FileCounts,
    /// The fence regions (the wire's syntax-region spans — line 1-indexed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fences: Vec<OutputFence>,
    /// The review notes attached to this file (with engine-classified markdown
    /// runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<OutputNote>,
    /// Pre-classified syntax token runs for all context/content lines of the
    /// file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_runs: Vec<Vec<OutputRun>>,
    /// Pre-assembled row sequence for default context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<OutputRow>,
}

impl OutputFile {
    pub fn reverse(&self) -> Self {
        let status = match self.status.as_str() {
            "added" => "deleted",
            "deleted" => "added",
            s => s,
        };
        OutputFile {
            path: self.path.clone(),
            status: status.to_string(),
            new_line_count: self.old_line_count,
            old_line_count: self.new_line_count,
            new_content: self.old_content.clone(),
            old_content: self.new_content.clone(),
            hunks: self.hunks.iter().map(OutputHunk::reverse).collect(),
            raw_hunks: self.raw_hunks.iter().map(OutputHunk::reverse).collect(),
            counts: self.counts.reverse(),
            fences: self.fences.clone(),
            notes: self.notes.iter().map(OutputNote::reverse).collect(),
            content_runs: self
                .content_runs
                .iter()
                .map(|runs| runs.iter().map(OutputRun::reverse).collect())
                .collect(),
            rows: self.rows.iter().map(OutputRow::reverse).collect(),
        }
    }

    pub fn reconstruct(&self) -> DiffFile {
        let status = match self.status.as_str() {
            "added" => FileStatus::Added,
            "deleted" => FileStatus::Deleted,
            "renamed" => FileStatus::Renamed,
            "untracked" => FileStatus::Untracked,
            _ => FileStatus::Modified,
        };
        DiffFile {
            path: self.path.clone(),
            old_path: None,
            status,
            new_line_count: self.new_line_count,
            old_line_count: self.old_line_count,
            new_content: self.new_content.clone(),
            old_content: self.old_content.clone(),
            hunks: self.hunks.iter().map(OutputHunk::reconstruct).collect(),
            raw_hunks: self.raw_hunks.iter().map(OutputHunk::reconstruct).collect(),
            no_content: self.new_content.is_empty() && self.old_content.is_empty(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untracked_skipped: Vec<String>,
}

impl PipelineOutput {
    pub fn reverse(&self) -> Self {
        PipelineOutput {
            files: self.files.iter().map(OutputFile::reverse).collect(),
            branch: self.branch.clone(),
            untracked_skipped: self.untracked_skipped.clone(),
        }
    }

    pub fn reconstruct(&self) -> Vec<DiffFile> {
        self.files.iter().map(OutputFile::reconstruct).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fence_delimiter_line_preserves_comment_prefix_color() {
        let profile = lang_profile("rs");
        let regions = vec![FenceRegion {
            start: 2,
            end: 3,
            lang: Some("rs".to_string()),
        }];
        let tokens = classify_ctx_line("/// ```ignore", Some(1), profile, &regions);
        assert_eq!(
            tokens.len(),
            2,
            "delimiter line produces 2 tokens (prefix + fence)"
        );
        assert_eq!(
            tokens[0].color, "syntaxComment",
            "comment prefix is syntaxComment"
        );
        assert_eq!(tokens[0].text, "/// ");
        assert_eq!(tokens[1].color, "muted", "fence is muted");
        assert_eq!(tokens[1].text, "```ignore");
    }

    #[test]
    fn test_mod_comment_row_preserves_comment_color_after_subword_fill() {
        let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,1 +1,1 @@\n-// The lowest layer the lower-walk may descend to: a directory found with\n+// The deepest layer the below-walk may descend to: a directory found with\n";
        let mut contents = HashMap::new();
        contents.insert(
            "test.rs".to_string(),
            "// The deepest layer the below-walk may descend to: a directory found with\n"
                .to_string(),
        );
        let output = run_pipeline(diff, &contents);
        let row = &output.files[0].hunks[0].rows[0];
        let common_after = row.runs.iter().find(|r| r.text.contains("layer the"));
        assert!(common_after.is_some(), "common run 'layer the' exists");
        assert_eq!(
            common_after.unwrap().color.as_deref(),
            Some("syntaxComment"),
            "text after fill preserves syntaxComment color"
        );
    }

    #[test]
    fn test_comment_reflow_del_span_positioning() {
        // Row 1078: `lowers` is deleted before `non-top-upper layers` is added, and
        // trailing `layer's markers —` lands at the end
        let old_1078 = "// Walk the lowers top → bottom, honoring each layer's markers —";
        let new_1078 = "// Walk the non-top-upper layers top → bottom, honoring each";
        let r_1078 = Row::Mod {
            old_line: DelLine::new(LineNum::new(1078).unwrap(), old_1078),
            new_line: AddLine::with_spans(
                LineNum::new(1078).unwrap(),
                new_1078,
                word_spans(old_1078, new_1078),
            ),
        };
        let runs_1078 = merged_runs(&r_1078);
        let non_common_1078: Vec<(RunKind, &str)> = runs_1078
            .iter()
            .filter(|r| r.kind != RunKind::Common)
            .map(|r| (r.kind, r.text.as_str()))
            .collect();
        assert_eq!(
            non_common_1078,
            vec![
                (RunKind::Del, "lowers"),
                (RunKind::Add, "non-top-upper layers"),
                (RunKind::Del, " layer's markers —"),
            ],
            "1078: lowers deletes before non-top-upper layers adds, layer's markers strikes at end"
        );

        // Row 1079: `layer's markers — ` adds at start, `upper`->`layer` pairs,
        // `whiteouts...` strikes at end of `carries` (not inside `ca...rries`)
        let old_1079 = "// a frozen (snapshotted) upper carries whiteouts and opaque dirs";
        let new_1079 = "// layer's markers — a frozen (snapshotted) layer carries";
        let r_1079 = Row::Mod {
            old_line: DelLine::new(LineNum::new(1079).unwrap(), old_1079),
            new_line: AddLine::with_spans(
                LineNum::new(1079).unwrap(),
                new_1079,
                word_spans(old_1079, new_1079),
            ),
        };
        let runs_1079 = merged_runs(&r_1079);
        let non_common_1079: Vec<(RunKind, &str)> = runs_1079
            .iter()
            .filter(|r| r.kind != RunKind::Common)
            .map(|r| (r.kind, r.text.as_str()))
            .collect();
        assert_eq!(
            non_common_1079,
            vec![
                (RunKind::Add, "layer's markers — "),
                (RunKind::Del, "upper"),
                (RunKind::Add, "layer"),
                (RunKind::Del, " whiteouts and opaque dirs"),
            ],
            "1079: layer's markers adds at start, upper->layer pairs, whiteouts strikes at end of carries"
        );
    }

    #[test]
    fn test_wire_reconstruct_and_reverse() {
        let diff = "diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,1 +10,2 @@ fn old() {
-old line one
+new line one
+new line two
";
        let mut contents = HashMap::new();
        contents.insert(
            "src/a.rs".to_string(),
            "new line one\nnew line two".to_string(),
        );

        let wire = run_pipeline(diff, &contents);
        let reconstructed_files = wire.reconstruct();
        assert_eq!(reconstructed_files.len(), 1);
        assert_eq!(reconstructed_files[0].path, "src/a.rs");

        // Involution on wire output: wire.reverse().reverse() == wire
        let wire_rev = wire.reverse();
        assert_eq!(wire_rev.reverse(), wire);

        // Reconstruct from reverse wire matches reversed AST files
        let rev_files_from_wire = wire_rev.reconstruct();
        let rev_files_direct: Vec<DiffFile> = reconstructed_files
            .iter()
            .map(super::super::parse::DiffFile::reverse)
            .collect();
        assert_eq!(rev_files_from_wire[0].path, rev_files_direct[0].path);
        assert_eq!(
            rev_files_from_wire[0].hunks.len(),
            rev_files_direct[0].hunks.len()
        );
        for (h1, h2) in rev_files_from_wire[0]
            .hunks
            .iter()
            .zip(rev_files_direct[0].hunks.iter())
        {
            assert_eq!(h1.old_start, h2.old_start);
            assert_eq!(h1.new_start, h2.new_start);
            assert_eq!(h1.rows.len(), h2.rows.len());
            for (r1, r2) in h1.rows.iter().zip(h2.rows.iter()) {
                assert_eq!(r1.kind(), r2.kind());
                assert_eq!(r1.text(), r2.text());
                assert_eq!(r1.new_line(), r2.new_line());
                assert_eq!(r1.old_line(), r2.old_line());
            }
        }
    }

    #[test]
    fn test_run_pipeline_full_with_old_contents() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -250,5 +250,5 @@\n                     origin: Mount::Overlay {\n-                    resolved_layers: unresolved,\n+                    layers: unresolved,\n";
        let mut contents = HashMap::new();
        contents.insert(
            "src/lib.rs".to_string(),
            "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        layers: unresolved,\n    })\n}".to_string(),
        );
        let mut old_contents = HashMap::new();
        old_contents.insert(
            "src/lib.rs".to_string(),
            "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        resolved_layers: unresolved,\n    })\n}".to_string(),
        );
        let out = run_pipeline_full(diff, &contents, Some(&old_contents), None, None);
        let mod_row = out.files[0].hunks[0]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Mod);
        assert!(
            mod_row.is_some(),
            "resolved_layers and layers pair into a Mod row"
        );
        let runs = &mod_row.unwrap().runs;
        let del_run = runs.iter().find(|r| r.kind == RunKind::Del);
        assert_eq!(del_run.map(|r| r.text.as_str()), Some("resolved_"));
    }
}
