//! Unit tests for exact move detection and block move rules.

use super::*;
use crate::parse::{AddLine, CtxLine, DelLine, FileStatus, Hunk, Row};

fn del(num: u32, text: &str) -> Row {
    Row::Del(DelLine::new(num, text))
}

fn add(num: u32, text: &str) -> Row {
    Row::Add(AddLine::new(num, text))
}

fn ctx(old_num: u32, new_num: u32, text: &str) -> Row {
    Row::Ctx(CtxLine::new(old_num, new_num, text))
}

fn file(hunks: Vec<Hunk>) -> DiffFile {
    DiffFile {
        path: "m.rs".into(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: FileStatus::Modified,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
        raw_hunks: Vec::new(),
        no_content: false,
        is_binary: false,
        has_file_headers: false,
        old_bytes: None,
        new_bytes: None,
    }
}

fn hunk(rows: Vec<Row>) -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 3,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

// Port of the moved-line test block from test/review.test.ts.
#[test]
fn exact_content_pair_marked_moved() {
    let mut f = file(vec![hunk(vec![
        del(1, "old one"),
        add(1, "new one"),
        add(2, "extra line"),
        add(3, ""),
        del(4, "moved line"),
        add(4, "moved line"),
        add(5, "after the blank"),
    ])]);
    mark_moves(&mut f);
    let moved_add = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(4) && r.text() == "moved line")
        .expect("add");
    assert!(
        moved_add.is_moved(),
        "exact-content pair marked moved (add)"
    );
    assert_eq!(moved_add.new_line(), Some(4), "target at new_line 4");

    let moved_del = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(4) && r.text() == "moved line")
        .expect("del");
    assert!(
        moved_del.is_moved(),
        "exact-content pair marked moved (del source)"
    );
}

// Port of the block-move rule test: the old find_mount chain
// verbatim-matching the NEW get method must NOT be marked moved.
#[test]
fn block_move_rule_interior_matches_are_not_moves() {
    let mut f = file(vec![
        hunk(vec![
            del(1019, "        self.mount_stack"),
            del(1020, "            .filesystems"),
            del(1021, "            .iter()"),
            del(
                1022,
                "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            ),
            del(
                1023,
                "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
            ),
            add(1060, "        self.mount_stack.get(guest_path)"),
            add(1061, "    }"),
        ]),
        hunk(vec![
            add(
                771,
                "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {",
            ),
            add(772, "        self.filesystems"),
            add(773, "            .iter()"),
            add(
                774,
                "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            ),
            add(775, "            .map(|fs| fs.as_ref() as &dyn Filesystem)"),
            add(776, "    }"),
        ]),
    ]);
    mark_moves(&mut f);
    let del_iter = f.hunks[0]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Del(d) if d.text.trim() == ".iter()"));
    assert!(
        del_iter.is_some() && !del_iter.unwrap().is_moved(),
        "the interior chain del is NOT a move (the run's first del didn't move)"
    );
    let add_iter = f.hunks[1]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Add(a) if a.text.trim() == ".iter()"));
    assert!(
        add_iter.is_some() && !add_iter.unwrap().is_moved(),
        "the get method's chain add is NOT marked moved"
    );
}

// When the whole block moves (including the leading line), common method call
// lines like .rows, .iter(), .collect() must all be marked moved.
#[test]
fn moved_method_chain_block_marks_all_lines_moved() {
    let mut f = file(vec![
        hunk(vec![
            del(128, "    let del_ids: Vec<usize> = h"),
            del(129, "        .rows"),
            del(130, "        .iter()"),
            del(131, "        .collect();"),
        ]),
        hunk(vec![
            add(97, "    let del_ids: Vec<usize> = h"),
            add(98, "        .rows"),
            add(99, "        .iter()"),
            add(103, "        .collect();"),
            // Another addition in the diff that happens to contain substring `.iter()`
            add(200, "    tokens.iter().map(|t| t.id).collect()"),
        ]),
    ]);
    mark_moves(&mut f);

    for (old_n, txt) in [
        (128, "    let del_ids: Vec<usize> = h"),
        (129, "        .rows"),
        (130, "        .iter()"),
        (131, "        .collect();"),
    ] {
        let r = f.hunks[0].rows.iter().find(|r| r.old_line() == Some(old_n));
        assert!(
            r.is_some() && r.unwrap().is_moved(),
            "source line {old_n} ({txt}) must be marked moved"
        );
    }

    for (new_n, txt) in [
        (97, "    let del_ids: Vec<usize> = h"),
        (98, "        .rows"),
        (99, "        .iter()"),
        (103, "        .collect();"),
    ] {
        let r = f.hunks[1].rows.iter().find(|r| r.new_line() == Some(new_n));
        assert!(
            r.is_some() && r.unwrap().is_moved(),
            "target line {new_n} ({txt}) must be marked moved"
        );
    }
}

