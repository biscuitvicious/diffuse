//! Unit tests for syntax section classification and cluster partitioning.

use std::{
    io::Read,
    path::PathBuf,
    thread::{available_parallelism, spawn},
};

use super::*;
use diffuse_syntax::registry::lang_profile;

use crate::parse::{AddLine, CtxLine, DelLine, Hunk, LineNum, Row};

#[test]
fn test_classify_syntax_section() {
    let rs = lang_profile("rs");
    let py = lang_profile("py");

    assert_eq!(
        classify_syntax_section("/// Doc comment", rs),
        SyntaxSectionKind::DocComment
    );
    assert_eq!(
        classify_syntax_section("// Helper line comment", rs),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("# Python comment", py),
        SyntaxSectionKind::LineComment
    );
    assert_eq!(
        classify_syntax_section("#[derive(Debug, Clone)]", rs),
        SyntaxSectionKind::Attribute
    );
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", rs),
        SyntaxSectionKind::Declaration
    );
    assert_eq!(
        classify_syntax_section("let mut total = 0;", rs),
        SyntaxSectionKind::Code
    );
    assert_eq!(classify_syntax_section("   ", rs), SyntaxSectionKind::Blank);
    // When profile is None, all non-blank lines classify as plain Code
    assert_eq!(
        classify_syntax_section("pub fn execute_job(&self) -> Result<()> {", None),
        SyntaxSectionKind::Code
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
    let rs = lang_profile("rs");
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

    let segments = partition_hunk_segments(&h, None, None, rs);
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

use super::dp::{
    DpOp, DpRowMeta, DpSolution, Weights, block_anchor, block_lcs, cell_touches_band_edge,
    dp_tokens, mod_pair_needs_scoring, score_lcs_candidates_par, score_lcs_candidates_seq,
    solve_cluster, verify_projection,
};
use diffuse_syntax::prelude::CloseDelimiter;
// (Row, DelLine, AddLine, LineNum already imported at the top of this file)

fn del_row(n: u32, text: &str) -> Row {
    Row::Del(DelLine::new(LineNum::new(n).unwrap(), text))
}

fn add_row(n: u32, text: &str) -> Row {
    Row::Add(AddLine::new(LineNum::new(n).unwrap(), text))
}

fn code_cluster(old: Vec<Row>, new: Vec<Row>) -> ChangeCluster<'static> {
    ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows: old,
        new_rows: new,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    }
}

