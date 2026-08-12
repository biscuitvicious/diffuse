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
use serde::Serialize;

/// A row in the wire-contract output: the collapse row + its merged runs.
#[derive(Serialize)]
pub struct OutputRow {
    pub kind: &'static str,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    pub spans: crate::parse::Spans,
    pub runs: Vec<OutputRun>,
    pub moved: bool,
}

#[derive(Serialize)]
pub struct OutputRun {
    pub kind: &'static str,
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
/// non-mod row is a single whole-line run of its kind (an add classifies
/// its syntax; a del stays plain).
pub fn colored_runs(
    r: &crate::parse::Row,
    profile: Option<&crate::syntax::LangProfile>,
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    if r.kind != RowKind::Mod {
        if r.kind == RowKind::Add {
            for t in classify_tokens(&r.text, profile) {
                out.push(OutputRun {
                    kind: "add",
                    color: Some(t.color),
                    text: t.text,
                });
            }
        } else {
            out.push(OutputRun {
                kind: row_kind_name(r.kind),
                color: None,
                text: r.text.clone(),
            });
        }
        return out;
    }
    for run in merged_runs(r) {
        if run.kind == RunKind::Del {
            out.push(OutputRun {
                kind: "del",
                color: None,
                text: run.text,
            });
        } else {
            for t in classify_tokens(&run.text, profile) {
                out.push(OutputRun {
                    kind: if run.kind == RunKind::Common {
                        "common"
                    } else {
                        "add"
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
        let counts = file_counts(f);
        let mut hunks_out = Vec::new();
        for h in &f.hunks {
            let mut rows_out = Vec::new();
            for r in &h.rows {
                rows_out.push(OutputRow {
                    kind: row_kind_name(r.kind),
                    new_line: r.new_line,
                    old_line: r.old_line,
                    text: r.text.clone(),
                    del_text: r.del_text.clone(),
                    spans: r.spans.clone().unwrap_or_default(),
                    runs: colored_runs(r, profile.as_ref()),
                    moved: r.moved,
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

#[derive(Serialize)]
pub struct OutputHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub rows: Vec<OutputRow>,
}

#[derive(Serialize)]
pub struct OutputFile {
    pub path: String,
    pub status: String,
    pub new_line_count: usize,
    pub old_line_count: usize,
    pub hunks: Vec<OutputHunk>,
    pub counts: crate::counts::FileCounts,
}

#[derive(Serialize)]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
}