// When a trailing blank line exists at both source and target sites of a moved
// block, it moves with the block.
#[test]
fn trailing_blank_line_at_target_site_moves_with_block() {
    let mut f = file(vec![
        hunk(vec![
            del(10, "let a = 1;"),
            del(11, "let b = 2;"),
            del(12, ""), // trailing blank at source
        ]),
        hunk(vec![
            add(50, "let a = 1;"),
            add(51, "let b = 2;"),
            add(52, ""), // matching trailing blank at target
            add(70, ""), // another unrelated blank line
        ]),
    ]);
    mark_moves(&mut f);

    // Both line 12 and line 52 must be marked moved
    let d12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(12))
        .unwrap();
    assert!(
        d12.is_moved(),
        "source trailing blank line 12 must be marked moved"
    );

    let a52 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(52))
        .unwrap();
    assert!(
        a52.is_moved(),
        "target trailing blank line 52 must be marked moved"
    );

    let a70 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(70))
        .unwrap();
    assert!(
        !a70.is_moved(),
        "unrelated blank line 70 must NOT be marked moved"
    );
}

// When a trailing blank line exists only at the source site (target site has
// another code line or ends without blank), the blank line does not move and
// does not steal an unrelated blank line elsewhere.
#[test]
fn trailing_blank_line_missing_at_target_site_does_not_move() {
    let mut f = file(vec![
        hunk(vec![
            del(10, "let a = 1;"),
            del(11, "let b = 2;"),
            del(12, ""), // trailing blank at source
        ]),
        hunk(vec![
            add(50, "let a = 1;"),
            add(51, "let b = 2;"),
            add(52, "let c = 3;"), // not a blank line
            add(70, ""),           // unrelated blank line far away
        ]),
    ]);
    mark_moves(&mut f);

    let d12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(12))
        .unwrap();
    assert!(
        !d12.is_moved(),
        "source trailing blank line 12 must NOT be marked moved"
    );

    let a70 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(70))
        .unwrap();
    assert!(
        !a70.is_moved(),
        "unrelated blank line 70 must NOT be marked moved"
    );
}

#[test]
fn whitespace_only_lines_are_not_moves() {
    let mut f = file(vec![
        hunk(vec![del(1, ""), del(2, "    ")]),
        hunk(vec![add(10, ""), add(11, "    ")]),
    ]);
    mark_moves(&mut f);

    for h in &f.hunks {
        for r in &h.rows {
            if r.text().trim().is_empty() {
                assert!(
                    !r.is_moved(),
                    "whitespace-only line {:?} must NOT be marked moved",
                    r.text()
                );
            }
        }
    }
}

#[test]
fn test_boilerplate_attribute_alone_in_deleted_function_is_not_a_move() {
    let mut f = file(vec![
        hunk(vec![
            del(446, "#[test]"),
            del(447, "fn dp_oracle_mirage() {"),
            del(
                448,
                "    assert!(run_dp_oracle_on_corpus(\"mirage-2025-08\") > 0);",
            ),
            del(449, "}"),
        ]),
        hunk(vec![
            add(684, "#[test]"),
            add(
                685,
                "fn test_doc_comment_partial_truncation_pairs_as_mod() {",
            ),
            add(686, "    let rs = lang_profile(\"rs\");"),
            add(687, "}"),
        ]),
    ]);
    mark_moves(&mut f);

    for h in &f.hunks {
        for r in &h.rows {
            assert!(
                !r.is_moved(),
                "boilerplate line {:?} in un-moved block must NOT be marked moved",
                r.text()
            );
        }
    }
}

#[test]
fn test_cohesive_function_move_with_boilerplate_attribute() {
    let mut f = file(vec![
        hunk(vec![
            del(100, "#[test]"),
            del(101, "fn moved_helper() {"),
            del(102, "    assert_eq!(compute_hash(), 42);"),
            del(103, "}"),
        ]),
        hunk(vec![
            add(500, "#[test]"),
            add(501, "fn moved_helper() {"),
            add(502, "    assert_eq!(compute_hash(), 42);"),
            add(503, "}"),
        ]),
    ]);
    mark_moves(&mut f);

    let moved_adds: Vec<_> = f.hunks[1].rows.iter().filter(|r| r.is_moved()).collect();
    assert_eq!(
        moved_adds.len(),
        4,
        "all 4 lines of cohesive relocated function must be marked moved"
    );
    let moved_dels: Vec<_> = f.hunks[0].rows.iter().filter(|r| r.is_moved()).collect();
    assert_eq!(
        moved_dels.len(),
        4,
        "all 4 del source lines must be marked moved"
    );
}

