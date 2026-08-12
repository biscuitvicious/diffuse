//! The engine pipeline for the bin: parse → reconstruct the old side →
//! mark moves → collapse → merged runs → counts. The output shape is the
//! wire contract (PLAN.md §2).

use crate::collapse::collapse_hunk_rows;
use crate::counts::file_counts;
use crate::merge::{RunKind, merged_runs};
use crate::moves::mark_moves;
use crate::old_content::old_content_from_diff;
use crate::parse::{RowKind, parse_diff};
use crate::syntax::{classify_tokens, lang_profile};
use serde::{Deserialize, Serialize};

/// A row in the wire-contract output: the collapse row + its merged runs.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRow {
    pub kind: String,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    pub spans: crate::parse::Spans,
    pub runs: Vec<OutputRun>,
    pub moved: bool,
    /// The mod's absorbed section lines — the renderer draws them as pure
    /// context (newLine + text, no tint). Added when the renderer began
    /// consuming the contract (PLAN.md §4); also lets the acceptance walk
    /// verify the header counts (tail rows count as adds/moves).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<OutputTailRow>,
}

/// One absorbed line of a mod's tail.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTailRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub moved: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRun {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub text: String,
}

fn row_kind_name(k: RowKind) -> &'static str {
    match k {
        RowKind::Add => "add",
        RowKind::Del => "del",
        RowKind::Ctx => "ctx",
        RowKind::Sep => "sep",
        RowKind::File => "file",
        RowKind::Mod => "mod",
    }
}

/// The merged runs with the semantic syntax colors on the common/add runs
/// (the del runs stay plain — the strike is the renderer's business). A
/// mod row — or a moved row whose delText differs (the movedDiff inline
/// subword — an indentation reflow, say) — gets the merged runs; anything
/// else is a single whole-line run of its kind (an add classifies its
/// syntax; a del stays plain).
pub fn colored_runs(
    r: &crate::parse::Row,
    profile: Option<&crate::syntax::LangProfile>,
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    // The renderer's token source (mergedTokens in review.ts): a mod row,
    // or a moved row whose delText differs from its text, merges; a
    // wholesale mod's del block is styled directly by the renderer.
    let use_merged = (r.kind == RowKind::Mod
        || (r.moved && r.del_text.as_deref() != Some(r.text.as_str())))
        && r.del_text.is_some();
    if !use_merged {
        if r.kind == RowKind::Add {
            for t in classify_tokens(&r.text, profile) {
                out.push(OutputRun {
                    kind: "add".to_string(),
                    color: Some(t.color),
                    text: t.text,
                });
            }
        } else {
            out.push(OutputRun {
                kind: row_kind_name(r.kind).to_string(),
                color: None,
                text: r.text.clone(),
            });
        }
        return out;
    }
    for run in merged_runs(r) {
        if run.kind == RunKind::Del {
            out.push(OutputRun {
                kind: "del".to_string(),
                color: None,
                text: run.text,
            });
        } else {
            for t in classify_tokens(&run.text, profile) {
                out.push(OutputRun {
                    kind: if run.kind == RunKind::Common {
                        "common".to_string()
                    } else {
                        "add".to_string()
                    },
                    color: Some(t.color),
                    text: t.text,
                });
            }
        }
    }
    out
}

/// Run the pipeline over a diff + the new-file contents; produce the
/// wire-contract output.
pub fn run_pipeline(
    diff: &str,
    contents: &std::collections::HashMap<String, String>,
) -> PipelineOutput {
    let mut files = parse_diff(diff);
    let mut out_files = Vec::new();
    for f in &mut files {
        let ext = f.path.rsplit('.').next().unwrap_or("");
        let profile = lang_profile(ext);
        let hash_comment = profile.as_ref().is_some_and(|p| p.hash_comment);
        f.new_content = contents
            .get(&f.path)
            .map(|c| c.split('\n').map(String::from).collect())
            .unwrap_or_default();
        f.new_line_count = f.new_content.len();
        if f.old_content.is_empty() {
            f.old_content = old_content_from_diff(diff, &f.path);
            f.old_line_count = f.old_content.len();
        }
        mark_moves(f);
        let old_content = f.old_content.clone();
        let new_content = f.new_content.clone();
        for h in &mut f.hunks {
            h.rows = collapse_hunk_rows(h, hash_comment, Some(&old_content), Some(&new_content));
        }
        // A moved row's own spans: the collapse marks the move (delText =
        // the paired del) but bakes no spans — the renderer needs the
        // indentation subword (8 → 12 spaces is a semantic move, not
        // invisible). Bake word_spans(delText, text) so the wire carries
        // them (the TS engine computed them on the fly).
        for h in &mut f.hunks {
            for r in &mut h.rows {
                if r.moved
                    && r.del_text.is_some()
                    && r.del_text.as_deref() != Some(r.text.as_str())
                    && r.spans.is_none()
                {
                    let del = r.del_text.clone().unwrap_or_default();
                    r.spans = Some(crate::lcs::word_spans(&del, &r.text));
                }
            }
        }
        let counts = file_counts(f);
        let mut hunks_out = Vec::new();
        for h in &f.hunks {
            let mut rows_out = Vec::new();
            for r in &h.rows {
                rows_out.push(OutputRow {
                    kind: row_kind_name(r.kind).to_string(),
                    new_line: r.new_line,
                    old_line: r.old_line,
                    text: r.text.clone(),
                    del_text: r.del_text.clone(),
                    spans: r.spans.clone().unwrap_or_default(),
                    runs: colored_runs(r, profile.as_ref()),
                    moved: r.moved,
                    tail: r
                        .tail
                        .iter()
                        .map(|t| OutputTailRow {
                            new_line: t.new_line,
                            old_line: t.old_line,
                            text: t.text.clone(),
                            moved: t.moved,
                        })
                        .collect(),
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
        out_files.push(OutputFile {
            path: f.path.clone(),
            status: format!("{:?}", f.status).to_lowercase(),
            new_line_count: f.new_line_count,
            old_line_count: f.old_line_count,
            hunks: hunks_out,
            counts,
        });
    }
    PipelineOutput { files: out_files }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub rows: Vec<OutputRow>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub path: String,
    pub status: String,
    pub new_line_count: usize,
    pub old_line_count: usize,
    pub hunks: Vec<OutputHunk>,
    pub counts: crate::counts::FileCounts,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
}
