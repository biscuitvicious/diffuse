use std::borrow::Cow;

use diffuse_syntax::{color::TokenColor, registry::lang_profile};

use super::*;
use crate::{
    merge::RunKind,
    parse::{RowGroup, RowKind},
};

fn make_run(kind: RunKind, text: &str) -> OutputRun {
    OutputRun {
        kind,
        color: TokenColor::Default,
        sgr: None,
        text: Cow::Owned(text.to_string()),
    }
}

#[test]
fn test_mod_glyph_variants_and_helpers() {
    assert_eq!(ModGlyph::Top.as_char(), '┌');
    assert_eq!(ModGlyph::Top.as_str(), "┌");
    assert!(!ModGlyph::Top.is_bottom());

    assert_eq!(ModGlyph::Mid.as_char(), '│');
    assert_eq!(ModGlyph::Mid.as_str(), "│");
    assert!(!ModGlyph::Mid.is_bottom());

    assert_eq!(ModGlyph::Bottom.as_char(), '└');
    assert_eq!(ModGlyph::Bottom.as_str(), "└");
    assert!(ModGlyph::Bottom.is_bottom());

    // Isolated (single row) side glyphs
    assert_eq!(ModGlyph::for_side(ModSide::Del), ModGlyph::Top);
    assert_eq!(ModGlyph::for_side(ModSide::Add), ModGlyph::Bottom);

    // Single-row cluster (total = 1)
    assert_eq!(ModGlyph::for_cluster(ModSide::Del, 0, 1), ModGlyph::Top);
    assert_eq!(ModGlyph::for_cluster(ModSide::Add, 0, 1), ModGlyph::Bottom);

    // Multi-row cluster (total = 3)
    assert_eq!(ModGlyph::for_cluster(ModSide::Del, 0, 3), ModGlyph::Top);
    assert_eq!(ModGlyph::for_cluster(ModSide::Del, 1, 3), ModGlyph::Mid);
    assert_eq!(ModGlyph::for_cluster(ModSide::Del, 2, 3), ModGlyph::Mid);
    assert_eq!(ModGlyph::for_cluster(ModSide::Add, 0, 3), ModGlyph::Mid);
    assert_eq!(ModGlyph::for_cluster(ModSide::Add, 1, 3), ModGlyph::Mid);
    assert_eq!(ModGlyph::for_cluster(ModSide::Add, 2, 3), ModGlyph::Bottom);
}

#[test]
fn test_output_hunk_valid_invariants() {
    let hunk = OutputHunk {
        old_start: 1,
        old_count: 3,
        new_start: 1,
        new_count: 3,
        rows: vec![
            OutputRow {
                kind: RowKind::Ctx,
                mov_side: None,
                new_line: Some(1),
                old_line: Some(1),
                runs: vec![make_run(RunKind::Ctx, "fn main() {")],
                wholesale: false,
                group: None,
            },
            OutputRow {
                kind: RowKind::Mod,
                mov_side: None,
                new_line: Some(2),
                old_line: Some(2),
                runs: vec![
                    make_run(RunKind::Ctx, "    let x = "),
                    make_run(RunKind::Del, "1"),
                    make_run(RunKind::Add, "2"),
                    make_run(RunKind::Ctx, ";"),
                ],
                wholesale: false,
                group: None,
            },
            OutputRow {
                kind: RowKind::Ctx,
                mov_side: None,
                new_line: Some(3),
                old_line: Some(3),
                runs: vec![make_run(RunKind::Ctx, "}")],
                wholesale: false,
                group: None,
            },
        ],
    };

    assert!(hunk.validate_invariants().is_ok());
}

#[test]
fn test_output_hunk_monotonicity_violations() {
    // Non-monotonic new_line
    let hunk_bad_new = OutputHunk {
        old_start: 1,
        old_count: 2,
        new_start: 1,
        new_count: 2,
        rows: vec![
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(10),
                old_line: None,
                runs: vec![make_run(RunKind::Add, "line 10")],
                wholesale: false,
                group: None,
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(10), // equal is also non-monotonic
                old_line: None,
                runs: vec![make_run(RunKind::Add, "line 10 duplicate")],
                wholesale: false,
                group: None,
            },
        ],
    };
    assert!(hunk_bad_new.validate_invariants().is_err());
}

#[test]
fn test_output_hunk_row_kind_line_number_violations() {
    // Add with old_line
    let row_add_with_old = OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![make_run(RunKind::Add, "test")],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_add_with_old],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());

    // Del with new_line
    let row_del_with_new = OutputRow {
        kind: RowKind::Del,
        mov_side: None,
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![make_run(RunKind::Del, "test")],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_del_with_new],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());

    // Mod missing old_line
    let row_mod_no_old = OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        new_line: Some(1),
        old_line: None,
        runs: vec![make_run(RunKind::Add, "test")],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_mod_no_old],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());

    // MovSrc with new_line
    let row_mov_src_with_new = OutputRow {
        kind: RowKind::Mov,
        mov_side: Some(MovSide::Src),
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![make_run(RunKind::Del, "test")],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_mov_src_with_new],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());

    // MovTgt with old_line
    let row_mov_tgt_with_old = OutputRow {
        kind: RowKind::Mov,
        mov_side: Some(MovSide::Tgt),
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![make_run(RunKind::Add, "test")],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_mov_tgt_with_old],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());
}

