use std::{collections::HashMap, path::PathBuf};

use crate::{
    parse::{AddLine, CtxLine, DelLine, Hunk, Row},
    pipeline::{PipelineOptions, run_pipeline_full},
    resurface::resurface_hunks,
    wire::OutputRow,
};

// The pristine `scrollbar-grab-drag` dispatch.rs case: the real
// `git diff -U4 -w` tape over the corpus snapshots (paths virtualized to
// `dispatch.rs`). The reindented blocks are suppressed from this tape — the
// mangle region (old 299–350 ↔ new 295–346) is entirely untaped.
const DISPATCH_W_PATCH: &str = include_str!("fixtures/dispatch_w.patch");

fn ctx(o: u32, n: u32, t: &str) -> Row {
    Row::Ctx(CtxLine::new(o, n, t))
}

fn del(n: u32, t: &str) -> Row {
    Row::Del(DelLine::new(n, t))
}

fn add(n: u32, t: &str) -> Row {
    Row::Add(AddLine::new(n, t))
}

fn hunk(old_start: u32, old_count: u32, new_start: u32, new_count: u32, rows: Vec<Row>) -> Hunk {
    Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

/// Row-kind signature of a hunk, for compact assertion failures.
fn signature(h: &Hunk) -> Vec<&'static str> {
    h.rows
        .iter()
        .map(|r| match r {
            Row::Ctx(_) => "ctx",
            Row::Del(_) => "del",
            Row::Add(_) => "add",
            Row::Mod { .. } => "mod",
            Row::MovSrc { .. } => "mov_src",
            Row::MovTgt { .. } => "mov_tgt",
            other => panic!("unexpected row: {other:?}"),
        })
        .collect()
}

#[test]
fn default_tape_is_strict_noop() {
    // A default-generated tape: byte-equal context, truly-equal gaps. The
    // pass must not touch a single row.
    let old = ["alpha", "beta", "gamma"];
    let new = ["alpha", "BETA", "gamma"];
    let mut hunks = vec![hunk(
        1,
        3,
        1,
        3,
        vec![
            ctx(1, 1, "alpha"),
            del(2, "beta"),
            add(2, "BETA"),
            ctx(3, 3, "gamma"),
        ],
    )];
    let before = hunks.clone();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks, before);
}

#[test]
fn ws_suppressed_ctx_row_splits_to_del_add() {
    // The git -w convention: the suppressed pair prints as one context row
    // carrying the NEW side's text. It must surface as Del(old)+Add(new).
    let old = ["fn main() {", "    let a = 1;", "}"];
    let new = ["fn main() {", "\tlet a = 1;", "}"];
    let mut hunks = vec![hunk(
        1,
        3,
        1,
        3,
        vec![
            ctx(1, 1, "fn main() {"),
            ctx(2, 2, "\tlet a = 1;"),
            ctx(3, 3, "}"),
        ],
    )];
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].rows,
        vec![
            ctx(1, 1, "fn main() {"),
            del(2, "    let a = 1;"),
            add(2, "\tlet a = 1;"),
            ctx(3, 3, "}"),
        ]
    );
    // Coverage (and therefore the header counts) is unchanged by a split.
    assert_eq!(hunks[0].old_count, 3);
    assert_eq!(hunks[0].new_count, 3);
}

#[test]
fn ws_suppressed_ctx_row_accepts_old_side_text() {
    // Either side's text on the context row proves the pair; both surface.
    let old = ["fn main() {", "    let a = 1;", "}"];
    let new = ["fn main() {", "\tlet a = 1;", "}"];
    let mut hunks = vec![hunk(
        1,
        3,
        1,
        3,
        vec![
            ctx(1, 1, "fn main() {"),
            ctx(2, 2, "    let a = 1;"),
            ctx(3, 3, "}"),
        ],
    )];
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(
        hunks[0].rows,
        vec![
            ctx(1, 1, "fn main() {"),
            del(2, "    let a = 1;"),
            add(2, "\tlet a = 1;"),
            ctx(3, 3, "}"),
        ]
    );
}

