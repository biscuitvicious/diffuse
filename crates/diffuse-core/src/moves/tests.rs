//! Unit tests for exact move detection and block move rules.

use super::*;
use crate::parse::{AddLine, CtxLine, DelLine, FileStatus, Hunk, Row, Spans};

fn del(num: u32, text: &str) -> Row {
    Row::Del(DelLine::new(num, text))
}

fn add(num: u32, text: &str) -> Row {
    Row::Add(AddLine::new(num, text))
}

fn ctx(old_num: u32, new_num: u32, text: &str) -> Row {
    Row::Ctx(CtxLine::new(old_num, new_num, text))
}

fn movsrc(num: u32, text: &str) -> Row {
    Row::MovSrc {
        line: DelLine::new(num, text),
        spans: Spans::default(),
        cached_runs: Vec::new(),
    }
}

fn movtgt(num: u32, text: &str) -> Row {
    Row::MovTgt {
        line: AddLine::new(num, text),
        spans: Spans::default(),
        cached_runs: Vec::new(),
    }
}

fn sep(text: &str) -> Row {
    Row::Sep(text.to_string())
}

fn mod_row(old_num: u32, new_num: u32, old_text: &str, new_text: &str) -> Row {
    Row::Mod {
        old_line: DelLine::new(old_num, old_text),
        new_line: AddLine::new(new_num, new_text),
        spans: Spans::default(),
        cached_runs: Vec::new(),
        is_anchor: false,
    }
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

#[test]
fn test_non_salient_lines_stick_to_move_group_and_are_not_stolen() {
    let rs = diffuse_syntax::registry::lang_profile("rs");
    let mut f = file(vec![hunk(vec![
        del(5, "if handled {"),
        add(10, "if handled {"),
        add(11, "    break;"),
        add(12, "}"),
        ctx(20, 20, "context_between();"),
        del(100, "hit_indicator_code();"),
        del(101, "    break;"),
        del(102, "}"),
        add(30, "hit_indicator_code();"),
        add(31, "    break;"),
        add(32, "}"),
    ])]);

    mark_moves_with_profile(&mut f, rs);

    // The hit_indicator block del(100..=102) must match add(30..=32)
    let add_30 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(30))
        .unwrap();
    let add_31 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(31))
        .unwrap();
    let add_32 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(32))
        .unwrap();

    assert!(add_30.is_moved(), "add 30 hit_indicator must be moved");
    assert!(
        add_31.is_moved(),
        "add 31 break; must stick to hit_indicator and be moved"
    );
    assert!(
        add_32.is_moved(),
        "add 32 }} must stick to hit_indicator and be moved"
    );

    // The handled block add(11..=12) must NOT steal break; and }
    let add_11 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(11))
        .unwrap();
    let add_12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(12))
        .unwrap();
    assert!(
        !add_11.is_moved(),
        "add 11 break; must NOT be moved (stolen)"
    );
    assert!(!add_12.is_moved(), "add 12 }} must NOT be moved (stolen)");
}

#[test]
fn test_three_line_trivial_context_run_still_splits_clusters() {
    // A context run of blank/delimiter lines with no salient code still
    // establishes a cluster boundary when it spans at least 3 lines, so a
    // trimmed-only (reindented) pair across it is a cross-cluster relocation.
    let mut f = file(vec![hunk(vec![
        del(10, "  first_statement();"),
        del(11, "  second_statement();"),
        ctx(12, 12, ""),
        ctx(13, 13, "}"),
        ctx(14, 14, ""),
        add(20, "    first_statement();"),
        add(21, "    second_statement();"),
    ])]);
    mark_moves_with_profile(&mut f, None);
    for (idx, expect_moved) in [(0, true), (1, true), (5, true), (6, true)] {
        assert_eq!(
            f.hunks[0].rows[idx].is_moved(),
            expect_moved,
            "row {idx} moved-bit mismatch"
        );
    }
}

