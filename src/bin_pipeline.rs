//! The engine pipeline for the bin: parse → reconstruct the old side →
//! mark moves → collapse → merged runs → counts. The output shape is the
//! wire contract (PLAN.md §2).

use crate::collapse::collapse_hunk_rows;
use crate::counts::file_counts;
use crate::merge::{RunKind, merged_runs};
use crate::moves::mark_moves;
use crate::old_content::old_content_from_diff;
use crate::parse::{RowKind, parse_diff};
use crate::syntax::{
    ClassifiedToken, classify_tokens, fence_regions, lang_profile, markdown_tokens,
};
use serde::{Deserialize, Serialize};

/// A row in the wire-contract output: the collapse row + its merged runs.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRow {
    pub kind: RowKind,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    pub spans: crate::parse::Spans,
    pub runs: Vec<OutputRun>,
    /// The mod is wholesale (the del block + the add block render as two
    /// separate blocks, never merged) — the renderer's old isWholesaleMod
    /// decision, baked by the engine so the TS merge machinery can go.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wholesale: bool,
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
    /// The engine's classified tokens for this absorbed line (it renders as
    /// ctx — the engine classifies it with the file's fence regions, same
    /// as a ctx row). The renderer reads these straight off the wire — no
    /// TS-side re-tokenization. Empty when there's nothing to classify.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<OutputRun>,
}

#[derive(Serialize, Deserialize)]
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