#[test]
fn interior_whitespace_ctx_row_splits_to_del_add() {
    // Interior whitespace is inside the -w suppression class (wider than
    // the Mov predicate's trim equality): the pair must surface even
    // though the DP will pair it as a Mod rather than a Mov.
    let old = ["fn main() {", "    foo (x);", "}"];
    let new = ["fn main() {", "    foo(x);", "}"];
    let mut hunks = vec![hunk(
        1,
        3,
        1,
        3,
        vec![
            ctx(1, 1, "fn main() {"),
            ctx(2, 2, "    foo(x);"),
            ctx(3, 3, "}"),
        ],
    )];
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(
        hunks[0].rows,
        vec![
            ctx(1, 1, "fn main() {"),
            del(2, "    foo (x);"),
            add(2, "    foo(x);"),
            ctx(3, 3, "}"),
        ]
    );
}

#[test]
fn ctx_row_differing_beyond_whitespace_is_left_alone() {
    // A context row whose snapshot texts differ beyond whitespace
    // contradicts the -w contract; the tape is inconsistent and must be
    // preserved untouched.
    let old = ["let a = 1;"];
    let new = ["let a = 2;"];
    let mut hunks = vec![hunk(1, 1, 1, 1, vec![ctx(1, 1, "let a = 1;")])];
    let before = hunks.clone();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks, before);
}

#[test]
fn ctx_row_matching_neither_side_is_left_alone() {
    // The pair differs only in whitespace but the taped text matches
    // neither snapshot: unprovable, untouched.
    let old = ["    x = 1;"];
    let new = ["        x = 1;"];
    let mut hunks = vec![hunk(1, 1, 1, 1, vec![ctx(1, 1, "GARBAGE")])];
    let before = hunks.clone();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks, before);
}

#[test]
fn reindent_only_file_synthesizes_full_tape_hunk() {
    // A file whose only change is a reindent is entirely untaped by
    // `git diff -w`. The pass synthesizes the change with ±ENGINE_CONTEXT
    // (=4) padding, exactly like a -U4 emission.
    let old = [
        "fn a() {", "    x();", "}", "", "fn b() {", "    y();", "    z();", "}", "", "// end",
    ];
    let new = [
        "fn a() {",
        "    x();",
        "}",
        "",
        "fn b() {",
        "        y();",
        "        z();",
        "}",
        "",
        "// end",
    ];
    let mut hunks = Vec::new();
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(
        signature(h),
        [
            "ctx", "ctx", "ctx", "ctx", "del", "del", "add", "add", "ctx", "ctx", "ctx"
        ]
    );
    assert_eq!(h.old_start, 2);
    assert_eq!(h.new_start, 2);
    assert_eq!(h.old_count, 9);
    assert_eq!(h.new_count, 9);
    assert_eq!(
        h.rows,
        vec![
            ctx(2, 2, "    x();"),
            ctx(3, 3, "}"),
            ctx(4, 4, ""),
            ctx(5, 5, "fn b() {"),
            del(6, "    y();"),
            del(7, "    z();"),
            add(6, "        y();"),
            add(7, "        z();"),
            ctx(8, 8, "}"),
            ctx(9, 9, ""),
            ctx(10, 10, "// end"),
        ]
    );
}

#[test]
fn equal_content_synthesizes_nothing() {
    let old = ["a", "b"];
    let new = ["a", "b"];
    let mut hunks = Vec::new();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert!(hunks.is_empty());
}

#[test]
fn gap_differing_beyond_whitespace_is_skipped_wholesale() {
    // Untaped region whose positional pairing differs beyond whitespace:
    // the tape cannot have come from whitespace-insensitive matching;
    // nothing is synthesized.
    let old = ["a", "b", "c"];
    let new = ["a", "X b Y", "c"];
    let mut hunks = Vec::new();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert!(hunks.is_empty());
}

#[test]
fn interior_whitespace_gap_synthesizes_hunk() {
    // Interior-whitespace changes are in the -w suppression class: the gap
    // synthesis must surface them (the DP pairs them as Mods).
    let old = ["a", "foo (x);", "c"];
    let new = ["a", "foo(x);", "c"];
    let mut hunks = Vec::new();
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].rows,
        vec![
            ctx(1, 1, "a"),
            del(2, "foo (x);"),
            add(2, "foo(x);"),
            ctx(3, 3, "c"),
        ]
    );
}