#[test]
fn test_output_hunk_wholesale_mod_violation() {
    let row_wholesale = OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![make_run(RunKind::Del, "old"), make_run(RunKind::Add, "new")],
        wholesale: true,
        group: None,
    };
    let hunk = OutputHunk {
        rows: vec![row_wholesale],
        ..Default::default()
    };
    assert!(hunk.validate_invariants().is_err());
}

#[test]
fn test_output_hunk_disk_exact_violations() {
    // Forward text mismatch: Mod row with text that does not equal non-del runs
    // When a Del row contains non-Del runs (e.g. Add run):
    let bad_del = OutputRow {
        kind: RowKind::Del,
        mov_side: None,
        new_line: None,
        old_line: Some(1),
        runs: vec![make_run(RunKind::Del, "old"), make_run(RunKind::Add, "new")],
        wholesale: false,
        group: None,
    };
    let hunk_bad_del = OutputHunk {
        rows: vec![bad_del],
        ..Default::default()
    };
    assert!(hunk_bad_del.validate_invariants().is_err());
    // When a Ctx row contains non-Ctx runs (e.g. Del run):
    let bad_ctx = OutputRow {
        kind: RowKind::Ctx,
        mov_side: None,
        new_line: Some(1),
        old_line: Some(1),
        runs: vec![
            make_run(RunKind::Ctx, "abc"),
            make_run(RunKind::Del, "extra del"),
        ],
        wholesale: false,
        group: None,
    };
    let hunk_bad_ctx = OutputHunk {
        rows: vec![bad_ctx],
        ..Default::default()
    };
    assert!(hunk_bad_ctx.validate_invariants().is_err());
}

#[test]
fn test_output_hunk_reflow_violations() {
    // 1. Orphan aux row
    let hunk_orphan = OutputHunk {
        rows: vec![OutputRow {
            kind: RowKind::Add,
            mov_side: None,
            new_line: Some(5),
            old_line: None,
            runs: vec![make_run(RunKind::Add, "aux")],
            wholesale: false,
            group: Some(RowGroup::Aux { anchor: 99 }), // nonexistent anchor 99
        }],
        ..Default::default()
    };
    assert!(hunk_orphan.validate_invariants().is_err());

    // 2. Aux row line numbers non-monotonic
    let hunk_aux_non_monotonic = OutputHunk {
        rows: vec![
            OutputRow {
                kind: RowKind::Mod,
                mov_side: None,
                new_line: Some(10),
                old_line: Some(10),
                runs: vec![
                    make_run(RunKind::Ctx, "use std::{"),
                    make_run(RunKind::MovedSrc, "io, ops"),
                ],
                wholesale: false,
                group: Some(RowGroup::Anchor),
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(12),
                old_line: None,
                runs: vec![make_run(RunKind::MovedTgt, "io,")],
                wholesale: false,
                group: Some(RowGroup::Aux { anchor: 10 }),
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(11), // 11 <= 12 non-monotonic!
                old_line: None,
                runs: vec![make_run(RunKind::MovedTgt, "ops")],
                wholesale: false,
                group: Some(RowGroup::Aux { anchor: 10 }),
            },
        ],
        ..Default::default()
    };
    assert!(hunk_aux_non_monotonic.validate_invariants().is_err());

    // 3. Import reflow old-text reconstruction mismatch
    let rust_profile = lang_profile("rs");
    let hunk_import_mismatch = OutputHunk {
        rows: vec![
            OutputRow {
                kind: RowKind::Mod,
                mov_side: None,
                new_line: Some(10),
                old_line: Some(10),
                runs: vec![
                    make_run(RunKind::Ctx, "use std::{"),
                    make_run(RunKind::MovedSrc, "io, ops};"),
                ],
                wholesale: false,
                group: Some(RowGroup::Anchor),
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(11),
                old_line: None,
                runs: vec![make_run(RunKind::MovedTgt, "net, ops};")], // "net" instead of "io"
                wholesale: false,
                group: Some(RowGroup::Aux { anchor: 10 }),
            },
        ],
        ..Default::default()
    };
    assert!(
        hunk_import_mismatch
            .validate_invariants_with_profile(rust_profile)
            .is_err()
    );

    // 4. Character conservation mismatch
    let hunk_char_mismatch = OutputHunk {
        rows: vec![
            OutputRow {
                kind: RowKind::Mod,
                mov_side: None,
                new_line: Some(10),
                old_line: Some(10),
                runs: vec![
                    make_run(RunKind::Ctx, "items = ["),
                    make_run(RunKind::MovedSrc, "alpha, beta"),
                ],
                wholesale: false,
                group: Some(RowGroup::Anchor),
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(11),
                old_line: None,
                runs: vec![make_run(RunKind::MovedTgt, "alpha, gamma")], // gamma != beta
                wholesale: false,
                group: Some(RowGroup::Aux { anchor: 10 }),
            },
        ],
        ..Default::default()
    };
    assert!(hunk_char_mismatch.validate_invariants().is_err());
}