#[test]
fn test_pre_pass_locks_trimmed_pair_across_trivial_context() {
    // The pre-pass is a broad shield: ANY context row (even a single blank
    // line) separates clusters for its purposes, so a trimmed-only pair
    // across it is locked in before collapse can steal either side into a
    // spurious Mod. The main pass applies the narrower salient-aware rule.
    let mut f = file(vec![hunk(vec![
        del(10, "  first_statement();"),
        del(11, "  second_statement();"),
        ctx(12, 12, ""),
        add(20, "    first_statement();"),
        add(21, "    second_statement();"),
    ])]);
    mark_cross_cluster_moves_with_profile(&mut f, None);
    assert!(f.hunks[0].rows[0].is_moved());
    assert!(f.hunks[0].rows[1].is_moved());
    assert!(f.hunks[0].rows[3].is_moved());
    assert!(f.hunks[0].rows[4].is_moved());
}

#[test]
fn test_indent_width_counts_leading_whitespace() {
    assert_eq!(super::indent_width("    x();"), 4);
    assert_eq!(super::indent_width("x();"), 0);
    assert_eq!(super::indent_width(""), 0);
    assert_eq!(super::indent_width("   "), 3);
}

#[test]
fn test_pair_delta_requires_trimmed_equal_non_blank() {
    assert_eq!(super::pair_delta("    a();", "        a();"), Some(4));
    assert_eq!(super::pair_delta("        a();", "    a();"), Some(-4));
    assert_eq!(super::pair_delta("a();", "a();"), Some(0));
    assert_eq!(super::pair_delta("    a();", "    b();"), None);
    assert_eq!(super::pair_delta("", "    "), None);
    assert_eq!(super::pair_delta("    a();", ""), None);
}

#[test]
fn test_tail_pair_matches_requires_uniform_delta_and_exact_text() {
    // Same trimmed text at the expected delta matches.
    assert!(super::tail_pair_matches("    };", "        };", 4));
    // Same trimmed text at a different delta does not.
    assert!(!super::tail_pair_matches("    };", "            };", 4));
    // Different text does not match.
    assert!(!super::tail_pair_matches("    };", "        {", 4));
    // Trailing-whitespace differences break the match (exactness).
    assert!(!super::tail_pair_matches("    a();  ", "        a();", 4));
    // Blank pairs match by position alone, regardless of delta.
    assert!(super::tail_pair_matches("", "", 4));
    assert!(super::tail_pair_matches("   ", "\t", -2));
    // Blank/non-blank never matches.
    assert!(!super::tail_pair_matches("", "        };", 4));
    assert!(!super::tail_pair_matches("    };", "", 4));
}

// Miniature of the dispatch.rs hit_indicator topology: a locked core block,
// trailing Add leftovers, and old trailing rows mis-paired as Ctx far away.
// The main pass locks the core but cannot touch the trailing rows; tail
// absorption (Phase 2) must sweep them into the move at uniform delta.
#[test]
fn test_mov_tail_absorbs_uniform_shift_continuation() {
    // NOTE: the boundary ctx sits *between* the runs in row order (forcing
    // the cluster split) but its line numbers live outside the move (old 5
    // before the core, new 50 after the trailing rows), mirroring the real
    // dispatch case where the mid-hunk context precedes the block in file
    // order. File-order tail stepping therefore skips it.
    let mut f = file(vec![hunk(vec![
        del(10, "    core_a();"),
        del(11, "        core_b();"),
        ctx(5, 50, "real_code();"),
        add(20, "        core_a();"),
        add(21, "            core_b();"),
        add(22, "        };"),
        add(23, ""),
        ctx(12, 100, "    };"),
        ctx(13, 101, ""),
    ])]);
    let locked = mark_moves_with_profile(&mut f, None);
    // Core locked by the main pass (trimmed-only, cross-cluster).
    assert!(f.hunks[0].rows[0].is_moved());
    assert!(f.hunks[0].rows[1].is_moved());
    assert!(f.hunks[0].rows[3].is_moved());
    assert!(f.hunks[0].rows[4].is_moved());
    // Trailing rows untouched by the main pass.
    assert!(!f.hunks[0].rows[5].is_moved());
    assert!(!f.hunks[0].rows[6].is_moved());

    let _ = super::absorb_mov_tails(&mut f, &locked, None, true);

    // Tail absorbed at Δ=+4: old 12/13 + new 22/23 join the move.
    for line in [12, 13] {
        let row = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.old_line() == Some(line))
            .unwrap_or_else(|| panic!("old line {line} must exist"));
        assert!(row.is_moved(), "old line {line} must be absorbed");
    }
    for line in [22, 23] {
        let row = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.new_line() == Some(line))
            .unwrap_or_else(|| panic!("new line {line} must exist"));
        assert!(row.is_moved(), "new line {line} must be absorbed");
    }
    // Orphaned far partners fall back to plain leftovers.
    for line in [100, 101] {
        let row = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.new_line() == Some(line))
            .unwrap_or_else(|| panic!("new line {line} must exist"));
        assert!(
            !row.is_moved(),
            "orphaned far partner (new {line}) must stay a leftover"
        );
    }
}