#[test]
fn close_runs_merge_distant_runs_split() {
    let mk = |second: usize| -> (Vec<&'static str>, Vec<&'static str>) {
        let old: Vec<&'static str> = (1..=20)
            .map(|i| match i {
                4 => "    a();",
                i if i == second => "    b();",
                i => match i {
                    1..=9 => "l1",
                    _ => "l2",
                },
            })
            .collect();
        let new: Vec<&'static str> = (1..=20)
            .map(|i| match i {
                4 => "        a();",
                i if i == second => "        b();",
                i => match i {
                    1..=9 => "l1",
                    _ => "l2",
                },
            })
            .collect();
        (old, new)
    };

    // 8 equal lines between the runs (5..=12 for the second run at 13): the
    // windows of a -U4 emission touch, so the runs merge into one hunk with
    // the interior context shown.
    let (old, new) = mk(13);
    let mut hunks = Vec::new();
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 1, "close runs must merge: {hunks:#?}");
    assert_eq!(
        signature(&hunks[0]),
        [
            "ctx", "ctx", "ctx", "del", "add", "ctx", "ctx", "ctx", "ctx", "ctx", "ctx", "ctx",
            "ctx", "del", "add", "ctx", "ctx", "ctx", "ctx",
        ]
    );

    // 9 equal lines between the runs (second run at 14): two separate
    // -U4-style hunks.
    let (old, new) = mk(14);
    let mut hunks = Vec::new();
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 2, "distant runs must split: {hunks:#?}");
    assert_eq!(hunks[0].old_start, 1);
    assert_eq!(hunks[1].old_start, 10);
}

#[test]
fn gap_padding_clamps_to_hunk_coverage() {
    // The parsed hunk covers old 1..=4; the suppressed change at old 8 sits
    // in the gap [5, 12]. Its leading padding clamps at the gap start (3
    // rows, not 4), staying disjoint from the parsed hunk's coverage.
    let old: Vec<&str> = (1..=12)
        .map(|i| if i == 8 { "    q();" } else { "m" })
        .collect();
    let new: Vec<&str> = (1..=12)
        .map(|i| if i == 8 { "        q();" } else { "m" })
        .collect();
    let mut hunks = vec![hunk(
        1,
        4,
        1,
        4,
        vec![
            ctx(1, 1, "m"),
            del(2, "m"),
            add(2, "M"),
            ctx(3, 3, "m"),
            ctx(4, 4, "m"),
        ],
    )];
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].old_start, 1, "parsed hunk stays first");
    let h = &hunks[1];
    assert_eq!(h.old_start, 5);
    assert_eq!(h.old_count, 8);
    assert_eq!(h.new_start, 5);
    assert_eq!(h.new_count, 8);
    assert_eq!(
        h.rows,
        vec![
            ctx(5, 5, "m"),
            ctx(6, 6, "m"),
            ctx(7, 7, "m"),
            del(8, "    q();"),
            add(8, "        q();"),
            ctx(9, 9, "m"),
            ctx(10, 10, "m"),
            ctx(11, 11, "m"),
            ctx(12, 12, "m"),
        ]
    );
}

#[test]
fn synthesized_hunks_keep_tape_order() {
    // A suppressed change before the first parsed hunk: the synthesized
    // hunk must be inserted before it, keeping the tape positionally ordered.
    let old: Vec<&str> = (1..=10)
        .map(|i| if i == 2 { "    p();" } else { "m" })
        .collect();
    let new: Vec<&str> = (1..=10)
        .map(|i| if i == 2 { "        p();" } else { "m" })
        .collect();
    let mut hunks = vec![hunk(
        8,
        3,
        8,
        3,
        vec![ctx(8, 8, "m"), del(9, "m"), add(9, "M"), ctx(10, 10, "m")],
    )];
    assert!(resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].old_start, 1, "synthesized hunk leads the tape");
    assert_eq!(hunks[0].new_start, 1);
    assert_eq!(hunks[1].old_start, 8, "parsed hunk stays second");
}

#[test]
fn unequal_gap_pairing_is_skipped() {
    // The middle gap pairs 5 old lines against 9 new lines: no positional
    // pairing exists, so nothing may be synthesized there. The trailing gap
    // (4 = 4) is walked and found byte-equal.
    let old: Vec<&str> = (1..=16).map(|_| "o").collect();
    let new: Vec<&str> = (1..=20)
        .map(|i| if (5..=13).contains(&i) { "j" } else { "o" })
        .collect();
    let mut hunks = vec![
        hunk(
            1,
            4,
            1,
            4,
            vec![
                ctx(1, 1, "o"),
                del(2, "o"),
                add(2, "N"),
                ctx(3, 3, "o"),
                ctx(4, 4, "o"),
            ],
        ),
        hunk(
            10,
            3,
            14,
            3,
            vec![
                del(10, "o"),
                del(11, "o"),
                del(12, "o"),
                add(14, "o"),
                add(15, "o"),
                add(16, "o"),
            ],
        ),
    ];
    let before = hunks.clone();
    assert!(!resurface_hunks(&mut hunks, &old, &new));
    assert_eq!(hunks, before);
}