#[test]
fn test_output_file_counts_and_fence_invariants() {
    let mut file = OutputFile {
        path: "src/main.rs".to_string(),
        counts: FileCounts {
            adds: 1,
            dels: 0,
            mods: 0,
            movs: 0,
            ..Default::default()
        },
        hunks: vec![OutputHunk {
            rows: vec![OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                new_line: Some(1),
                old_line: None,
                runs: vec![make_run(RunKind::Add, "line 1")],
                wholesale: false,
                group: None,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    assert!(file.validate_invariants().is_ok());

    // Mismatched counts
    file.counts.adds = 2; // expected 1
    assert!(file.validate_invariants().is_err());
    file.counts.adds = 1;

    // Hunk count exceeds file count
    file.hunks[0].rows.push(OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(2),
        old_line: None,
        runs: vec![make_run(RunKind::Add, "line 2")],
        wholesale: false,
        group: None,
    });
    assert!(file.validate_invariants().is_err());
    file.hunks[0].rows.pop();

    // Overlapping fences
    file.new_line_count = 100;
    file.fences = vec![
        OutputFence {
            start: 10,
            end: 20,
            lang: "rs".to_string(),
        },
        OutputFence {
            start: 15, // overlaps with 10..=20
            end: 30,
            lang: "rs".to_string(),
        },
    ];
    assert!(file.validate_invariants().is_err());
}

#[test]
fn test_output_file_untracked_reverse_involution() {
    let file = OutputFile {
        path: "src/untracked.rs".to_string(),
        status: FileStatus::Untracked,
        new_bytes: Some(42),
        old_bytes: None,
        new_line_count: 2,
        old_line_count: 0,
        counts: FileCounts {
            adds: 2,
            dels: 0,
            mods: 0,
            movs: 0,
            ..Default::default()
        },
        hunks: vec![OutputHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 2,
            rows: vec![
                OutputRow {
                    kind: RowKind::Add,
                    mov_side: None,
                    new_line: Some(1),
                    old_line: None,
                    runs: vec![make_run(RunKind::Add, "line 1")],
                    wholesale: false,
                    group: None,
                },
                OutputRow {
                    kind: RowKind::Add,
                    mov_side: None,
                    new_line: Some(2),
                    old_line: None,
                    runs: vec![make_run(RunKind::Add, "line 2")],
                    wholesale: false,
                    group: None,
                },
            ],
        }],
        new_content: vec!["line 1".to_string(), "line 2".to_string()],
        ..Default::default()
    };

    assert_eq!(file.byte_size(), Some(42));
    assert_eq!(file.display_size(), "+42B");

    let rev = file.clone().reverse();
    assert_eq!(rev.status, "untracked");
    assert_eq!(rev.byte_size(), Some(42));
    assert_eq!(rev.display_size(), "+42B");
    assert_eq!(rev.counts.adds, 0);
    assert_eq!(rev.counts.dels, 2);

    let rev_rev = rev.reverse();
    assert_eq!(rev_rev, file);
}

#[test]
fn test_output_file_moved_character_conservation() {
    let mut file = OutputFile {
        path: "src/test.rs".to_string(),
        status: FileStatus::Modified,
        new_line_count: 2,
        old_line_count: 2,
        counts: FileCounts {
            mods: 2,
            ..Default::default()
        },
        hunks: vec![OutputHunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 2,
            rows: vec![
                OutputRow {
                    kind: RowKind::Mod,
                    mov_side: None,
                    new_line: Some(1),
                    old_line: Some(1),
                    runs: vec![
                        make_run(RunKind::Ctx, "foo("),
                        make_run(RunKind::MovedSrc, "bar"),
                        make_run(RunKind::Ctx, ")"),
                    ],
                    wholesale: false,
                    group: None,
                },
                OutputRow {
                    kind: RowKind::Mod,
                    mov_side: None,
                    new_line: Some(2),
                    old_line: Some(2),
                    runs: vec![
                        make_run(RunKind::Ctx, "baz("),
                        make_run(RunKind::MovedTgt, "bar"),
                        make_run(RunKind::Ctx, ")"),
                    ],
                    wholesale: false,
                    group: None,
                },
            ],
        }],
        ..Default::default()
    };

    assert!(file.validate_invariants().is_ok());

    // Introduce mismatch: MovedTgt has "qux" instead of "bar"
    file.hunks[0].rows[1].runs[1] = make_run(RunKind::MovedTgt, "qux");
    let err = file.validate_invariants().unwrap_err();
    assert!(err.contains("moved character conservation mismatch"));
}