#[test]
fn test_resolve_mov_pairs_by_line_drops_stale_pairs() {
    // Exact-text bypass locks old 10 <-> new 20; the other rows never lock.
    let mut f = file(vec![hunk(vec![
        del(10, "moved line"),
        add(20, "moved line"),
        del(11, "other old"),
        add(21, "other new"),
    ])]);
    let locked = mark_moves_with_profile(&mut f, None);
    assert_eq!(locked.len(), 1);

    // A locked line pair re-resolves to the same positions.
    assert_eq!(resolve_mov_pairs_by_line(&f, &[(10, 20)]), locked);
    // Rows that are not moves, half-moved pairs, and unknown lines drop.
    assert!(resolve_mov_pairs_by_line(&f, &[(11, 21)]).is_empty());
    assert!(resolve_mov_pairs_by_line(&f, &[(10, 21)]).is_empty());
    assert!(resolve_mov_pairs_by_line(&f, &[(99, 99)]).is_empty());
}

fn row_sig(r: &Row) -> (String, Option<u32>, Option<u32>) {
    let kind = match r {
        Row::Ctx(_) => "ctx",
        Row::Del(_) => "del",
        Row::Add(_) => "add",
        Row::MovSrc { .. } => "movsrc",
        Row::MovTgt { .. } => "movtgt",
        _ => "other",
    }
    .to_string();
    (kind, r.old_line(), r.new_line())
}