/// The row-kind signature of a solution, in emission order.
fn signature(sol: &DpSolution) -> Vec<&'static str> {
    sol.rows
        .iter()
        .map(|r| match r {
            Row::Mod { .. } => "mod",
            Row::MovSrc(_) => "mov_src",
            Row::MovTgt(_) => "mov_tgt",
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
    assert_eq!(signature(&sol), ["mov_src", "mov_tgt"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(matches!(sol.transitions[0], DpOp::Mov));
}

#[test]
fn dp_similar_lines_pair_as_mod() {
    let c = code_cluster(
        vec![del_row(10, "    let total = count + 1;")],
        vec![add_row(21, "    let total = count + 2;")],
    );
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
    assert_eq!(signature(&sol), ["mod"]);
    verify_projection(&sol, &c).expect("projection invariant holds");
    assert!(matches!(sol.transitions[0], DpOp::Mod { .. }));
}

#[test]
fn test_match_arms_pair_one_to_one() {
    let old_lines = vec![
        del_row(13, "        CountKind::Dels => 203,"),
        del_row(14, "        CountKind::Adds => 120,"),
        del_row(15, "        CountKind::Mods => 214,"),
        del_row(16, "        CountKind::Movs => 147,"),
        del_row(17, "        CountKind::Notes => 220,"),
    ];
    let new_lines = vec![
        add_row(18, "        CountKind::Dels => palette.count_dels,"),
        add_row(19, "        CountKind::Adds => palette.count_adds,"),
        add_row(20, "        CountKind::Mods => palette.count_mods,"),
        add_row(21, "        CountKind::Movs => palette.count_movs,"),
        add_row(22, "        CountKind::Notes => palette.count_notes,"),
    ];
    let c = code_cluster(old_lines, new_lines);
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
    assert_eq!(
        signature(&sol),
        ["mod", "mod", "mod", "mod", "mod"],
        "Parallel match arms must pair 1-to-1 as Mods, got {:?}",
        signature(&sol)
    );
}

#[test]
fn dp_unrelated_lines_stay_del_and_add() {
    // Zero shared tokens: pairing is worthless — a standalone Del + Add
    // decomposition must win.
    let c = code_cluster(
        vec![del_row(10, "fn generate_report(&self) -> Report {")],
        vec![add_row(21, "    drop(buffer);")],
    );
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
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
    let mut sol2 = solve_cluster(&reordered, rs);
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
    let rs = lang_profile("rs");
    let sol = solve_cluster(&c, rs);
    assert!(matches!(sol.transitions[0], DpOp::Expand { k: 2, .. }));
    verify_projection(&sol, &c).expect("projection invariant holds");
    if let DpOp::Expand { spans, .. } = &sol.transitions[0] {
        assert!(
            spans.iter().all(|s| s.line > 0),
            "subline moves must target lines beyond the anchor: {spans:?}"
        );
    }
}

fn run_dp_oracle_on_corpus(name: &str) -> usize {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use serde::Deserialize;

    use crate::parse::parse_diff;

    #[derive(Deserialize)]
    struct Fixture {
        diff: String,
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = if manifest.join("tests/golden").exists() {
        manifest.join("tests/golden")
    } else {
        manifest.join("../../tests/golden")
    };
    let diff = {
        let corpus_dir = dir.join(name);
        let patch_gz = corpus_dir.join("diff.patch.gz");
        let patch_plain = corpus_dir.join("diff.patch");
        let legacy_gz = dir.join(format!("{name}.json.gz"));
        let legacy_plain = dir.join(format!("{name}.json"));
        if patch_gz.exists() {
            let bytes = fs::read(&patch_gz)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", patch_gz.display()));
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut s = String::new();
            decoder
                .read_to_string(&mut s)
                .expect("valid decompressed gzip");
            s
        } else if patch_plain.exists() {
            fs::read_to_string(&patch_plain)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", patch_plain.display()))
        } else if legacy_gz.exists() {
            let bytes = fs::read(&legacy_gz)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", legacy_gz.display()));
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut s = String::new();
            decoder
                .read_to_string(&mut s)
                .expect("valid decompressed gzip");
            let f: Fixture = serde_json::from_str(&s).unwrap();
            f.diff
        } else {
            let raw = fs::read_to_string(&legacy_plain)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", legacy_plain.display()));
            let f: Fixture = serde_json::from_str(&raw).unwrap();
            f.diff
        }
    };
    let files = Arc::new(parse_diff(&diff));
    let num_threads = available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let counter = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for thread_idx in 0..num_threads {
        let files = Arc::clone(&files);
        let counter = Arc::clone(&counter);
        let name_str = name.to_string();
        handles.push(spawn(move || {
            for (idx, f) in files.iter().enumerate() {
                if idx % num_threads != thread_idx {
                    continue;
                }
                let ext = f.path.rsplit('.').next().unwrap_or("");
                let prof = lang_profile(ext);
                for h in &f.hunks {
                    for seg in partition_hunk_segments(h, None, None, prof) {
                        if let HunkSegment::Cluster(c) = seg {
                            let sol = solve_cluster(&c, prof);
                            verify_projection(&sol, &c).unwrap_or_else(|e| {
                                panic!("[{name_str}] {}: projection invariant failed: {e}", f.path)
                            });
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    counter.load(Ordering::Relaxed)
}

#[test]
fn dp_oracle_diffuse() {
    assert!(run_dp_oracle_on_corpus("diffuse-2026-08") > 0);
}

#[test]
fn dp_oracle_opt_1() {
    assert!(run_dp_oracle_on_corpus("opt-1-cache-regex-window-slice") > 0);
}

#[test]
fn dp_oracle_opt_2() {
    assert!(run_dp_oracle_on_corpus("opt-2-cache-markdown-regex") > 0);
}

#[test]
fn dp_oracle_opt_3() {
    assert!(run_dp_oracle_on_corpus("opt-3-cache-lang-profile") > 0);
}

#[test]
fn dp_oracle_opt_4() {
    assert!(run_dp_oracle_on_corpus("opt-4-manual-scanner-prefilter") > 0);
}

#[test]
fn dp_oracle_diffuse_history_mid() {
    assert!(run_dp_oracle_on_corpus("diffuse-history-mid") > 0);
}

// ---- Head-anchor detection (Phase 4 validation) ----
//
// The block anchor must favor the line that continues the source line's
// head (its first matched token), so a 1-to-K expansion lays the `Mod` on
// the `use std::{` line and reflows the tail — matching the legacy wire.

use crate::similarity::TokenInterner;

#[test]
fn block_anchor_fires_for_use_std_brace() {
    let old = "use std::{ return self.name.clone(); }";
    let new_lines = ["use std::{", "    return self.name.clone(); }"];

    let mut interner = TokenInterner::new();
    let old_tokens = dp_tokens(&mut interner, old);
    let new_tokens: Vec<_> = new_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();

    let bm = block_lcs(&old_tokens, &new_tokens, &interner);

    // Diagnostics: show which lines have the head token (old index 0) matched.
    for (li, s) in bm.per_line.iter().enumerate() {
        for &ti in s {
            if bm.pair_of.get(&(li, ti)) == Some(&0) {
                eprintln!("head token 0 matched on block line {li}");
            }
        }
    }
    eprintln!("per_line match counts: {:?}", bm.per_line);
    eprintln!("pair_of: {:?}", bm.pair_of);

    let anchor = block_anchor(&bm, true);
    eprintln!("block_anchor -> {anchor}");

    assert_eq!(
        anchor, 0,
        "anchor should be line 0 (the `use std::{{` head), got {anchor}"
    );
}

#[test]
fn block_anchor_fires_for_fn_signature() {
    let old = "fn render(&self) -> String { return self.name.clone(); }";
    let new_lines = [
        "fn render(&self) -> String {",
        "    return self.name.clone(); }",
    ];

    let mut interner = TokenInterner::new();
    let old_tokens = dp_tokens(&mut interner, old);
    let new_tokens: Vec<_> = new_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();

    let bm = block_lcs(&old_tokens, &new_tokens, &interner);
    let anchor = block_anchor(&bm, true);
    eprintln!("fn block_anchor -> {anchor}");
    assert_eq!(
        anchor, 0,
        "anchor should be line 0 (the `fn` head), got {anchor}"
    );
}

#[test]
fn test_mirage_cache_cluster() {
    let old =
        "use std::{cell::RefCell, num::NonZeroUsize, os::unix::fs::MetadataExt, path::PathBuf};";
    let new_lines = [
        "use std::{",
        "    cell::RefCell,",
        "    num::NonZeroUsize,",
        "    os::unix::fs::MetadataExt,",
        "    path::PathBuf,",
        "    sync::atomic::{AtomicU64, Ordering},",
        "};",
    ];

    use crate::{
        cluster::{ChangeCluster, dp},
        parse::{AddLine, DelLine, LineNum},
    };

    let old_rows = vec![Row::Del(DelLine::new(LineNum::new(1).unwrap(), old))];
    let new_rows = new_lines
        .iter()
        .enumerate()
        .map(|(i, &s)| Row::Add(AddLine::new(LineNum::new(i as u32 + 1).unwrap(), s)))
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let rs = lang_profile("rs");
    let sol = dp::solve_cluster(&cluster, rs);
    eprintln!("SOL ROWS:");
    for r in &sol.rows {
        eprintln!(
            "  {:?} (old={:?}, new={:?}): {:?}",
            r.kind(),
            r.old_line(),
            r.new_line(),
            r.text()
        );
    }
    eprintln!("SOL TRANSITIONS: {:?}", sol.transitions);
    eprintln!("SOL SCORE: {}", sol.score);
}

#[test]
fn test_mirage_lookup_cluster() {
    use crate::{
        cluster::{ChangeCluster, dp},
        parse::{AddLine, DelLine, LineNum},
    };

    let old_rows = vec![
        Row::Del(DelLine::new(
            LineNum::new(109).unwrap(),
            "            return self.results.borrow_mut().get(key).cloned();",
        )),
        Row::Del(DelLine::new(
            LineNum::new(111).unwrap(),
            "        self.cache.borrow_mut().pop(key);",
        )),
        Row::Del(DelLine::new(
            LineNum::new(112).unwrap(),
            "        self.results.borrow_mut().pop(key);",
        )),
        Row::Del(DelLine::new(LineNum::new(113).unwrap(), "        None")),
    ];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(143).unwrap(),
            "            self.cache.borrow_mut().pop(key);",
        )),
        Row::Add(AddLine::new(
            LineNum::new(144).unwrap(),
            "            self.results.borrow_mut().pop(key);",
        )),
        Row::Add(AddLine::new(
            LineNum::new(145).unwrap(),
            "            return None;",
        )),
        Row::Add(AddLine::new(
            LineNum::new(147).unwrap(),
            "        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();",
        )),
        Row::Add(AddLine::new(
            LineNum::new(148).unwrap(),
            "        if entry_epoch != epoch {",
        )),
        Row::Add(AddLine::new(
            LineNum::new(149).unwrap(),
            "            // Computed under an older mount state: the epoch bump (mode flip,",
        )),
        Row::Add(AddLine::new(
            LineNum::new(150).unwrap(),
            "            // mount mutation) invalidated it.",
        )),
        Row::Add(AddLine::new(
            LineNum::new(151).unwrap(),
            "            self.cache.borrow_mut().pop(key);",
        )),
        Row::Add(AddLine::new(
            LineNum::new(152).unwrap(),
            "            self.results.borrow_mut().pop(key);",
        )),
        Row::Add(AddLine::new(
            LineNum::new(153).unwrap(),
            "            return None;",
        )),
        Row::Add(AddLine::new(LineNum::new(154).unwrap(), "        }")),
        Row::Add(AddLine::new(
            LineNum::new(155).unwrap(),
            "        Some(result)",
        )),
    ];

    let cluster = ChangeCluster {
        preceding_ctx: vec!["        if !fingerprint.is_valid() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["    }"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let rs = lang_profile("rs");
    let sol = dp::solve_cluster(&cluster, rs);
    eprintln!("LOOKUP SOL ROWS:");
    for r in &sol.rows {
        eprintln!(
            "  {:?} (old={:?}, new={:?}): {:?}",
            r.kind(),
            r.old_line(),
            r.new_line(),
            r.text()
        );
    }
    eprintln!("LOOKUP SOL TRANSITIONS: {:?}", sol.transitions);
    eprintln!("LOOKUP SOL SCORE: {}", sol.score);
}

#[test]
fn test_doc_comment_partial_truncation_pairs_as_mod() {
    let rs = lang_profile("rs");
    let old_rows = vec![
        Row::Del(DelLine::new(
            LineNum::new(4).unwrap(),
            "//! run UNCHANGED on top. The target: the frozen `mirage-2025-08` wire,",
        )),
        Row::Del(DelLine::new(
            LineNum::new(5).unwrap(),
            "//! byte-identical; other corpora are allowed drift while the DP replaces",
        )),
        Row::Del(DelLine::new(
            LineNum::new(6).unwrap(),
            "//! the greedy heuristics.",
        )),
    ];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(4).unwrap(),
        "//! run UNCHANGED on top.",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["//! Header"],
        old_rows,
        new_rows,
        following_ctx: vec!["//!"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::DocComment,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert_eq!(sol.rows.len(), 3);
    assert!(matches!(sol.rows[0], Row::Mod { .. }));
    assert_eq!(sol.rows[0].text(), "//! run UNCHANGED on top.");
    assert_eq!(sol.rows[0].old_line(), Some(4));
    assert_eq!(sol.rows[0].new_line(), Some(4));
    assert!(matches!(sol.rows[1], Row::Del(_)));
    assert!(matches!(sol.rows[2], Row::Del(_)));
}

#[test]
fn test_collapse_hunk_doc_comment_partial_truncation() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 3,
        old_count: 5,
        new_start: 3,
        new_count: 3,
        section: String::new(),
        above_ctx: 1,
        below_ctx: 1,
        rows: vec![
            Row::Ctx(CtxLine::new(
                LineNum::new(3).unwrap(),
                LineNum::new(3).unwrap(),
                "//! Context above",
            )),
            Row::Del(DelLine::new(
                LineNum::new(4).unwrap(),
                "//! run UNCHANGED on top. The target: the frozen `mirage-2025-08` wire,",
            )),
            Row::Del(DelLine::new(
                LineNum::new(5).unwrap(),
                "//! byte-identical; other corpora are allowed drift while the DP replaces",
            )),
            Row::Del(DelLine::new(
                LineNum::new(6).unwrap(),
                "//! the greedy heuristics.",
            )),
            Row::Add(AddLine::new(
                LineNum::new(4).unwrap(),
                "//! run UNCHANGED on top.",
            )),
            Row::Ctx(CtxLine::new(
                LineNum::new(7).unwrap(),
                LineNum::new(5).unwrap(),
                "//! Context below",
            )),
        ],
    };
    let collapsed = crate::collapse::collapse_hunk_rows(&h, None, None, rs);
    assert_eq!(collapsed.len(), 5);
    assert!(matches!(collapsed[0], Row::Ctx(_)));
    assert!(matches!(collapsed[1], Row::Mod { .. }));
    assert_eq!(collapsed[1].text(), "//! run UNCHANGED on top.");
    assert_eq!(collapsed[1].old_line(), Some(4));
    assert_eq!(collapsed[1].new_line(), Some(4));
    assert!(matches!(collapsed[2], Row::Del(_)));
    assert!(matches!(collapsed[3], Row::Del(_)));
    assert!(matches!(collapsed[4], Row::Ctx(_)));
}

#[test]
fn test_weights_min_containment_len_tuning() {
    let rs = lang_profile("rs");
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(1).unwrap(),
        "let result = foo();",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(1).unwrap(),
        "let result = foo(); // suffix comment",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn test() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    // Default weights (min_containment_len = 8) pairs as Mod
    let sol_default = dp::solve_cluster_with(&cluster, rs, &dp::Weights::default());
    assert_eq!(sol_default.rows.len(), 1);
    assert!(matches!(sol_default.rows[0], Row::Mod { .. }));

    let custom_w = dp::Weights {
        min_containment_len: 100,
        ..Default::default()
    };
    assert_eq!(dp::DEFAULT_MIN_CONTAINMENT_LEN, 8);
    assert_eq!(custom_w.min_containment_len, 100);
}

#[test]
fn test_rewritten_comment_block_does_not_pair_across_offset_drift() {
    let rs = lang_profile("rs");
    let old_rows = vec![
        Row::Del(DelLine::new(
            LineNum::new(132).unwrap(),
            "    // A del run whose FIRST line did not move is a deleted block, not a",
        )),
        Row::Del(DelLine::new(
            LineNum::new(133).unwrap(),
            "    // relocation — the interior \"moved\" lines are coincidental same-text",
        )),
        Row::Del(DelLine::new(
            LineNum::new(134).unwrap(),
            "    // matches (the old `find_mount`'s `.iter()/.rfind()/.map()` chain",
        )),
        Row::Del(DelLine::new(
            LineNum::new(135).unwrap(),
            "    // verbatim-matching lines inside the NEW `get` method). The move would",
        )),
        Row::Del(DelLine::new(
            LineNum::new(136).unwrap(),
            "    // render the new method's chain as ≈ inside a clearly-added block —",
        )),
        Row::Del(DelLine::new(
            LineNum::new(137).unwrap(),
            "    // \"`self.filesystems` added, the rest moved?\" — misleading. The whole",
        )),
        Row::Del(DelLine::new(
            LineNum::new(138).unwrap(),
            "    // deleted block reads as deleted; the new method reads as added.",
        )),
    ];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(133).unwrap(),
            "    // Block Move Cohesion Rule:",
        )),
        Row::Add(AddLine::new(
            LineNum::new(134).unwrap(),
            "    // In a multi-line deletion run (e.g. deleting a function), isolated matches",
        )),
        Row::Add(AddLine::new(
            LineNum::new(135).unwrap(),
            "    // (e.g. single-line attributes like `#[test]`, closing braces `}`, or",
        )),
        Row::Add(AddLine::new(
            LineNum::new(136).unwrap(),
            "    // interior method calls) are coincidental boilerplate, NOT code",
        )),
        Row::Add(AddLine::new(
            LineNum::new(137).unwrap(),
            "    // relocations.",
        )),
        Row::Add(AddLine::new(LineNum::new(138).unwrap(), "    //")),
        Row::Add(AddLine::new(
            LineNum::new(139).unwrap(),
            "    // A deletion run is only marked moved if:",
        )),
        Row::Add(AddLine::new(
            LineNum::new(140).unwrap(),
            "    // - The ENTIRE deletion run moved, OR",
        )),
        Row::Add(AddLine::new(
            LineNum::new(141).unwrap(),
            "    // - A contiguous prefix block of >= 2 lines starting at run[0] moved.",
        )),
        Row::Add(AddLine::new(
            LineNum::new(142).unwrap(),
            "    // Otherwise, all lines in the deletion run remain normal deletions.",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["    }"],
        old_rows,
        new_rows,
        following_ctx: vec!["    let mut unmark_positions: Vec<RowPos> = Vec::new();"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::LineComment,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.rows.iter().all(|r| !matches!(r, Row::Mod { .. })),
        "completely rewritten comment block must NOT pair random lines as Mod"
    );
}

#[test]
fn test_weights_phase1_defaults() {
    let w = dp::Weights::default();
    assert_eq!(w.max_indent_shift, 16);
    assert_eq!(w.min_tail_matched_chars, 8);
    assert!((w.min_block_line_coverage - 0.50).abs() < 1e-6);
}

#[test]
fn test_symmetrical_block_reflow_involution() {
    let rs = lang_profile("rs");
    // Expand (1-to-2): statement head indentation matches (4 spaces)
    let old_rows_expand = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "    let result = compute_transform(alpha, beta);",
    ))];
    let new_rows_expand = vec![
        Row::Add(AddLine::new(
            LineNum::new(20).unwrap(),
            "    let result = compute_transform(",
        )),
        Row::Add(AddLine::new(
            LineNum::new(21).unwrap(),
            "        alpha, beta);",
        )),
    ];
    let cluster_expand = ChangeCluster {
        preceding_ctx: vec!["fn transform() {"],
        old_rows: old_rows_expand,
        new_rows: new_rows_expand,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol_expand = dp::solve_cluster(&cluster_expand, rs);
    assert!(
        sol_expand
            .transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Expand { .. })),
        "1-to-2 expand with matching head indentation must pair as DpOp::Expand"
    );

    // Collapse (2-to-1): exact inverse under diff reversal
    let old_rows_collapse = vec![
        Row::Del(DelLine::new(
            LineNum::new(20).unwrap(),
            "    let result = compute_transform(",
        )),
        Row::Del(DelLine::new(
            LineNum::new(21).unwrap(),
            "        alpha, beta);",
        )),
    ];
    let new_rows_collapse = vec![Row::Add(AddLine::new(
        LineNum::new(10).unwrap(),
        "    let result = compute_transform(alpha, beta);",
    ))];
    let cluster_collapse = ChangeCluster {
        preceding_ctx: vec!["fn transform() {"],
        old_rows: old_rows_collapse,
        new_rows: new_rows_collapse,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol_collapse = dp::solve_cluster(&cluster_collapse, rs);
    assert!(
        sol_collapse
            .transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Collapse { .. })),
        "2-to-1 collapse with matching head indentation must pair as DpOp::Collapse symmetrically"
    );
}

#[test]
fn test_block_reflow_disqualified_on_head_indent_mismatch() {
    let rs = lang_profile("rs");
    // 1-to-2 candidate with mismatched head indentation (0 vs 4 spaces)
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "let result = compute_transform(alpha, beta);",
    ))];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(20).unwrap(),
            "    let result = compute_transform(",
        )),
        Row::Add(AddLine::new(
            LineNum::new(21).unwrap(),
            "        alpha, beta);",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn transform() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Expand { .. } | dp::DpOp::Collapse { .. })),
        "expand exceeding max_indent_shift must be disqualified from block pairing"
    );
}