#[test]
fn pipeline_surfaces_reindent_with_true_pairing() {
    // End to end: a zero-hunk -w tape plus snapshots of a reindent-only
    // change. The pipeline must surface the suppressed change; the cluster
    // pairing then links it with its true source (as a Mov via mark_moves,
    // or as a Mod carrying only the indent spans — presentation is engine
    // policy, the pairing is the contract).
    let diff = "diff --git a/f.rs b/f.rs\nindex 1111111..2222222 100644\n--- a/f.rs\n+++ b/f.rs\n";
    let mut old_map = HashMap::new();
    old_map.insert("f.rs".to_string(), "fn a() {\n    x();\n}\n".to_string());
    let mut new_map = HashMap::new();
    new_map.insert("f.rs".to_string(), "fn a() {\n\tx();\n}\n".to_string());
    let out = run_pipeline_full(
        diff,
        Some(new_map),
        Some(old_map),
        PipelineOptions::default(),
    );
    let rows: Vec<&OutputRow> = out
        .files
        .iter()
        .flat_map(|f| f.hunks.iter())
        .flat_map(|h| h.rows.iter())
        .collect();
    assert!(
        pairs_with_output(&rows, 2, 2),
        "reindented `x();` must pair with its source, got {rows:#?}"
    );
}

#[test]
fn dispatch_w_tape_pairs_reindented_blocks_with_true_sources() {
    // The pristine regression (see the fixture header): under -w matching the
    // reindented bottom-gap block (new 335..=347) and the reindented
    // file-header block (new 310..=333) are untaped. The pass must surface
    // both, and the DP must pair each block with its true source —
    // old 339→new 335 and the rival twin old 317→new 313 — with no trace of
    // the original mangle (old 317→new 335).
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = if manifest.join("tests/golden").exists() {
        manifest.join("tests/golden")
    } else {
        manifest.join("../../tests/golden")
    };
    let corpus = dir.join("scrollbar-grab-drag/files");
    let old = std::fs::read_to_string(corpus.join("crates-diffuse-tui-src-app-dispatch.old.rs"))
        .expect("corpus snapshot");
    let new = std::fs::read_to_string(corpus.join("crates-diffuse-tui-src-app-dispatch.new.rs"))
        .expect("corpus snapshot");

    let mut old_map = HashMap::new();
    old_map.insert("dispatch.rs".to_string(), old);
    let mut new_map = HashMap::new();
    new_map.insert("dispatch.rs".to_string(), new);
    let out = run_pipeline_full(
        DISPATCH_W_PATCH,
        Some(new_map),
        Some(old_map),
        PipelineOptions::default(),
    );

    let rows: Vec<&OutputRow> = out
        .files
        .iter()
        .filter(|f| f.path == "dispatch.rs")
        .flat_map(|f| f.hunks.iter())
        .flat_map(|h| h.rows.iter())
        .collect();
    assert!(!rows.is_empty(), "dispatch.rs must render");
    assert!(
        pairs_with_output(&rows, 339, 335),
        "the bottom-gap twin (old 339) must pair with new 335"
    );
    assert!(
        pairs_with_output(&rows, 340, 336),
        "the chain must continue: old 340 (`&& !self.raw_diff`) with new 336"
    );
    assert!(
        pairs_with_output(&rows, 317, 313),
        "the rival file-header twin (old 317) must pair within its own block (new 313)"
    );
    assert!(
        !pairs_with_output(&rows, 317, 335),
        "the mangle signature (old 317 paired with new 335) must be gone"
    );
}

/// Whether the row stream links old line `old` with new line `new` — as a
/// Mod, or as an adjacent MovSrc/MovTgt pair (representation is engine
/// policy; the pairing is the contract).
fn pairs_with_output(rows: &[&OutputRow], old: u32, new: u32) -> bool {
    crate::wire::test_rows_pair_with(rows, old, new)
}
