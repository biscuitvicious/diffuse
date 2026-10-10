//! Ports of the similarity-scoring behavior from test/review.test.ts (the
//! pairing metric). The TS suite exercises these through `collapseHunkRows`;
//! these are direct sanity checks of the scoring functions themselves.

use crate::similarity::{line_similarity, line_similarity_parts};
use diffuse_syntax::registry::lang_profile;

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

#[test]
fn comment_line_markers() {
    let rs = lang_profile("rs").unwrap();
    let py = lang_profile("py").unwrap();

    assert!(rs.is_comment_line("// a comment"));
    assert!(rs.is_comment_line("/// doc"));
    assert!(rs.is_comment_line("/* block"));
    assert!(rs.is_comment_line("* continuation"));
    assert!(py.is_comment_line("# derive"), "py hash comment");
    assert!(!rs.is_comment_line("#[derive]"), "rust # is not a comment");
    assert!(!rs.is_comment_line("let x = 1;"));
}

#[test]
fn test_syntax_weighted_similarity_prioritizes_identifiers() {
    let base = "pub fn resolve_path(vfs: &VFS, path: &Path) -> Result<PathBuf>;";
    let renamed = "pub fn resolve_path(vfs: &VFS, req: &Request) -> io::Result<ResolvedPath>;";
    let unrelated = "pub fn check_perms(vfs: &VFS, path: &Path) -> bool;";

    let sim_renamed = line_similarity(base, renamed);
    let sim_unrelated = line_similarity(base, unrelated);

    assert!(
        sim_renamed >= sim_unrelated,
        "rename of same function ({:.3}) should score higher than unrelated function ({:.3})",
        sim_renamed,
        sim_unrelated
    );
}