#[test]
fn test_block_line_coverage_allows_unmatched_formatting_lines() {
    let rs = lang_profile("rs");
    // 1-to-3 expand where 2 lines match content tokens but 1 line has an
    // unmatched parameter
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "let config = ClientConfigBuilder::new().with_timeout(30).build();",
    ))];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(20).unwrap(),
            "let config = ClientConfigBuilder::new()",
        )),
        Row::Add(AddLine::new(
            LineNum::new(21).unwrap(),
            "    .with_debug_payload()",
        )),
        Row::Add(AddLine::new(
            LineNum::new(22).unwrap(),
            "    .with_timeout(30).build();",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn setup() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    // 2 out of 3 lines match content (coverage 66.7% >= min_block_line_coverage
    // 50%), so the block reflow should be accepted rather than aborted by
    // count == 0.
    assert!(
        sol.transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Expand { .. })),
        "1-to-3 expand with 2/3 line coverage must pair as DpOp::Expand"
    );
}

#[test]
fn test_block_line_coverage_rejects_insufficient_matches() {
    let rs = lang_profile("rs");
    // 1-to-3 candidate where only 1 line matches content tokens (1/3 = 33% <
    // 50% threshold)
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "let config = ClientConfigBuilder::new();",
    ))];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(20).unwrap(),
            "let config = ClientConfigBuilder::new();",
        )),
        Row::Add(AddLine::new(
            LineNum::new(21).unwrap(),
            "    let completely_unrelated = other_call();",
        )),
        Row::Add(AddLine::new(
            LineNum::new(22).unwrap(),
            "    another_statement();",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn setup() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Expand { .. })),
        "1-to-3 candidate with only 1/3 line coverage must NOT pair as DpOp::Expand"
    );
}

#[test]
fn test_prefix_containment_within_max_indent_shift_pairs_as_mod() {
    let rs = lang_profile("rs");
    // Indentation shift of 8 spaces (within max_indent_shift = 16)
    // similarity_ratio = 16 / 102 = 0.156 (< 0.25 threshold), so this ONLY
    // pairs as Mod if prefix containment succeeds.
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "    unmark_positions",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "            unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Mod { .. })),
        "statement prefix containment within max_indent_shift must pair as Mod"
    );
}

#[test]
fn test_prefix_containment_exceeding_max_indent_shift_is_rejected() {
    let rs = lang_profile("rs");
    // Indentation shift of 24 spaces (exceeds max_indent_shift = 16)
    // similarity_ratio = 16 / 102 = 0.156 (< 0.25 threshold), so with prefix
    // containment rejected, it must NOT pair as Mod.
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "unmark_positions",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "                        unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Mod { .. })),
        "statement prefix containment exceeding max_indent_shift must NOT pair as Mod"
    );
}

#[test]
fn test_prefix_containment_reverse_direction_pairs_as_mod() {
    let rs = lang_profile("rs");
    // Reverse direction: old row is the longer line, new row is the prefix
    // receiver
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "            unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "    unmark_positions",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Mod { .. })),
        "reverse prefix containment (longer old line -> shorter new line) must pair as Mod"
    );
}

#[test]
fn test_prefix_containment_non_boundary_subword_rejected() {
    let rs = lang_profile("rs");
    // New line has unmark_positions as a prefix substring, but not at a token
    // boundary ('_' continues the identifier).
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "    unmark_positions",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "            unmark_positions_different_call(run.iter().copied().filter(|p| moved_dels.contains(p)));",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Mod { .. })),
        "prefix match not at a token boundary must NOT pair as Mod"
    );
}

