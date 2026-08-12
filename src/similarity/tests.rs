//! Ports of the similarity-scoring behavior from test/review.test.ts (the
//! pairing metric). The TS suite exercises these through `collapseHunkRows`;
//! these are direct sanity checks of the scoring functions themselves.

use crate::similarity::{is_comment_line, line_similarity, line_similarity_parts};

// Identical content → sim 1; the add is fully matched.
#[test]
fn identical_lines_score_one() {
    assert!((line_similarity("same line", "same line") - 1.0).abs() < 1e-9);
    let parts = line_similarity_parts("same line", "same line");
    assert_eq!(parts.add_matched, parts.add_total);
}

// Disjoint content → sim 0 (whitespace tokens excluded — an indent-only
// reflow must not inflate the match).
#[test]
fn disjoint_lines_score_zero() {
    assert!(line_similarity("abc def", "xyz qrs") < 0.05);
    // Whitespace-only tokens are excluded: `None` vs `        }` share only
    // their leading spaces — nothing.
    assert!((line_similarity("None", "        }") - 0.0).abs() < 1e-9);
}

// The add-fraction distinguishes a fragment pair (add inside del — an old
// line split into a block) from a growth pair (del inside add — `None` →
// `return None;`), which score identically on `sim` alone.
#[test]
fn fragment_vs_growth_parts() {
    let split = line_similarity_parts(
        "use std::{cell::RefCell, num::NonZeroUsize, path::PathBuf};",
        "    cell::RefCell,",
    );
    let growth = line_similarity_parts("None", "return None;");
    assert_eq!(
        split.add_matched, split.add_total,
        "the fragment's add content sits entirely inside the del"
    );
    assert!(
        growth.add_matched < growth.add_total,
        "the growth's add contains more than the del"
    );
}

// Comment markers: the `///`/`//`/`/*`/`*` markers are language-agnostic;
// `#` depends on the profile (hash-comment languages only — never Rust
// attrs).
#[test]
fn comment_line_markers() {
    assert!(is_comment_line("// a comment", false));
    assert!(is_comment_line("/// doc", false));
    assert!(is_comment_line("/* block", false));
    assert!(is_comment_line("* continuation", false));
    assert!(is_comment_line("# derive", true), "py hash comment");
    assert!(
        !is_comment_line("#[derive]", false),
        "rust # is not a comment"
    );
    assert!(!is_comment_line("let x = 1;", false));
}