#[test]
fn test_standalone_unique_statement_move() {
    let mut f = file(vec![
        hunk(vec![del(10, "register_custom_hook(&mut registry);")]),
        hunk(vec![add(90, "register_custom_hook(&mut registry);")]),
    ]);
    mark_moves(&mut f);

    assert!(f.hunks[0].rows[0].is_moved());
    assert!(f.hunks[1].rows[0].is_moved());
}

#[test]
fn test_shell_closing_delimiter_move_is_trivial() {
    let sh = diffuse_syntax::registry::lang_profile("sh");
    let mut f = file(vec![hunk(vec![del(10, "fi")]), hunk(vec![add(90, "fi")])]);
    mark_moves_with_profile(&mut f, sh);

    // Standalone trivial delimiter (fi) must NOT be marked as a moved line
    assert!(!f.hunks[0].rows[0].is_moved());
    assert!(!f.hunks[1].rows[0].is_moved());
}

#[test]
fn test_move_with_changed_indentation_populates_runs_on_both_source_and_target() {
    let mut f = file(vec![
        hunk(vec![del(10, "        calculate_total(items);")]),
        hunk(vec![add(90, "    calculate_total(items);")]),
    ]);
    mark_moves(&mut f);

    let src = &f.hunks[0].rows[0];
    let tgt = &f.hunks[1].rows[0];

    assert!(src.is_moved());
    assert!(tgt.is_moved());

    assert!(
        !src.cached_runs().is_empty(),
        "MovSrc must carry cached_runs"
    );
    assert!(
        !tgt.cached_runs().is_empty(),
        "MovTgt must carry cached_runs"
    );
    assert_eq!(
        src.cached_runs(),
        tgt.cached_runs(),
        "MovSrc and MovTgt must carry identical cached_runs"
    );
}

#[test]
fn test_cross_cluster_moves_rejects_single_line_move() {
    let mut f = file(vec![
        hunk(vec![del(10, "single_line_statement();")]),
        hunk(vec![add(90, "single_line_statement();")]),
    ]);
    // Pre-pass requires contiguous group of >= 2 lines, so single-line move is
    // ignored
    mark_cross_cluster_moves_with_profile(&mut f, None);
    assert!(!f.hunks[0].rows[0].is_moved());
    assert!(!f.hunks[1].rows[0].is_moved());

    // Normal post-pass mark_moves DOES match it
    mark_moves(&mut f);
    assert!(f.hunks[0].rows[0].is_moved());
    assert!(f.hunks[1].rows[0].is_moved());
}

#[test]
fn test_cross_cluster_moves_accepts_contiguous_group() {
    let mut f = file(vec![
        hunk(vec![
            del(10, "first_statement();"),
            del(11, "second_statement();"),
        ]),
        hunk(vec![
            add(90, "first_statement();"),
            add(91, "second_statement();"),
        ]),
    ]);
    mark_cross_cluster_moves_with_profile(&mut f, None);
    assert!(f.hunks[0].rows[0].is_moved());
    assert!(f.hunks[0].rows[1].is_moved());
    assert!(f.hunks[1].rows[0].is_moved());
    assert!(f.hunks[1].rows[1].is_moved());
}

#[test]
fn test_cross_cluster_moves_ignores_intra_cluster_matches() {
    // Within the same cluster (no Row::Ctx between del and add), pre-pass ignores
    // it
    let mut f = file(vec![hunk(vec![
        del(10, "first_statement();"),
        del(11, "second_statement();"),
        add(20, "first_statement();"),
        add(21, "second_statement();"),
    ])]);
    mark_cross_cluster_moves_with_profile(&mut f, None);
    assert!(!f.hunks[0].rows[0].is_moved());
    assert!(!f.hunks[0].rows[1].is_moved());
    assert!(!f.hunks[0].rows[2].is_moved());
    assert!(!f.hunks[0].rows[3].is_moved());

    // When separated by Row::Ctx into different clusters within the same hunk, it
    // matches
    let mut f2 = file(vec![hunk(vec![
        del(10, "first_statement();"),
        del(11, "second_statement();"),
        ctx(12, 12, "unchanged_context();"),
        add(20, "first_statement();"),
        add(21, "second_statement();"),
    ])]);
    mark_cross_cluster_moves_with_profile(&mut f2, None);
    assert!(f2.hunks[0].rows[0].is_moved());
    assert!(f2.hunks[0].rows[1].is_moved());
    assert!(f2.hunks[0].rows[3].is_moved());
    assert!(f2.hunks[0].rows[4].is_moved());
}