#[test]
fn test_prefix_containment_comment_mismatch_rejected() {
    let rs = lang_profile("rs");
    // Old row is a comment, new row is code
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "    // unmark_positions",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "            unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Mod { .. })),
        "comment vs code mismatch must NOT pair as Mod via prefix containment"
    );
}

#[test]
fn test_unrelated_tail_statements_not_paired_on_zero_indent_diff() {
    let rs = lang_profile("rs");
    // Two completely unrelated lines before closing delimiter with same indent:
    // Old: alpha();
    // New: zzzzz = 42;
    // Both followed by "}" (close_dist == 1). Indent diff == 0.0.
    // They share 0 tokens and should NOT be paired as Mod!
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(10).unwrap(),
        "    alpha();",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(20).unwrap(),
        "    zzzzz = 42;",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["fn process() {"],
        old_rows,
        new_rows,
        following_ctx: vec!["}"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert!(
        sol.transitions
            .iter()
            .all(|t| !matches!(t, dp::DpOp::Mod { .. })),
        "unrelated tail statements must NOT pair as Mod solely because indent_diff == 0.0"
    );
}

#[test]
fn test_section_headers_distinguish_toml_from_rust_and_python() {
    let toml = lang_profile("toml");
    let rs = lang_profile("rs");
    let py = lang_profile("py");

    // In TOML, [section] is a statement/section start anchor candidate
    assert!(dp::is_statement_start("[dependencies]", toml));
    assert!(dp::is_statement_start("[package]", toml));

    // In Rust / Python, [item] is an expression, NOT a section header statement
    // start
    assert!(!dp::is_statement_start("[item]", rs));
    assert!(!dp::is_statement_start("[item]", py));
}

#[test]
fn test_scope_restricted_deletion_pairing() {
    use crate::{
        cluster::pairing::dp_pairing,
        parse::{AddLine, CtxLine, DelLine, Hunk, LineNum},
    };

    let rs = lang_profile("rs");
    let old_content: &[&str] = &[
        "fn alpha() {",
        "    let count = 0;",
        "    process();",
        "}",
        "",
        "fn beta() {",
        "    let count = 0;",
        "    other();",
        "}",
    ];
    let new_content: &[&str] = &[
        "fn alpha() {",
        "    let total = 0;",
        "    process();",
        "}",
        "",
        "fn beta() {",
        "    let count = 0;",
        "    other();",
        "}",
    ];

    let h = Hunk {
        old_start: 1,
        old_count: 4,
        new_start: 1,
        new_count: 4,
        section: String::new(),
        rows: vec![
            Row::Ctx(CtxLine::new(
                LineNum::new(1).unwrap(),
                LineNum::new(1).unwrap(),
                "fn alpha() {",
            )),
            Row::Del(DelLine::new(LineNum::new(2).unwrap(), "    let count = 0;")),
            Row::Add(AddLine::new(LineNum::new(2).unwrap(), "    let total = 0;")),
            Row::Ctx(CtxLine::new(
                LineNum::new(3).unwrap(),
                LineNum::new(3).unwrap(),
                "    process();",
            )),
            Row::Ctx(CtxLine::new(
                LineNum::new(4).unwrap(),
                LineNum::new(4).unwrap(),
                "}",
            )),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let results = dp_pairing(&h, Some(old_content), Some(new_content), rs);
    // Row 2 (Add "let total = 0;") must pair with Row 1 (Del "let count = 0;")
    // despite "let count = 0;" existing in unrelated fn beta() in new_content!
    assert_eq!(results.pair_of.get(&2), Some(&1));
}

#[test]
fn test_dp_pairing_disqualifies_zero_common_substitutions_upstream() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 1,
        old_count: 2,
        new_start: 1,
        new_count: 2,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(LineNum::new(1).unwrap(), "alpha")),
            Row::Add(AddLine::new(LineNum::new(1).unwrap(), "omega")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let results = crate::cluster::pairing::dp_pairing(&h, None, None, rs);
    // Row 1 (Add "omega") and Row 0 (Del "alpha") share 0 common tokens and 0
    // common indentation. DP must mathematically disqualify them upstream
    // and NEVER pair them as Mod!
    assert!(
        !results.pair_of.contains_key(&1),
        "Disjoint lines sharing 0 common characters must not be paired as Mod (got pair: {:?})",
        results.pair_of.get(&1)
    );
}

#[test]
fn test_is_statement_start_head_pairing() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 512,
        old_count: 4,
        new_start: 515,
        new_count: 1,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(
                LineNum::new(512).unwrap(),
                "pub(crate) fn is_statement_start(",
            )),
            Row::Del(DelLine::new(LineNum::new(513).unwrap(), "    text: &str,")),
            Row::Del(DelLine::new(
                LineNum::new(514).unwrap(),
                "    profile: Option<&crate::syntax::profile::LangProfile>,",
            )),
            Row::Del(DelLine::new(LineNum::new(515).unwrap(), ") -> bool {")),
            Row::Add(AddLine::new(
                LineNum::new(515).unwrap(),
                "pub(crate) fn is_statement_start(text: &str, profile: Option<&LangProfile>) -> bool {",
            )),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let c = crate::cluster::ChangeCluster {
        preceding_ctx: vec![],
        old_rows: h.rows[..4].to_vec(),
        new_rows: h.rows[4..].to_vec(),
        following_ctx: vec![],
        del_survival: rustc_hash::FxHashSet::default(),
        section: crate::cluster::SyntaxSectionKind::Code,
    };
    let mut arena = crate::cluster::dp::DpScratchArena::default();
    let sol = crate::cluster::dp::solve_cluster_with_arena(
        &c,
        rs,
        &crate::cluster::dp::Weights::default(),
        &mut arena,
    );
    assert!(
        matches!(
            sol.transitions.first(),
            Some(crate::cluster::dp::DpOp::Collapse { anchor: 0, .. })
        ),
        "Multi-line function signature collapse must anchor on line 0 (got: {:?})",
        sol.transitions
    );

    let results = crate::cluster::pairing::dp_pairing(&h, None, None, rs);
    assert_eq!(
        results.pair_of.get(&4),
        Some(&0),
        "Function signature header should pair with new full function signature, got: {:?}",
        results.pair_of.get(&4)
    );
}

#[test]
fn test_block_anchor_prefers_overwhelming_content_over_trivial_prefix_head() {
    let old = "    syntax::prelude::{CloseDelimiter, first_ascii_word},";
    let new_lines = [
        "    syntax::{",
        "        prelude::{CloseDelimiter, first_ascii_word},",
    ];

    let mut interner = TokenInterner::new();
    let old_tokens = dp_tokens(&mut interner, old);
    let new_tokens: Vec<_> = new_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();

    let bm = block_lcs(&old_tokens, &new_tokens, &interner);
    let anchor = block_anchor(&bm, false);
    assert_eq!(
        anchor, 1,
        "anchor should be line 1 (the content carrying prelude::{{...}}), not line 0 (trivial prefix), got {anchor}"
    );
}

#[test]
fn test_use_crate_expand_anchors_on_statement_head() {
    let old = "use crate::core::counts::{CountKind, FileCounts};";
    let new_lines = [
        "use crate::{",
        "    core::counts::{CountKind, FileCounts},",
        "    theme::ThemePalette,",
        "};",
    ];

    let mut interner = TokenInterner::new();
    let old_tokens = dp_tokens(&mut interner, old);
    let new_tokens: Vec<_> = new_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();

    let bm = block_lcs(&old_tokens, &new_tokens, &interner);
    let anchor = block_anchor(&bm, true);
    assert_eq!(
        anchor, 0,
        "anchor should be line 0 (`use crate::{{` statement head), not line 1 (`core::counts`), got {anchor}"
    );

    // Assignment line wraps (`let var = \n expr`) should anchor on the
    // expression (line 1), preserving sub-line diffs within the expression.
    let old_score = "    let score_mid = semantic_boundary_score(Some(\"        let x = 1;\"), Some(\"        let y = 2;\"));";
    let new_score_lines = [
        "    let score_mid =",
        "        semantic_boundary_score(Some(\"        let x = 1;\"), Some(\"        let y = 2;\"), rs);",
    ];
    let old_score_tok = dp_tokens(&mut interner, old_score);
    let new_score_tok: Vec<_> = new_score_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();
    let bm_score = block_lcs(&old_score_tok, &new_score_tok, &interner);
    assert_eq!(
        block_anchor(&bm_score, false),
        1,
        "Assignment wrap should anchor on the substantive expression on line 1"
    );

    let old_sb = "                    let sb_char = if row_i >= thumb_offset && row_i < thumb_offset + thumb_size {";
    let new_sb_lines = [
        "                    let sb_char =",
        "                        if track_row >= thumb_offset && track_row < thumb_offset + thumb_size {",
    ];
    let old_sb_tok = dp_tokens(&mut interner, old_sb);
    let new_sb_tok: Vec<_> = new_sb_lines
        .iter()
        .map(|s| dp_tokens(&mut interner, s))
        .collect();
    let bm_sb = block_lcs(&old_sb_tok, &new_sb_tok, &interner);
    assert_eq!(
        block_anchor(&bm_sb, false),
        1,
        "Assignment wrap should anchor on the substantive expression on line 1"
    );

    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 9,
        old_count: 1,
        new_start: 8,
        new_count: 4,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(
                LineNum::new(9).unwrap(),
                "use crate::core::counts::{CountKind, FileCounts};",
            )),
            Row::Add(AddLine::new(LineNum::new(8).unwrap(), "use crate::{")),
            Row::Add(AddLine::new(
                LineNum::new(9).unwrap(),
                "    core::counts::{CountKind, FileCounts},",
            )),
            Row::Add(AddLine::new(
                LineNum::new(10).unwrap(),
                "    theme::ThemePalette,",
            )),
            Row::Add(AddLine::new(LineNum::new(11).unwrap(), "};")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let results = crate::cluster::pairing::dp_pairing(&h, None, None, rs);
    assert_eq!(
        results.pair_of.get(&1),
        Some(&0),
        "Statement head 'use crate::{{' must be paired with the old 'use crate::...' line, got {:?}",
        results.pair_of
    );
}