/// A fenced syntax region in a file: the span of lines whose content
/// classifies with a different language than the file's. `lang` empty =
/// the plain "code" style. The delimiter lines (start-1 / end+1) render
/// in the file's default, muted.
#[derive(Serialize, Deserialize)]
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
    profile: Option<&crate::syntax::LangProfile>,
    regions: &[crate::syntax::FenceRegion],
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
    let region = new_line.and_then(|n| crate::syntax::region_at(regions, n));
    crate::syntax::classify_line(text, profile, region)
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
    r: &crate::parse::Row,
    profile: Option<&crate::syntax::LangProfile>,
    regions: &[crate::syntax::FenceRegion],
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    let new_line = r.new_line;
    // The FULL-line classifier (an add row — the strip + the delimiter
    // + the region/md default all apply), and the FRAGMENT classifier (a
    // merged common/add run — the strip is a full-line concern, the
    // fragments classify as-is).
    let classify_full = |text: &str| -> Vec<ClassifiedToken> {
        classify_ctx_line(text, new_line, profile, regions)
    };
    // A merged common/add fragment classifies in the row's region (no
    // delimiter-muted path — a fragment is never a whole delimiter line).
    let region = new_line.and_then(|n| crate::syntax::region_at(regions, n));
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
    // The renderer's token source (the wire runs): a mod row, or a moved
    // row whose delText differs from its text, merges.
    let use_merged = (r.kind == RowKind::Mod
        || (r.moved && r.del_text.as_deref() != Some(r.text.as_str())))
        && r.del_text.is_some();
    if !use_merged {
        if r.kind == RowKind::Add || r.kind == RowKind::Ctx {
            // Add AND ctx rows carry the engine's classified runs — the
            // renderer reads them straight off the wire for both row
            // kinds (no TS-side re-tokenization). A del stays plain (the
            // strike is the renderer's business). The RUN kind is the
            // renderer's strike/fill hint: add → Add, ctx → Ctx (a ctx
            // row is neither struck nor filled — Ctx falls through to
            // the line tint in styleTokens).
            let kind = if r.kind == RowKind::Add {
                RunKind::Add
            } else {
                RunKind::Ctx
            };
            for t in classify_full(&r.text) {
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
                text: r.text.clone(),
            });
        }
        return out;
    }
    let full_tokens = classify_full(&r.text);
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
    contents: &std::collections::HashMap<String, String>,
    notes_db: Option<&crate::notes::NotesDB>,
    repo: Option<&str>,
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
        // The fence regions: the file's syntax-region spans (generic — md's
        // ``` blocks, rust's doc-comment examples, ...).
        let regions = fence_regions(&f.new_content, profile);
        let mut hunks_out = Vec::new();
        for h in &f.hunks {
            let mut rows_out = Vec::new();
            for r in &h.rows {
                rows_out.push(OutputRow {
                    kind: r.kind,
                    new_line: r.new_line,
                    old_line: r.old_line,
                    text: r.text.clone(),
                    del_text: r.del_text.clone(),
                    spans: r.spans.clone().unwrap_or_default(),
                    runs: colored_runs(r, profile, &regions),
                    wholesale: r.kind == RowKind::Mod && crate::merge::is_wholesale_mod(r),
                    moved: r.moved,
                    tail: r
                        .tail
                        .iter()
                        .map(|t| {
                            let runs = classify_ctx_line(&t.text, t.new_line, profile, &regions);
                            OutputTailRow {
                                new_line: t.new_line,
                                old_line: t.old_line,
                                text: t.text.clone(),
                                moved: t.moved,
                                runs: runs
                                    .into_iter()
                                    .map(|tk| OutputRun {
                                        kind: RunKind::Ctx,
                                        color: Some(tk.color),
                                        sgr: tk.sgr,
                                        text: tk.text,
                                    })
                                    .collect(),
                            }
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
        let notes_out = match (notes_db, repo) {
            (Some(db), Some(r)) => crate::notes::notes_for_file(db, r, &f.path)
                .into_iter()
                .map(|n| OutputNote {
                    id: n.id.clone(),
                    file: n.file.clone(),
                    side: n.side,
                    start: n.start,
                    end: n.end,
                    text: n.text.clone(),
                    created_at: n.created_at,
                    runs: crate::notes::note_runs(&n.text),
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
                    lang: crate::syntax::fence_lang_ext(&r.lang.clone().unwrap_or_default())
                        .to_string(),
                })
                .collect(),
            notes: notes_out,
            content_runs,
            rows: crate::visible::build_visible(
                &f.hunks,
                f.new_line_count,
                f.old_line_count,
                &f.new_content,
                &f.old_content,
                f.new_content.is_empty() && f.old_content.is_empty(),
            )
            .into_iter()
            .map(|r| {
                let runs =
                    if r.kind == RowKind::Mod || r.kind == RowKind::Add || r.kind == RowKind::Del {
                        colored_runs(&r, profile, &regions)
                    } else {
                        classify_ctx_line(&r.text, r.new_line.or(r.old_line), profile, &regions)
                            .into_iter()
                            .map(|tk| OutputRun {
                                kind: RunKind::Ctx,
                                color: Some(tk.color),
                                sgr: tk.sgr,
                                text: tk.text,
                            })
                            .collect()
                    };
                let wholesale = r.kind == RowKind::Mod && crate::merge::is_wholesale_mod(&r);
                OutputRow {
                    kind: r.kind,
                    new_line: r.new_line,
                    old_line: r.old_line,
                    text: r.text,
                    del_text: r.del_text,
                    spans: r.spans.unwrap_or_default(),
                    runs,
                    wholesale,
                    moved: r.moved,
                    tail: Vec::new(),
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
pub fn run_pipeline(
    diff: &str,
    contents: &std::collections::HashMap<String, String>,
) -> PipelineOutput {
    run_pipeline_with_notes(diff, contents, None, None)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputNote {
    pub id: String,
    pub file: String,
    pub side: crate::notes::Side,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub created_at: u64,
    pub runs: Vec<OutputRun>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_content: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub old_content: Vec<String>,
    pub hunks: Vec<OutputHunk>,
    pub counts: crate::counts::FileCounts,
    /// The fence regions (the wire's syntax-region spans — line 1-indexed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fences: Vec<OutputFence>,
    /// The review notes attached to this file (with engine-classified markdown runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<OutputNote>,
    /// Pre-classified syntax token runs for all context/content lines of the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_runs: Vec<Vec<OutputRun>>,
    /// Pre-assembled row sequence for default context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<OutputRow>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untracked_skipped: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Row, RowKind};

    #[test]
    fn test_fence_delimiter_line_preserves_comment_prefix_color() {
        let profile = crate::syntax::lang_profile("rs");
        let regions = vec![crate::syntax::FenceRegion {
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
        let mut contents = std::collections::HashMap::new();
        contents.insert(
            "test.rs".to_string(),
            "// The deepest layer the below-walk may descend to: a directory found with\n"
                .to_string(),
        );
        let output = run_pipeline(diff, &contents);
        let row = &output.files[0].hunks[0].rows[0];
        let common_after = row.runs.iter().find(|r| r.text.contains(" layer the "));
        assert!(common_after.is_some(), "common run ' layer the ' exists");
        assert_eq!(
            common_after.unwrap().color.as_deref(),
            Some("syntaxComment"),
            "text after fill preserves syntaxComment color"
        );
    }

    #[test]
    fn test_comment_reflow_del_span_positioning() {
        // Row 1078: `lowers` is deleted before `non-top-upper layers` is added, and trailing `layer's markers —` lands at the end
        let old_1078 = "// Walk the lowers top → bottom, honoring each layer's markers —";
        let new_1078 = "// Walk the non-top-upper layers top → bottom, honoring each";
        let r_1078 = Row {
            kind: RowKind::Mod,
            text: new_1078.to_string(),
            new_line: Some(1078),
            old_line: Some(1078),
            spans: Some(crate::lcs::word_spans(old_1078, new_1078)),
            moved: false,
            del_text: Some(old_1078.to_string()),
            tail: Vec::new(),
        };
        let runs_1078 = crate::merge::merged_runs(&r_1078);
        let non_common_1078: Vec<(crate::merge::RunKind, &str)> = runs_1078
            .iter()
            .filter(|r| r.kind != crate::merge::RunKind::Common)
            .map(|r| (r.kind, r.text.as_str()))
            .collect();
        assert_eq!(
            non_common_1078,
            vec![
                (crate::merge::RunKind::Del, "lowers"),
                (crate::merge::RunKind::Add, "non-top-upper layers"),
                (crate::merge::RunKind::Del, " layer's markers —"),
            ],
            "1078: lowers deletes before non-top-upper layers adds, layer's markers strikes at end"
        );

        // Row 1079: `layer's markers — ` adds at start, `upper`->`layer` pairs, `whiteouts...` strikes at end of `carries` (not inside `ca...rries`)
        let old_1079 = "// a frozen (snapshotted) upper carries whiteouts and opaque dirs";
        let new_1079 = "// layer's markers — a frozen (snapshotted) layer carries";
        let r_1079 = Row {
            kind: RowKind::Mod,
            text: new_1079.to_string(),
            new_line: Some(1079),
            old_line: Some(1079),
            spans: Some(crate::lcs::word_spans(old_1079, new_1079)),
            moved: false,
            del_text: Some(old_1079.to_string()),
            tail: Vec::new(),
        };
        let runs_1079 = crate::merge::merged_runs(&r_1079);
        let non_common_1079: Vec<(crate::merge::RunKind, &str)> = runs_1079
            .iter()
            .filter(|r| r.kind != crate::merge::RunKind::Common)
            .map(|r| (r.kind, r.text.as_str()))
            .collect();
        assert_eq!(
            non_common_1079,
            vec![
                (crate::merge::RunKind::Add, "layer's markers — "),
                (crate::merge::RunKind::Del, "upper"),
                (crate::merge::RunKind::Add, "layer"),
                (crate::merge::RunKind::Del, " whiteouts and opaque dirs"),
            ],
            "1079: layer's markers adds at start, upper->layer pairs, whiteouts strikes at end of carries"
        );
    }
}
