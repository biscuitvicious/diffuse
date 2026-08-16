//! Unit tests for syntax section classification and cluster partitioning.

use super::*;
use crate::parse::{AddLine, CtxLine, DelLine, Hunk, LineNum, Row};

#[test]
fn test_classify_syntax_section() {
    assert_eq!(
        classify_syntax_section("/// Doc comment", false),
        SyntaxSectionKind::DocComment
    );
    assert_eq!(
        classify_syntax_section("// Helper line comment", false),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("# Python comment", true),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("#[derive(Debug, Clone)]", false),
        SyntaxSectionKind::Attribute
    );
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", false),
        SyntaxSectionKind::Declaration
    );
    assert_eq!(
        classify_syntax_section("let mut total = 0;", false),
        SyntaxSectionKind::Code
    );
    assert_eq!(
        classify_syntax_section("   ", false),
        SyntaxSectionKind::Blank
    );
}

#[test]
fn test_syntax_section_compatibility() {
    assert!(SyntaxSectionKind::DocComment.is_compatible(SyntaxSectionKind::DocComment));
    assert!(SyntaxSectionKind::Code.is_compatible(SyntaxSectionKind::Code));
    assert!(SyntaxSectionKind::Code.is_compatible(SyntaxSectionKind::Declaration));
    assert!(!SyntaxSectionKind::DocComment.is_compatible(SyntaxSectionKind::Code));
    assert!(!SyntaxSectionKind::Attribute.is_compatible(SyntaxSectionKind::DocComment));
}

#[test]
fn test_partition_hunk_segments_with_syntax_barriers() {
    let h = Hunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 5,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(LineNum::new(1).unwrap(), "/// Obsolete doc")),
            Row::Ctx(CtxLine::new(
                LineNum::new(2).unwrap(),
                LineNum::new(2).unwrap(),
                "pub fn existing() {",
            )),
            Row::Add(AddLine::new(LineNum::new(3).unwrap(), "    let x = 10;")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let segments = partition_hunk_segments(&h, false);
    assert_eq!(segments.len(), 3);

    // Segment 0: Doc comment deletion cluster
    match &segments[0] {
        HunkSegment::Cluster(c) => {
            assert_eq!(c.section, SyntaxSectionKind::DocComment);
            assert_eq!(c.old_rows.len(), 1);
            assert_eq!(c.new_rows.len(), 0);
        }
        _ => panic!("Expected cluster"),
    }

    // Segment 1: Context line
    match &segments[1] {
        HunkSegment::Context(r) => {
            assert_eq!(r.text(), "pub fn existing() {");
        }
        _ => panic!("Expected context"),
    }

    // Segment 2: Code addition cluster
    match &segments[2] {
        HunkSegment::Cluster(c) => {
            assert_eq!(c.section, SyntaxSectionKind::Code);
            assert_eq!(c.old_rows.len(), 0);
            assert_eq!(c.new_rows.len(), 1);
        }
        _ => panic!("Expected cluster"),
    }
}

// ---- Cluster DP transformation engine (Phase 3) ----
//
// The DP solves each (M dels, N adds) cluster into an optimal row stream
// (Mod / Mov / Del / Add / Expand / Collapse). Every test asserts BOTH the
// chosen decomposition AND the bidirectional projection invariant
// (Π_old ≡ old ∧ Π_new ≡ new) — the invariant is not optional.

use super::dp::{DpOp, DpSolution, solve_cluster, verify_projection};
// (Row, DelLine, AddLine, LineNum already imported at the top of this file)

fn del_row(n: u32, text: &str) -> Row {
    Row::Del(DelLine::new(LineNum::new(n).unwrap(), text))
}

fn add_row(n: u32, text: &str) -> Row {
    Row::Add(AddLine::new(LineNum::new(n).unwrap(), text))
}

fn code_cluster(old: Vec<Row>, new: Vec<Row>) -> ChangeCluster {
    ChangeCluster {
        old_rows: old,
        new_rows: new,
        section: SyntaxSectionKind::Code,
    }
}

/// The row-kind signature of a solution, in emission order.
fn signature(sol: &DpSolution) -> Vec<&'static str> {
    sol.rows
        .iter()
        .map(|r| match r {
            Row::Mod { .. } => "mod",
            Row::Mov { .. } => "mov",
            Row::Del(_) => "del",
            Row::Add(_) => "add",
            other => panic!("unexpected row in DP output: {other:?}"),
        })
        .collect()
}