#[test]
fn test_dp_pairing_populates_expand_blocks() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 53,
        old_count: 1,
        new_start: 53,
        new_count: 4,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(
                LineNum::new(53).unwrap(),
                "    syntax::prelude::{CloseDelimiter, first_ascii_word},",
            )),
            Row::Add(AddLine::new(LineNum::new(53).unwrap(), "    syntax::{")),
            Row::Add(AddLine::new(
                LineNum::new(54).unwrap(),
                "        prelude::{CloseDelimiter, first_ascii_word},",
            )),
            Row::Add(AddLine::new(
                LineNum::new(55).unwrap(),
                "        profile::LangProfile,",
            )),
            Row::Add(AddLine::new(LineNum::new(56).unwrap(), "    },")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let results = crate::cluster::pairing::dp_pairing(&h, None, None, rs);
    // Anchor is row 2 (new line 54) paired with row 0 (old line 53)
    assert_eq!(results.pair_of.get(&2), Some(&0));
    assert!(
        results.expand_blocks.contains_key(&2),
        "expand_blocks must track the multi-line expansion block anchored at row 2"
    );
    assert_eq!(
        results.expand_blocks.get(&2).unwrap(),
        &vec![1, 2],
        "expand_blocks must contain all addition row indices in the expansion"
    );
}

#[test]
fn test_expansion_companion_adds_receive_donor_spans() {
    let rs = lang_profile("rs");
    let h = Hunk {
        old_start: 53,
        old_count: 1,
        new_start: 53,
        new_count: 4,
        section: String::new(),
        rows: vec![
            Row::Del(DelLine::new(
                LineNum::new(53).unwrap(),
                "    syntax::prelude::{CloseDelimiter, first_ascii_word},",
            )),
            Row::Add(AddLine::new(LineNum::new(53).unwrap(), "    syntax::{")),
            Row::Add(AddLine::new(
                LineNum::new(54).unwrap(),
                "        prelude::{CloseDelimiter, first_ascii_word},",
            )),
            Row::Add(AddLine::new(
                LineNum::new(55).unwrap(),
                "        profile::LangProfile,",
            )),
            Row::Add(AddLine::new(LineNum::new(56).unwrap(), "    },")),
        ],
        above_ctx: 0,
        below_ctx: 0,
    };

    let collapsed = crate::collapse::collapse_hunk_rows(&h, None, None, rs);
    assert_eq!(collapsed.len(), 4);
    match &collapsed[0] {
        Row::Add(add_line) => {
            assert_eq!(add_line.text, "    syntax::{");
            assert_eq!(
                add_line.del_text.as_deref(),
                Some("    syntax::prelude::{CloseDelimiter, first_ascii_word},"),
                "Companion prefix addition must link to the donor line"
            );
            assert!(
                !add_line.spans.add.contains(&(0, add_line.text.len())),
                "Companion prefix addition must not be marked as a wholesale addition"
            );
        }
        other => panic!("Expected Row::Add for prefix row, got {:?}", other),
    }
}

#[test]
fn test_cluster_product_cap_falls_back_to_wholesale() {
    let old_rows: Vec<Row> = (1..=20)
        .map(|i| {
            Row::Del(DelLine::new(
                LineNum::new(i).unwrap(),
                format!("let a{i} = {i};"),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = (1..=20)
        .map(|i| {
            Row::Add(AddLine::new(
                LineNum::new(i).unwrap(),
                format!("let a{i} = {i} + 1;"),
            ))
        })
        .collect();
    let cluster = ChangeCluster {
        old_rows,
        new_rows,
        preceding_ctx: vec![],
        following_ctx: vec![],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    // Cap at 100: 20 * 20 = 400 > 100, falls back to wholesale (20 dels + 20
    // adds)
    dp::set_cluster_product_cap(100);
    let sol_capped = dp::solve_cluster_with(&cluster, None, &dp::Weights::default());
    assert_eq!(sol_capped.rows.len(), 40);
    assert_eq!(sol_capped.transitions.len(), 40);
    assert!(
        sol_capped
            .transitions
            .iter()
            .take(20)
            .all(|t| matches!(t, dp::DpOp::Del))
    );
    assert!(
        sol_capped
            .transitions
            .iter()
            .skip(20)
            .all(|t| matches!(t, dp::DpOp::Add))
    );

    // Reset to default
    dp::set_cluster_product_cap(dp::DEFAULT_CLUSTER_PRODUCT_CAP);
    let sol_uncapped = dp::solve_cluster_with(&cluster, None, &dp::Weights::default());
    // Under uncapped DP, the 20 similar lines are paired into Mod rows
    assert!(
        sol_uncapped
            .transitions
            .iter()
            .any(|t| matches!(t, dp::DpOp::Mod { .. }))
    );
}

#[test]
fn test_skyline_dp_2000_line_wholesale_replacement_pairs_into_mods() {
    use crate::{
        cluster::{ChangeCluster, dp},
        parse::{AddLine, DelLine, LineNum},
    };

    let count = 2000;
    let old_strings: Vec<String> = (0..count)
        .map(|i| format!("    let counter_{i:04} = compute_value({i}, false);"))
        .collect();
    let new_strings: Vec<String> = (0..count)
        .map(|i| format!("    let counter_{i:04} = compute_value({i}, true);"))
        .collect();

    let old_rows: Vec<Row> = old_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Del(DelLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = new_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Add(AddLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let sol = dp::solve_cluster_with(&cluster, None, &dp::Weights::default());

    // All 2,000 lines should be paired into Mod rows rather than 2000 Dels +
    // 2000 Adds
    let mod_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, dp::DpOp::Mod { .. }))
        .count();
    assert_eq!(
        mod_count,
        count,
        "Expected all {count} lines to be paired into Mod rows, got {mod_count} Mods and {} total rows",
        sol.rows.len()
    );
    assert_eq!(sol.rows.len(), count);
}

#[test]
fn test_skyline_dp_7000_line_wholesale_replacement_pairs_into_mods() {
    use crate::{
        cluster::{ChangeCluster, dp},
        parse::{AddLine, DelLine, LineNum},
    };

    let count = 7183;
    let old_strings: Vec<String> = (0..count)
        .map(|i| format!("    let counter_{i:04} = compute_value({i}, false);"))
        .collect();
    let new_strings: Vec<String> = (0..count)
        .map(|i| format!("    let counter_{i:04} = compute_value({i}, true);"))
        .collect();

    let old_rows: Vec<Row> = old_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Del(DelLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = new_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Add(AddLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let sol = dp::solve_cluster_with(&cluster, None, &dp::Weights::default());

    // All 7,183 lines should be paired into Mod rows rather than 7183 Dels +
    // 7183 Adds
    let mod_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, dp::DpOp::Mod { .. }))
        .count();
    assert_eq!(
        mod_count,
        count,
        "Expected all {count} lines to be paired into Mod rows, got {mod_count} Mods and {} total rows",
        sol.rows.len()
    );
    assert_eq!(sol.rows.len(), count);
}

#[test]
fn test_skyline_dp_asymmetric_burst_preserves_alignment() {
    use crate::{
        cluster::{ChangeCluster, dp},
        parse::{AddLine, DelLine, LineNum},
    };

    // 50 lines modified before burst, 40 lines added (asymmetric burst), 50
    // lines modified after burst
    let mut old_strings = Vec::new();
    let mut new_strings = Vec::new();

    for i in 0..50 {
        old_strings.push(format!("let pre_{i} = alpha({i});"));
        new_strings.push(format!("let pre_{i} = alpha_updated({i});"));
    }
    // Asymmetric insertion of 40 lines
    for i in 0..40 {
        new_strings.push(format!("let burst_insert_{i} = new_call({i});"));
    }
    // Post-burst lines
    for i in 0..50 {
        old_strings.push(format!("let post_{i} = omega({i});"));
        new_strings.push(format!("let post_{i} = omega_updated({i});"));
    }

    let old_rows: Vec<Row> = old_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Del(DelLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = new_strings
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Row::Add(AddLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                s.as_str(),
            ))
        })
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let sol = dp::solve_cluster_with(&cluster, None, &dp::Weights::default());
    let mod_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, dp::DpOp::Mod { .. }))
        .count();

    // Both the 50 pre-burst and 50 post-burst lines should be recognized as
    // Mods (100 total Mods)
    assert_eq!(
        mod_count, 100,
        "Expected 100 paired Mod rows (50 pre + 50 post burst), got {mod_count}"
    );
    let add_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, dp::DpOp::Add))
        .count();
    assert_eq!(add_count, 40, "Expected 40 Add rows for burst insertion");
}