#[test]
fn test_relocate_del_orphans_merge_up_into_preceding_del_run() {
    // Inline Ctx-break emission interleaves orphans with the move run:
    // del, mov, del, mov, … — relocation must gather the orphans up into
    // one contiguous del run behind the preceding del block.
    let mut f = file(vec![hunk(vec![
        ctx(1, 1, "c();"),
        del(308, "d();"),
        movtgt(310, "m1();"),
        movtgt(311, "m2();"),
        del(309, "d();"),
        movtgt(312, "m3();"),
        del(310, "d();"),
        movsrc(320, "s();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[309, 310], &[]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("ctx".to_string(), Some(1), Some(1)),
            ("del".to_string(), Some(308), None),
            ("del".to_string(), Some(309), None),
            ("del".to_string(), Some(310), None),
            ("movtgt".to_string(), None, Some(310)),
            ("movtgt".to_string(), None, Some(311)),
            ("movtgt".to_string(), None, Some(312)),
            ("movsrc".to_string(), Some(320), None),
        ],
        "del orphans must merge up into one contiguous del run"
    );
}

#[test]
fn test_relocate_del_orphan_anchors_after_any_row_kind() {
    // The anchor is the row carrying old line seed-1, whatever kind it is:
    // here old 308 sits in a MovSrc row (absorbed into a move), so the
    // orphan attaches right after it instead of skipping to the del block.
    let mut f = file(vec![hunk(vec![
        del(300, "d();"),
        movsrc(308, "s();"),
        movtgt(310, "m1();"),
        del(309, "d();"),
        movtgt(311, "m2();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[309], &[]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("del".to_string(), Some(300), None),
            ("movsrc".to_string(), Some(308), None),
            ("del".to_string(), Some(309), None),
            ("movtgt".to_string(), None, Some(310)),
            ("movtgt".to_string(), None, Some(311)),
        ],
        "del orphan must sit right after its preceding old line"
    );
}

#[test]
fn test_relocate_del_orphan_unit_aborts_at_sep_between_anchor_and_unit() {
    // A `Sep` row strictly between the anchor row (old 199) and the unit
    // (dels 200-201) must abort the relocation: the unit stays in place.
    // The seed chains del 201 across the neutral `MovTgt` row (no Ctx/Mod/
    // Sep boundary between them), so the unit spans the boundary region —
    // exactly the case the Mod/Sep crossing guard exists for.
    let mut f = file(vec![hunk(vec![
        del(199, "a();"),
        sep(""),
        movtgt(300, "m();"),
        del(200, "b();"),
        del(201, "c();"),
        ctx(202, 302, "d();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[200], &[]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("del".to_string(), Some(199), None),
            ("other".to_string(), None, None),
            ("movtgt".to_string(), None, Some(300)),
            ("del".to_string(), Some(200), None),
            ("del".to_string(), Some(201), None),
            ("ctx".to_string(), Some(202), Some(302)),
        ],
        "del unit must stay in place behind a Sep row"
    );

    // Control: the same layout without the Sep row relocates, so the
    // assertion above is sensitive to the guard rather than vacuous.
    let mut f = file(vec![hunk(vec![
        del(199, "a();"),
        movtgt(300, "m();"),
        del(200, "b();"),
        del(201, "c();"),
        ctx(202, 302, "d();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[200], &[]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("del".to_string(), Some(199), None),
            ("del".to_string(), Some(200), None),
            ("del".to_string(), Some(201), None),
            ("movtgt".to_string(), None, Some(300)),
            ("ctx".to_string(), Some(202), Some(302)),
        ],
        "identical layout without the Sep row must relocate"
    );
}

#[test]
fn test_relocate_add_orphans_merge_down_before_following_line() {
    // Mirror image: the orphan drops to just before the row carrying new
    // line seed+1 (here a plain leftover add), preserving file order.
    let mut f = file(vec![hunk(vec![
        movsrc(330, "s();"),
        add(363, "a();"),
        del(340, "d();"),
        add(364, "b();"),
        ctx(342, 352, "c();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[], &[363]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("movsrc".to_string(), Some(330), None),
            ("del".to_string(), Some(340), None),
            ("add".to_string(), None, Some(363)),
            ("add".to_string(), None, Some(364)),
            ("ctx".to_string(), Some(342), Some(352)),
        ],
        "add orphan must sit just before its following new line"
    );
}

#[test]
fn test_relocate_add_orphan_without_following_line_stays() {
    // No row below carries a greater new line: with no line-contiguity
    // target the orphan stays in place instead of merging out of order.
    let mut f = file(vec![hunk(vec![
        movsrc(330, "s();"),
        add(363, "a();"),
        del(340, "d();"),
        add(351, "b();"),
        ctx(342, 352, "c();"),
    ])]);
    super::relocate_tail_orphans(&mut f, &[], &[363]);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(row_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("movsrc".to_string(), Some(330), None),
            ("add".to_string(), None, Some(363)),
            ("del".to_string(), Some(340), None),
            ("add".to_string(), None, Some(351)),
            ("ctx".to_string(), Some(342), Some(352)),
        ],
        "orphan add without a following line must stay in place"
    );
}

#[test]
fn test_tail_absorption_stops_when_delta_changes_mid_run() {
    // Core has delta +4 (old indent 4 -> new indent 8).
    // Tail row 1 (old 12, new 22): delta +4 -> matches and absorbed.
    // Tail row 2 (old 13, new 23): delta +8 (old indent 4 -> new indent 12) ->
    // delta drift, stops. Tail row 3 (old 14, new 24): delta +4 -> not absorbed
    // because run was terminated.
    //
    // The `ctx(5, 50, "boundary();")` between the del and add runs is required
    // to split them into different clusters. `mark_moves_with_profile` only
    // locks trimmed-only (re-indented) pairs that are cross-cluster, so without
    // the boundary the core would not be locked and there would be nothing for
    // tail absorption to extend.
    let mut f = file(vec![hunk(vec![
        del(10, "    core_a();"),
        del(11, "        core_b();"),
        ctx(5, 50, "boundary();"),
        add(20, "        core_a();"),
        add(21, "            core_b();"),
        del(12, "    tail_1();"),
        del(13, "    tail_2();"),
        del(14, "    tail_3();"),
        add(22, "        tail_1();"),
        add(23, "            tail_2();"),
        add(24, "        tail_3();"),
    ])]);
    let locked = mark_moves_with_profile(&mut f, None);
    assert_eq!(locked.len(), 2);

    let absorbed = super::absorb_mov_tails(&mut f, &locked, None, false);
    assert_eq!(absorbed.absorbed, vec![(12, 22)]);

    let row_12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(12))
        .unwrap();
    let row_13 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(13))
        .unwrap();
    let row_14 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(14))
        .unwrap();

    assert!(
        row_12.is_moved(),
        "tail_1 must be absorbed at uniform delta"
    );
    assert!(
        !row_13.is_moved(),
        "tail_2 must NOT be absorbed due to delta drift"
    );
    assert!(
        !row_14.is_moved(),
        "tail_3 must NOT be absorbed after run termination"
    );
}

#[test]
fn test_tail_absorption_stops_at_real_edit() {
    // Core has delta +4.
    // Tail row 1 (old 12, new 22): identical text shifted by +4 -> absorbed.
    // Tail row 2 (old 13, new 23): substantive edit ("value_a" vs "value_b") ->
    // stops immediately.
    //
    // The `ctx(5, 50, "boundary();")` between the del and add runs is required
    // to split them into different clusters so the core is locked by the
    // cross-cluster trimmed-match rule (see Phase 3 comment in
    // `test_tail_absorption_stops_when_delta_changes_mid_run`).
    let mut f = file(vec![hunk(vec![
        del(10, "    core_a();"),
        del(11, "        core_b();"),
        ctx(5, 50, "boundary();"),
        add(20, "        core_a();"),
        add(21, "            core_b();"),
        del(12, "    tail_same();"),
        del(13, "    compute(value_a);"),
        add(22, "        tail_same();"),
        add(23, "        compute(value_b);"),
    ])]);
    let locked = mark_moves_with_profile(&mut f, None);
    assert_eq!(locked.len(), 2);

    let absorbed = super::absorb_mov_tails(&mut f, &locked, None, false);
    assert_eq!(absorbed.absorbed, vec![(12, 22)]);

    let row_13 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(13))
        .unwrap();
    let row_23 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(23))
        .unwrap();
    assert!(
        !row_13.is_moved(),
        "old 13 is a real edit and must not be absorbed"
    );
    assert!(
        !row_23.is_moved(),
        "new 23 is a real edit and must not be absorbed"
    );
}

#[test]
fn test_tail_absorption_stops_before_mod_or_already_moved_row() {
    // Core has delta +4.
    // Next row in file order is a Mod row (not Del/Add/Ctx).
    // Tail absorption must not attempt to claim or break Mod rows.
    //
    // The `ctx(5, 50, "boundary();")` is present for the same cluster-split
    // reason as the other Phase 3 tests: without it the core del/add run is
    // in the same cluster and `mark_moves_with_profile` does not lock it.
    let mut f = file(vec![hunk(vec![
        del(10, "    core_a();"),
        del(11, "        core_b();"),
        ctx(5, 50, "boundary();"),
        add(20, "        core_a();"),
        add(21, "            core_b();"),
        mod_row(12, 22, "    let x = 1;", "        let x = 1;"),
    ])]);
    let locked = mark_moves_with_profile(&mut f, None);
    assert_eq!(locked.len(), 2);

    let absorbed = super::absorb_mov_tails(&mut f, &locked, None, true);
    assert!(
        absorbed.absorbed.is_empty(),
        "Mod row must terminate tail walk"
    );
}

#[test]
fn test_tail_absorption_hunk_boundary_confinement() {
    // Core spans hunk 0 (source dels) and hunk 1 (target adds). Neither hunk has
    // subsequent rows. Hunk 2 contains candidates with matching text and delta,
    // but residing in an unrelated hunk. Tail absorption must remain confined to
    // the hunks containing the core move block.
    let mut f = file(vec![
        hunk(vec![del(10, "    core_a();"), del(11, "        core_b();")]),
        hunk(vec![
            add(20, "        core_a();"),
            add(21, "            core_b();"),
        ]),
        hunk(vec![
            del(12, "    tail_candidate();"),
            add(22, "        tail_candidate();"),
        ]),
    ]);
    let locked = mark_moves_with_profile(&mut f, None);
    assert_eq!(locked.len(), 2);

    let absorbed = super::absorb_mov_tails(&mut f, &locked, None, false);
    assert!(
        absorbed.absorbed.is_empty(),
        "Tail absorption must not cross hunk boundaries"
    );
    assert!(!f.hunks[2].rows[0].is_moved());
    assert!(!f.hunks[2].rows[1].is_moved());
}

fn collapsed_sig(r: &Row) -> (String, Option<u32>, Option<u32>) {
    let kind = match r {
        Row::Ctx(_) => "ctx",
        Row::Mod { .. } => "mod",
        Row::MovSrc { .. } => "movsrc",
        Row::MovTgt { .. } => "movtgt",
        _ => "other",
    }
    .to_string();
    (kind, r.old_line(), r.new_line())
}

#[test]
fn test_touching_bottom_halves_collapse_to_mod() {
    // Target half immediately followed by its source half: the rigidly
    // shifted pairs collapse into direct Mod rows in target order.
    let mut f = file(vec![hunk(vec![
        movtgt(10, "    a();"),
        movtgt(11, "    b();"),
        movsrc(20, "a();"),
        movsrc(21, "b();"),
    ])]);
    super::collapse_touching_moves(&mut f, &[(20, 10), (21, 11)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("mod".to_string(), Some(20), Some(10)),
            ("mod".to_string(), Some(21), Some(11)),
        ],
        "touching corresponding halves must collapse to Mod runs"
    );
    for r in &f.hunks[0].rows {
        let Row::Mod { spans, .. } = r else {
            panic!("expected Mod, got {r:?}")
        };
        assert!(!spans.is_empty(), "indent delta must carry word spans");
    }
}

#[test]
fn test_touching_top_halves_collapse_identical_to_ctx() {
    // Source half immediately followed by its target half, identical text:
    // collapse yields Ctx, not Mod.
    let mut f = file(vec![hunk(vec![movsrc(20, "a();"), movtgt(10, "a();")])]);
    super::collapse_touching_moves(&mut f, &[(20, 10)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![("ctx".to_string(), Some(20), Some(10))],
        "identical touching pairs must collapse to Ctx"
    );
}

#[test]
fn test_touching_mixed_attribution_collapses_corresponding_part_only() {
    // Only new 10 pairs into the adjacent source run (old 20); new 11
    // pairs outside it and old 21 pairs outside the target run. The
    // collapse extends as far as the mutual pairing reaches; the rest
    // stays moved.
    let mut f = file(vec![hunk(vec![
        movtgt(10, "    a();"),
        movtgt(11, "    b();"),
        movsrc(20, "a();"),
        movsrc(21, "c();"),
    ])]);
    super::collapse_touching_moves(&mut f, &[(20, 10), (99, 11), (21, 12)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("mod".to_string(), Some(20), Some(10)),
            ("movtgt".to_string(), None, Some(11)),
            ("movsrc".to_string(), Some(21), None),
        ],
        "only the mutually-paired part must collapse"
    );
}

#[test]
fn test_touching_non_corresponding_halves_stay_moved() {
    // Adjacent halves paired elsewhere are different moves touching by
    // accident: no mutual pairing, no collapse.
    let mut f = file(vec![hunk(vec![movtgt(10, "    a();"), movsrc(20, "a();")])]);
    super::collapse_touching_moves(&mut f, &[(50, 10), (20, 60)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("movtgt".to_string(), None, Some(10)),
            ("movsrc".to_string(), Some(20), None),
        ],
        "non-corresponding touching halves must stay moved"
    );
}

#[test]
fn test_touching_collapse_grows_through_fragmented_runs() {
    // One rigid shift fragmented across two touching windows: the seed
    // window's pairs grow through line-adjacent mutual pairs even where
    // the grown rows' own runs never touch, so the whole shift collapses.
    let mut f = file(vec![hunk(vec![
        movtgt(10, "    a();"),
        movtgt(11, "    b();"),
        movsrc(20, "a();"),
        movsrc(21, "b();"),
        movtgt(12, "    c();"),
        movsrc(22, "c();"),
    ])]);
    super::collapse_touching_moves(&mut f, &[(20, 10), (21, 11), (22, 12)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("mod".to_string(), Some(20), Some(10)),
            ("mod".to_string(), Some(21), Some(11)),
            ("mod".to_string(), Some(22), Some(12)),
        ],
        "growth must collapse the whole line-contiguous shift"
    );
}

#[test]
fn test_touching_growth_stops_at_unpaired_lines() {
    // Growth halts where line-adjacency breaks the mutual pairing: new 12
    // pairs outside the old run, so only the seeded pairs collapse.
    let mut f = file(vec![hunk(vec![
        movtgt(10, "    a();"),
        movtgt(11, "    b();"),
        movsrc(20, "a();"),
        movsrc(21, "b();"),
        movtgt(12, "    c();"),
        movsrc(22, "c();"),
    ])]);
    super::collapse_touching_moves(&mut f, &[(20, 10), (21, 11), (99, 12), (22, 13)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("mod".to_string(), Some(20), Some(10)),
            ("mod".to_string(), Some(21), Some(11)),
            ("movtgt".to_string(), None, Some(12)),
            ("movsrc".to_string(), Some(22), None),
        ],
        "growth must stop where mutual pairing breaks"
    );
}

#[test]
fn test_separated_halves_stay_moved() {
    // A context row between the halves: not touching, no collapse even
    // with full correspondence.
    let mut f = file(vec![hunk(vec![
        movtgt(10, "    a();"),
        ctx(5, 6, "c();"),
        movsrc(20, "a();"),
    ])]);
    super::collapse_touching_moves(&mut f, &[(20, 10)], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("movtgt".to_string(), None, Some(10)),
            ("ctx".to_string(), Some(5), Some(6)),
            ("movsrc".to_string(), Some(20), None),
        ],
        "separated halves must stay moved"
    );
}

#[test]
fn test_touching_collapse_no_pairs_is_noop() {
    // Without recorded pairs the pass is a no-op.
    let mut f = file(vec![hunk(vec![movtgt(10, "    a();"), movsrc(20, "a();")])]);
    super::collapse_touching_moves(&mut f, &[], None);
    let sigs: Vec<_> = f.hunks[0].rows.iter().map(collapsed_sig).collect();
    assert_eq!(
        sigs,
        vec![
            ("movtgt".to_string(), None, Some(10)),
            ("movsrc".to_string(), Some(20), None),
        ],
        "empty pair list must leave rows untouched"
    );
}