#[test]
fn dp_verbatim_line_pairs_as_mov() {
    // An identical (trim-insensitive) line pair is a semantic move, not a mod.
    let c = code_cluster(
        vec![del_row(10, "    let value = 42;")],
        vec![add_row(21, "        let value = 42;")],
    );
    let sol = solve_cluster(&c, false);
    assert_eq!(signature(&sol), ["mov"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(matches!(sol.transitions[0], DpOp::Mov));
}

#[test]
fn dp_similar_lines_pair_as_mod() {
    let c = code_cluster(
        vec![del_row(10, "    let total = count + 1;")],
        vec![add_row(21, "    let total = count + 2;")],
    );
    let sol = solve_cluster(&c, false);
    assert_eq!(signature(&sol), ["mod"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(matches!(sol.transitions[0], DpOp::Mod { .. }));
}

#[test]
fn dp_unrelated_lines_stay_del_and_add() {
    // Zero shared tokens: pairing is worthless — a standalone Del + Add
    // decomposition must win.
    let c = code_cluster(
        vec![del_row(10, "fn generate_report(&self) -> Report {")],
        vec![add_row(21, "    drop(buffer);")],
    );
    let sol = solve_cluster(&c, false);
    assert_eq!(signature(&sol), ["del", "add"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
}

#[test]
fn dp_expansion_one_to_two() {
    // One deleted statement split across two added lines: the 1-to-K
    // expansion must beat pairing just the first line.
    let c = code_cluster(
        vec![del_row(
            10,
            "fn render(&self) -> String { return self.name.clone(); }",
        )],
        vec![
            add_row(21, "fn render(&self) -> String {"),
            add_row(22, "    return self.name.clone(); }"),
        ],
    );
    let sol = solve_cluster(&c, false);
    assert_eq!(signature(&sol), ["mod", "add"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(
        matches!(sol.transitions[0], DpOp::Expand { k: 2, .. }),
        "expected a 1-to-2 expansion, got {:?}",
        sol.transitions
    );
}

#[test]
fn dp_collapse_two_to_one() {
    // Two deleted lines folded into one added line: the K-to-1 collapse.
    let c = code_cluster(
        vec![
            del_row(10, "fn render(&self) -> String {"),
            del_row(11, "    return self.name.clone(); }"),
        ],
        vec![add_row(
            21,
            "fn render(&self) -> String { return self.name.clone(); }",
        )],
    );
    let sol = solve_cluster(&c, false);
    // Plan §4.2: a K-to-1 collapse renders as the anchor Mod followed by
    // the absorbed Del rows — the anchor owns the new line's head.
    assert_eq!(signature(&sol), ["mod", "del"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(
        matches!(sol.transitions[0], DpOp::Collapse { k: 2, .. }),
        "expected a 2-to-1 collapse, got {:?}",
        sol.transitions
    );
}

#[test]
fn dp_blank_lines_pair_as_single_row() {
    // Blank ↔ blank: one row (no fragmentation into del+add).
    let c = code_cluster(vec![del_row(10, "")], vec![add_row(21, "")]);
    let sol = solve_cluster(&c, false);
    assert_eq!(signature(&sol).len(), 1);
    verify_projection(&sol, &c).expect("projection invariant holds");
}

#[test]
fn dp_mixed_cluster_covers_all_lines_in_order() {
    // del, del, add, add with heterogeneous similarity: whatever the
    // optimal decomposition, every old and new line must appear exactly
    // once, in order, on its projection side.
    let c = code_cluster(
        vec![
            del_row(10, "// TODO: remove this scaffold"),
            del_row(11, "    let handle = open(path)?;"),
        ],
        vec![
            add_row(22, "    let handle = open(&path)?;"),
            add_row(23, "    debug!(\"opened {path}\");"),
        ],
    );
    let sol = solve_cluster(&c, false);
    verify_projection(&sol, &c).expect("projection invariant holds");
    // The comment must NOT pair with code (syntax-section compat is the
    // cluster partitioner's job; within a mixed cluster the DP's own
    // comment/code guard must keep them apart).
    for r in &sol.rows {
        if let Row::Mod { old_line, new_line } = r {
            let comment = |t: &str| t.trim_start().starts_with("//");
            assert!(
                !(comment(&old_line.text) ^ comment(&new_line.text)),
                "comment paired with code: {old_line:?} <-> {new_line:?}"
            );
        }
    }
}

#[test]
fn dp_projection_oracle_rejects_corrupted_solution() {
    // The verifier itself must catch a solution that drops or reorders a
    // line on either side.
    let c = code_cluster(
        vec![del_row(10, "alpha"), del_row(11, "beta")],
        vec![add_row(21, "alpha"), add_row(22, "gamma")],
    );
    let sol = solve_cluster(&c, false);
    verify_projection(&sol, &c).expect("clean solution verifies");

    // Corrupt: drop one row from the solution.
    let corrupted = DpSolution {
        rows: sol.rows[..sol.rows.len() - 1].to_vec(),
        transitions: sol.transitions.clone(),
        score: sol.score,
    };
    assert!(verify_projection(&corrupted, &c).is_err());

    // Corrupt: reorder two same-kind rows on the old side.
    let reordered = code_cluster(
        vec![del_row(10, "alpha"), del_row(11, "beta")],
        vec![add_row(21, "alpha"), add_row(22, "gamma")],
    );
    let mut sol2 = solve_cluster(&reordered, false);
    // swap the two first rows if both are dels or mods — generic: reverse all
    sol2.rows.reverse();
    assert!(verify_projection(&sol2, &reordered).is_err());
}

#[test]
fn dp_subline_moves_recorded_for_reflow_expansion() {
    // A comment reflow: the tail of the deleted line continues on the next
    // added line. The expansion transition must record subline move spans
    // (source token -> target line + token) for tokens that landed beyond
    // the anchor line.
    let c = code_cluster(
        vec![del_row(
            10,
            "// The renderer walks the token stream and renders each row.",
        )],
        vec![
            add_row(21, "// The renderer walks the token stream"),
            add_row(22, "// and renders each row."),
        ],
    );
    let sol = solve_cluster(&c, false);
    assert!(matches!(sol.transitions[0], DpOp::Expand { k: 2, .. }));
    verify_projection(&sol, &c).expect("projection invariant holds");
    if let DpOp::Expand { spans, .. } = &sol.transitions[0] {
        assert!(
            spans.iter().all(|s| s.new_line > 0),
            "subline moves must target lines beyond the anchor: {spans:?}"
        );
    }
}

#[test]
fn dp_corpus_projection_oracle() {
    // The mechanical oracle (plan Step 3.4): over EVERY cluster of every
    // frozen golden corpus, the DP's optimal decomposition must satisfy the
    // bidirectional string invariant — the rows project back to exactly the
    // cluster's old lines and new lines, in order, byte for byte.
    use crate::{parse::parse_diff, syntax::lang_profile};
    use serde::Deserialize;
    use std::fs;

    #[derive(Deserialize)]
    struct Fixture {
        diff: String,
    }

    const CORPORA: &[&str] = &[
        "mirage-2025-08",
        "diffuse-2025-08",
        "opt-1-cache-regex-window-slice",
        "opt-2-cache-markdown-regex",
        "opt-3-cache-lang-profile",
        "opt-4-manual-scanner-prefilter",
        "diffuse-history-mid",
    ];

    let mut clusters = 0usize;
    for name in CORPORA {
        let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let raw = fs::read_to_string(&path).unwrap();
        let fixture: Fixture = serde_json::from_str(&raw).unwrap();
        let files = parse_diff(&fixture.diff);
        for f in &files {
            let ext = f.path.rsplit('.').next().unwrap_or("");
            let hash_comment = lang_profile(ext).is_some_and(|p| p.hash_comment);
            for h in &f.hunks {
                for seg in partition_hunk_segments(h, hash_comment) {
                    if let HunkSegment::Cluster(c) = seg {
                        let sol = solve_cluster(&c, hash_comment);
                        verify_projection(&sol, &c).unwrap_or_else(|e| {
                            panic!("[{name}] {}: projection invariant failed: {e}", f.path)
                        });
                        clusters += 1;
                    }
                }
            }
        }
    }
    assert!(
        clusters > 100,
        "expected a real corpus sweep, got {clusters} clusters"
    );
}