fn scoring_meta(trimmed_id: u32, comment: bool) -> DpRowMeta {
    DpRowMeta {
        trimmed_id,
        comment,
        kind: SyntaxSectionKind::Code,
        close_dist: 99,
        ..Default::default()
    }
}

struct ScoringFixture {
    cluster: ChangeCluster<'static>,
    old_tokens: Vec<crate::cluster::dp::DpTokens>,
    new_tokens: Vec<crate::cluster::dp::DpTokens>,
    old_meta: Vec<DpRowMeta>,
    new_meta: Vec<DpRowMeta>,
    interner: crate::similarity::TokenInterner,
    weights: Weights,
}

fn scoring_fixture() -> ScoringFixture {
    // Distinct vocabularies per row so cross pairs share (almost) nothing.
    // Only (0, 0) is a genuine mod candidate.
    let old_texts = [
        "let alpha = 1;",
        "zzz qqq;",
        "",
        "return x;",
        "var z=3;",
        "x1 = 1 + aaaa bbbb cccc dddd;",
    ];
    let new_texts = [
        "let alpha = 2;",
        "aaa bbb;",
        "",
        "return x;",
        "var z = 3;",
        "x1 = 2 + eeee ffff gggg hhhh;",
    ];
    let mut interner = crate::similarity::TokenInterner::new();
    let old_tokens: Vec<_> = old_texts
        .iter()
        .map(|t| dp_tokens(&mut interner, t))
        .collect();
    let new_tokens: Vec<_> = new_texts
        .iter()
        .map(|t| dp_tokens(&mut interner, t))
        .collect();
    // Trimmed ids: shared only by the verbatim pair (3, 3); distinct
    // everywhere else, including the whitespace-only-different pair (4, 4).
    let mut tids = [0u32; 12];
    for (k, t) in old_texts.iter().chain(new_texts.iter()).enumerate() {
        tids[k] = interner.intern(t.trim());
    }
    tids[3] = tids[9];
    tids[10] = tids[10].wrapping_add(1_000_000);
    let old_meta: Vec<_> = (0..6).map(|i| scoring_meta(tids[i], i == 1)).collect();
    let new_meta: Vec<_> = (0..6).map(|i| scoring_meta(tids[6 + i], false)).collect();
    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows: old_texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                Row::Del(DelLine::new(
                    LineNum::new(i as u32 + 1).unwrap(),
                    (*t).to_string(),
                ))
            })
            .collect(),
        new_rows: new_texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                Row::Add(AddLine::new(
                    LineNum::new(i as u32 + 1).unwrap(),
                    (*t).to_string(),
                ))
            })
            .collect(),
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    ScoringFixture {
        cluster,
        old_tokens,
        new_tokens,
        old_meta,
        new_meta,
        interner,
        weights: Weights::default(),
    }
}

fn collect_all(fx: &ScoringFixture) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..6 {
        for j in 0..6 {
            if mod_pair_needs_scoring(
                i,
                j,
                &fx.cluster,
                &fx.old_tokens,
                &fx.new_tokens,
                &fx.old_meta,
                &fx.new_meta,
                &fx.interner,
                None,
                &fx.weights,
            ) {
                out.push((i, j));
            }
        }
    }
    out
}

#[test]
fn mod_pair_candidate_count() {
    // Only the genuinely similar pair survives every guard arm:
    // (1,*) fall on comment mismatch, (2,2) is blank-skipped,
    // (3,3) takes the verbatim Mov path, (4,4) has identical token
    // streams, (5,5) fails the similarity ratio, and everything else
    // shares no tokens at zero indent.
    assert_eq!(collect_all(&scoring_fixture()), vec![(0, 0)]);
}

#[test]
fn test_mod_pair_prunes_incompatible_tail_and_high_indent() {
    let mut interner = TokenInterner::default();
    let weights = Weights::default();
    let old_text = "    let x = foo();";
    let new_text = "                let y = bar();";
    let ot = vec![dp_tokens(&mut interner, old_text)];
    let nt = vec![dp_tokens(&mut interner, new_text)];

    let om = DpRowMeta {
        trimmed_id: 1,
        indent: 4,
        close_dist: 1,
        close_tail: CloseDelimiter::Brace,
        ..Default::default()
    };

    let nm = DpRowMeta {
        trimmed_id: 2,
        indent: 16,
        close_dist: 1,
        close_tail: CloseDelimiter::Paren, // Different delimiter!
        ..Default::default()
    };

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows: vec![Row::Del(DelLine::new(
            LineNum::new(1).unwrap(),
            old_text.to_string(),
        ))],
        new_rows: vec![Row::Add(AddLine::new(
            LineNum::new(1).unwrap(),
            new_text.to_string(),
        ))],
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let needed = mod_pair_needs_scoring(
        0,
        0,
        &cluster,
        &ot,
        &nt,
        &[om],
        &[nm],
        &interner,
        None,
        &weights,
    );
    assert!(
        !needed,
        "pair with mismatching close tails and heavy indent shift must be pruned"
    );
}

#[test]
fn test_parallel_scoring_matches_sequential() {
    let fx = scoring_fixture();
    let candidates = collect_all(&fx);
    assert!(!candidates.is_empty());
    let mut seq = Vec::new();
    let mut par = Vec::new();
    score_lcs_candidates_seq(
        &candidates,
        &fx.old_tokens,
        &fx.new_tokens,
        &fx.interner,
        &mut seq,
    );
    score_lcs_candidates_par(
        &candidates,
        &fx.old_tokens,
        &fx.new_tokens,
        &fx.interner,
        &mut par,
    );
    assert_eq!(seq, par, "parallel scoring must match sequential exactly");
    assert_eq!(seq.len(), candidates.len());
}

#[test]
fn test_parallel_scoring_multi_row_matches_sequential() {
    let mut interner = TokenInterner::default();
    let mut old_tokens = Vec::new();
    let mut new_tokens = Vec::new();

    // Create 30 old rows and 40 new rows, each with varying tokens
    for i in 0..30 {
        let text = format!(
            "fn function_name_{i}(arg_a: usize, arg_b: String) -> Result<i32, Error> {{ val_{i} = {i} * 2; }}"
        );
        old_tokens.push(dp_tokens(&mut interner, &text));
    }
    for j in 0..40 {
        let text = format!(
            "fn function_name_{j}(arg_a: usize, arg_b: String) -> Result<i32, Error> {{ val_{j} = {j} * 3; }}"
        );
        new_tokens.push(dp_tokens(&mut interner, &text));
    }

    // Generate 1200 candidate pairs across all rows (well above chunk size 256)
    let mut candidates = Vec::new();
    for i in 0..30 {
        for j in 0..40 {
            candidates.push((i, j));
        }
    }
    assert_eq!(candidates.len(), 1200);

    let mut seq = Vec::new();
    let mut par = Vec::new();

    score_lcs_candidates_seq(&candidates, &old_tokens, &new_tokens, &interner, &mut seq);
    score_lcs_candidates_par(&candidates, &old_tokens, &new_tokens, &interner, &mut par);

    assert_eq!(
        seq, par,
        "parallel scoring must match sequential scoring across multi-row chunks"
    );
    assert_eq!(seq.len(), 1200);
    // Spot check that identical function signature pairs produce positive
    // character matches
    for i in 0..30 {
        let idx = i * 40 + i; // (i, i)
        assert!(
            seq[idx] > 0,
            "matching pair ({i}, {i}) must have positive score"
        );
    }
}

#[test]
fn test_anchor_decomposition_large_cluster() {
    let rs = lang_profile("rs");
    // Create 300 old lines and 300 new lines where every 3rd line is modified
    let old_lines: Vec<String> = (0..300)
        .map(|i| format!("    fn test_func_{i}() -> usize {{ {i} * 10 }}"))
        .collect();
    let new_lines: Vec<String> = (0..300)
        .map(|i| {
            if i % 3 == 0 {
                format!("    fn test_func_{i}() -> usize {{ {i} * 20 }}")
            } else {
                format!("    fn test_func_{i}() -> usize {{ {i} * 10 }}")
            }
        })
        .collect();

    let old_rows: Vec<Row> = old_lines
        .iter()
        .enumerate()
        .map(|(i, s)| Row::Del(DelLine::new(LineNum::new(i as u32 + 1).unwrap(), s.clone())))
        .collect();
    let new_rows: Vec<Row> = new_lines
        .iter()
        .enumerate()
        .map(|(i, s)| Row::Add(AddLine::new(LineNum::new(i as u32 + 1).unwrap(), s.clone())))
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let mut arena = dp::DpScratchArena::default();
    let decomposed = dp::solve_cluster_with_anchors(&cluster, rs, &Weights::default(), &mut arena);
    assert!(
        decomposed.is_some(),
        "cluster with 300 rows and anchors must decompose"
    );

    let sol = solve_cluster(&cluster, rs);
    assert_eq!(verify_projection(&sol, &cluster), Ok(()));
    let mov_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, DpOp::Mov))
        .count();
    let mod_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, DpOp::Mod { .. }))
        .count();
    assert_eq!(mov_count, 200, "200 identical lines must be paired as Mov");
    assert_eq!(mod_count, 100, "100 modified lines must be paired as Mod");
    assert_eq!(sol.transitions.len(), 300);

    // Cluster with no anchors must return None from solve_cluster_with_anchors
    let all_diff_cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows: cluster.old_rows,
        new_rows: (0..300)
            .map(|i| {
                Row::Add(AddLine::new(
                    LineNum::new(i as u32 + 1).unwrap(),
                    format!("    let totally_different_{i} = 999;"),
                ))
            })
            .collect(),
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    assert!(
        dp::solve_cluster_with_anchors(&all_diff_cluster, rs, &Weights::default(), &mut arena)
            .is_none(),
        "cluster with no anchors must return None"
    );
}

#[test]
fn test_anchor_decomposition_with_word_skeleton() {
    let rs = lang_profile("rs");
    let tag = |mut i: usize| -> String {
        let mut s = String::new();
        for _ in 0..3 {
            s.push((b'a' + (i % 26) as u8) as char);
            i /= 26;
        }
        s
    };
    // 300 lines where 100% of lines have different ANSI escape wrappers around
    // unique substantive code content: Old lines have {48;5;233}, new lines
    // have {48;5;234} and \x1b[38;5;238m
    let old_rows: Vec<Row> = (0..300)
        .map(|i| {
            let t = tag(i);
            Row::Del(DelLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                format!(
                    "{{48;5;233}}{{38;5;238}}  {i}  fn process_item_unique_tag_{t}() {{ {i} }} {{0}}"
                ),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = (0..300)
        .map(|i| {
            let t = tag(i);
            Row::Add(AddLine::new(
                LineNum::new(i as u32 + 1).unwrap(),
                format!(
                    "{{48;5;234}}\x1b[38;5;239m  {i}  fn process_item_unique_tag_{t}() {{ {i} }} {{0}}"
                ),
            ))
        })
        .collect();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows,
        new_rows,
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };

    let mut arena = dp::DpScratchArena::default();
    let decomposed = dp::solve_cluster_with_anchors(&cluster, rs, &Weights::default(), &mut arena);
    assert!(
        decomposed.is_some(),
        "cluster with 300 rows and normalized word skeleton anchors must decompose"
    );

    let sol = decomposed.unwrap();
    assert_eq!(verify_projection(&sol, &cluster), Ok(()));
    let mod_count = sol
        .transitions
        .iter()
        .filter(|t| matches!(t, DpOp::Mod { .. }))
        .count();
    assert_eq!(
        mod_count, 300,
        "all 300 normalized matches must be paired as Mod"
    );
}

#[test]
fn band_edge_touch_detection() {
    // Row bounds with n = 3 columns.
    let bounds = vec![(0, 2), (0, 2), (1, 3)];
    assert!(!cell_touches_band_edge(&bounds, 3, 0, 1));
    assert!(!cell_touches_band_edge(&bounds, 3, 0, 0));
    assert!(!cell_touches_band_edge(&bounds, 3, 2, 3));
    assert!(!cell_touches_band_edge(&bounds, 3, 2, 2));
    assert!(cell_touches_band_edge(&bounds, 3, 0, 2));
    assert!(cell_touches_band_edge(&bounds, 3, 2, 1));
    // Missing row bounds defensively count as touched.
    assert!(cell_touches_band_edge(&bounds, 3, 9, 0));
    assert!(cell_touches_band_edge(&[], 3, 0, 0));
}

#[test]
fn test_narrow_band_large_replacement() {
    // 1100-line diagonal-hugging replacement: exercises the narrow-band
    // attempt end to end (projection invariant + determinism).
    let old_rows: Vec<Row> = (1..=1100)
        .map(|i| {
            Row::Del(DelLine::new(
                LineNum::new(i).unwrap(),
                format!("old line {i} with some content here"),
            ))
        })
        .collect();
    let new_rows: Vec<Row> = (1..=1100)
        .map(|i| {
            Row::Add(AddLine::new(
                LineNum::new(i).unwrap(),
                format!("old line {i} with some content here!"),
            ))
        })
        .collect();
    let c = code_cluster(old_rows, new_rows);
    let rs = lang_profile("rs");
    let first = solve_cluster(&c, rs);
    verify_projection(&first, &c).expect("projection invariant holds");
    assert_eq!(first.rows.len(), 1100);
    let second = solve_cluster(&c, rs);
    assert_eq!(
        format!("{:?}", first.rows),
        format!("{:?}", second.rows),
        "large solves must be deterministic"
    );
    assert_eq!(first.score, second.score);
}

#[test]
fn test_guarded_dels_tail_candidates_parity() {
    let rs = lang_profile("rs");
    // Hunk with del and add lines
    let rows = vec![
        Row::Del(DelLine::new(LineNum::new(1).unwrap(), "    let a = 1;")),
        Row::Del(DelLine::new(LineNum::new(2).unwrap(), "    let b = 2;")),
        Row::Add(AddLine::new(LineNum::new(1).unwrap(), "    let a = 10;")),
        Row::Add(AddLine::new(LineNum::new(2).unwrap(), "    let b = 20;")),
    ];
    let h = Hunk {
        old_start: 1,
        old_count: 2,
        new_start: 1,
        new_count: 2,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let mut fn_cache = rustc_hash::FxHashMap::default();
    let guards = super::pairing::classify_dels(&h, None, None, rs, &mut fn_cache);
    // Since neither del survives in new_content and neither is identical,
    // classification must guard nothing neither hard nor soft
    assert!(!guards.hard.contains(&0));
    assert!(!guards.hard.contains(&1));
    assert!(guards.soft.is_empty());
}

#[test]
fn test_mod_pair_precomputed_invariants_and_prefix_containment() {
    let mut interner = TokenInterner::default();
    let weights = Weights::default();
    let old_text = "// compute_primary_record_header";
    let new_text = "// compute_primary_record_header and then do lots of other completely different things in this long line";
    let ot = vec![dp_tokens(&mut interner, old_text)];
    let nt = vec![
        dp_tokens(&mut interner, "dummy row for offset"),
        dp_tokens(&mut interner, new_text),
    ];

    let old_trimmed = old_text.trim();
    let new_trimmed = new_text.trim();

    let om = DpRowMeta {
        trimmed_id: interner.intern(old_trimmed),
        comment: true,
        indent: 0,
        stripped_len: old_trimmed.len(),
        ..Default::default()
    };
    let nm = DpRowMeta {
        trimmed_id: interner.intern(new_trimmed),
        comment: true,
        indent: 0,
        stripped_len: new_trimmed.len(),
        ..Default::default()
    };
    let dummy_nm = DpRowMeta::default();

    let cluster = ChangeCluster {
        preceding_ctx: Vec::new(),
        old_rows: vec![Row::Del(DelLine::new(
            LineNum::new(1).unwrap(),
            old_text.to_string(),
        ))],
        new_rows: vec![
            Row::Add(AddLine::new(
                LineNum::new(1).unwrap(),
                "dummy row for offset".to_string(),
            )),
            Row::Add(AddLine::new(LineNum::new(2).unwrap(), new_text.to_string())),
        ],
        following_ctx: Vec::new(),
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::LineComment,
    };

    // (0, 1) has offset_diff = 1 > 0 and comment = true, so min_ratio is 0.65.
    // similarity_ratio is ~0.31 (< 0.65), so it ONLY passes via prefix
    // containment!
    assert!(mod_pair_needs_scoring(
        0,
        1,
        &cluster,
        &ot,
        &nt,
        &[om],
        &[dummy_nm, nm],
        &interner,
        None,
        &weights,
    ));

    // If stripped_len is too short (< min_containment_len), it should not be
    // considered prefix containment
    let mut om_short = om;
    om_short.stripped_len = 3;
    assert!(!mod_pair_needs_scoring(
        0,
        1,
        &cluster,
        &ot,
        &nt,
        &[om_short],
        &[dummy_nm, nm],
        &interner,
        None,
        &Weights {
            min_containment_len: 8,
            ..weights
        },
    ));
}

#[test]
fn test_unicode_statement_head_alignment() {
    let old_rows = vec![
        del_row(1, "    функция вычисление_1() {"),
        del_row(2, "    процедура печать() {"),
    ];
    let new_rows = vec![
        add_row(1, "    функция вычисление_2() {"),
        add_row(2, "    процедура вывод() {"),
    ];

    let cluster = code_cluster(old_rows, new_rows);
    let sol = dp::solve_cluster(&cluster, None);
    assert_eq!(signature(&sol), ["mod", "mod"]);
    match (&sol.rows[0], &sol.rows[1]) {
        (
            Row::Mod {
                old_line: d1,
                new_line: a1,
            },
            Row::Mod {
                old_line: d2,
                new_line: a2,
            },
        ) => {
            assert!(d1.text.contains("вычисление_1"));
            assert!(a1.text.contains("вычисление_2"));
            assert!(d2.text.contains("печать"));
            assert!(a2.text.contains("вывод"));
        }
        other => panic!("expected 2 paired mod rows, got: {other:?}"),
    }
}

/// Regression: slot-continuity bonuses must never outvote content evidence.
/// One del, two adds; the first add shares a 28-char run with the del while
/// the second shares (almost) nothing, but the second sits adjacent to the
/// following close. With δ sized to the close-agreement tier instead of a
/// takeover 800, content wins.
#[test]
fn test_slot_continuity_bonus_sized_to_evidence() {
    let rs = lang_profile("rs");
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(3).unwrap(),
        "        \" diffuse \u{2212}5 +20 ~3 \u{b7} crates/diffuse-tui/src/\"",
    ))];
    let new_rows = vec![
        Row::Add(AddLine::new(
            LineNum::new(3).unwrap(),
            "        \" diffuse \u{2212}5 +20 ~3\",",
        )),
        Row::Add(AddLine::new(
            LineNum::new(4).unwrap(),
            "        \"header must omit the directory prefix\"",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["    assert_eq!(", "        pfx_line.to_string(),"],
        old_rows,
        new_rows,
        following_ctx: vec!["    );"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert_eq!(signature(&sol), ["mod", "add"]);
    match &sol.rows[..] {
        [Row::Mod { new_line, .. }, Row::Add(_)] => {
            assert!(
                new_line.text.contains("\u{2212}5 +20 ~3"),
                "mod must pair the del with the content-matching add, got: {}",
                new_line.text
            );
        }
        other => panic!("expected [mod, add], got: {other:?}"),
    }
}

/// The terminal-return/common-indent tail path still needs token evidence:
/// zero word overlap at close_dist 1 must not pair, even with the bonus.
/// Dissimilar lines stay unpaired at the solve level even with identical
/// surroundings: guards this against regressing into a forced pairing.
#[test]
fn test_dissimilar_pair_stays_unpaired() {
    let rs = lang_profile("rs");
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(3).unwrap(),
        "        \" diffuse \u{2212}5 +20 ~3 \u{b7} crates/diffuse-tui/src/\"",
    ))];
    let new_rows = vec![Row::Add(AddLine::new(
        LineNum::new(4).unwrap(),
        "        \"header must omit the directory prefix\"",
    ))];
    let cluster = ChangeCluster {
        preceding_ctx: vec![],
        old_rows,
        new_rows,
        following_ctx: vec!["    );"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert_eq!(signature(&sol), ["del", "add"]);
}

/// Short closers still pair on content alone: the δ sizing must not break
/// the rule's namesake case.
#[test]
fn test_short_closer_pairs_without_tail_bonus() {
    let rs = lang_profile("rs");
    let old_rows = vec![Row::Del(DelLine::new(LineNum::new(10).unwrap(), "    }"))];
    let new_rows = vec![Row::Add(AddLine::new(LineNum::new(20).unwrap(), "    };"))];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["    foo();"],
        old_rows,
        new_rows,
        following_ctx: vec!["    bar();"],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert_eq!(signature(&sol), ["mod"]);
}

/// Open proximity must not elect a keyword-coincidence rival over a content
/// twin: the dissimilar add sits closer to the block open (higher open
/// proximity) with head-word agreement for validity, but the similar add
/// shares a real token run. Locks the baseline behavior.
#[test]
fn test_open_proximity_prefers_content_twin() {
    let rs = lang_profile("rs");
    let old_rows = vec![Row::Del(DelLine::new(
        LineNum::new(3).unwrap(),
        "        if alpha_condition_here {",
    ))];
    let new_rows = vec![
        Row::Add(AddLine::new(LineNum::new(3).unwrap(), "        if x {")),
        Row::Add(AddLine::new(
            LineNum::new(4).unwrap(),
            "        if alpha_condition_there {",
        )),
    ];
    let cluster = ChangeCluster {
        preceding_ctx: vec!["    fn f() {"],
        old_rows,
        new_rows,
        following_ctx: vec![],
        del_survival: rustc_hash::FxHashSet::default(),
        section: SyntaxSectionKind::Code,
    };
    let sol = dp::solve_cluster(&cluster, rs);
    assert_eq!(signature(&sol), ["add", "mod"]);
    match &sol.rows[..] {
        [Row::Add(_), Row::Mod { new_line, .. }] => {
            assert!(
                new_line.text.contains("alpha_condition_there"),
                "mod must pair the content twin, got: {}",
                new_line.text
            );
        }
        other => panic!("expected [add, mod], got: {other:?}"),
    }
}

/// Surplus survival must not veto a high-similarity modification: the del
/// text has a verbatim twin elsewhere in the new file, but the nearby add
/// is its obvious 95%-overlap counterpart. Must pair as Mod.
#[test]
fn test_surviving_duplicate_high_similarity_pairs_as_mod() {
    let rows = vec![
        Row::Del(DelLine::new(
            LineNum::new(10).unwrap(),
            "        let x = 1;",
        )),
        Row::Add(AddLine::new(
            LineNum::new(10).unwrap(),
            "        let x = 2;",
        )),
    ];
    let h = Hunk {
        old_start: 10,
        old_count: 1,
        new_start: 10,
        new_count: 1,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let old = ["fn f() {", "        let x = 1;", "}"];
    let new = [
        "fn f() {",
        "        let x = 2;",
        "}",
        "fn g() {",
        "        let x = 1;",
        "}",
    ];
    let res = super::pairing::dp_pairing(&h, Some(&old[..]), Some(&new[..]), None);
    assert_eq!(
        res.pair_of.get(&1),
        Some(&0),
        "surviving twin elsewhere must not veto the modification pair"
    );
}

/// Surplus survival with no content behind the rival: stays unpaired even
/// once the veto becomes a penalty.
#[test]
fn test_surviving_duplicate_dissimilar_stays_unpaired() {
    let rows = vec![
        Row::Del(DelLine::new(
            LineNum::new(10).unwrap(),
            "        \" diffuse \u{2212}5 +20 ~3 \u{b7} crates/diffuse-tui/src/\"",
        )),
        Row::Add(AddLine::new(
            LineNum::new(10).unwrap(),
            "        \"header must omit the directory prefix\"",
        )),
    ];
    let h = Hunk {
        old_start: 10,
        old_count: 1,
        new_start: 10,
        new_count: 1,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let old = [
        "fn f() {",
        "        \" diffuse \u{2212}5 +20 ~3 \u{b7} crates/diffuse-tui/src/\"",
        "    );",
    ];
    let new = [
        "fn f() {",
        "        \"header must omit the directory prefix\"",
        "    );",
        "fn g() {",
        "        \" diffuse \u{2212}5 +20 ~3 \u{b7} crates/diffuse-tui/src/\"",
        "}",
    ];
    let res = super::pairing::dp_pairing(&h, Some(&old[..]), Some(&new[..]), None);
    assert!(
        res.pair_of.is_empty(),
        "dissimilar rival must stay unpaired, got: {:?}",
        res.pair_of
    );
}

/// Same-function twin: fn-scoped survival still pairs high-similarity mods.
#[test]
fn test_same_function_twin_pairs_as_mod() {
    let rs = lang_profile("rs");
    let rows = vec![
        Row::Del(DelLine::new(LineNum::new(2).unwrap(), "        let x = 1;")),
        Row::Add(AddLine::new(LineNum::new(2).unwrap(), "        let x = 2;")),
    ];
    let h = Hunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 4,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let old = ["fn f() {", "        let x = 1;", "}"];
    let new = ["fn f() {", "        let x = 2;", "        let x = 1;", "}"];
    let res = super::pairing::dp_pairing(&h, Some(&old[..]), Some(&new[..]), rs);
    assert_eq!(
        res.pair_of.get(&1),
        Some(&0),
        "same-function twin must not veto the modification pair"
    );
}

/// Twin in a different function: scope-narrowed survival never penalizes.
#[test]
fn test_different_function_twin_pairs_as_mod() {
    let rs = lang_profile("rs");
    let rows = vec![
        Row::Del(DelLine::new(LineNum::new(2).unwrap(), "        let x = 1;")),
        Row::Add(AddLine::new(LineNum::new(2).unwrap(), "        let x = 2;")),
    ];
    let h = Hunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 3,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let old = ["fn f() {", "        let x = 1;", "}"];
    let new = [
        "fn f() {",
        "        let x = 2;",
        "}",
        "fn g() {",
        "        let x = 1;",
        "}",
    ];
    let res = super::pairing::dp_pairing(&h, Some(&old[..]), Some(&new[..]), rs);
    assert_eq!(
        res.pair_of.get(&1),
        Some(&0),
        "different-function twin is out of scope and must not affect pairing"
    );
}

/// Deleted function: the del's fn resolves on the old side but has no new
/// body, so survival falls back to whole-file counts rather than skipping.
#[test]
fn test_deleted_function_falls_back_to_file_counts() {
    let rs = lang_profile("rs");
    let rows = vec![
        Row::Del(DelLine::new(LineNum::new(2).unwrap(), "        let x = 1;")),
        Row::Add(AddLine::new(LineNum::new(2).unwrap(), "        let x = 2;")),
    ];
    let h = Hunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 3,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    };
    let old = ["fn f() {", "        let x = 1;", "}"];
    let new = ["fn g() {", "        let x = 2;", "}"];
    let res = super::pairing::dp_pairing(&h, Some(&old[..]), Some(&new[..]), rs);
    assert_eq!(
        res.pair_of.get(&1),
        Some(&0),
        "unresolvable new scope must fall back to file counts, not skip"
    );
}
