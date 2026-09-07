//! Unit tests for diff identity shift transformations.

use super::*;
use crate::{
    parse::{AddLine, CtxLine, DelLine, Hunk, LineNum, Row},
    syntax::registry::lang_profile,
};

fn hunk(rows: Vec<Row>) -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

#[test]
fn test_max_shift_down_rows() {
    let h = hunk(vec![
        Row::Del(DelLine::new(LineNum::new(10).unwrap(), "old line")),
        Row::Add(AddLine::new(LineNum::new(20).unwrap(), "line 1")),
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    }")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "")),
        Row::Add(AddLine::new(
            LineNum::new(23).unwrap(),
            "    pub fn next() {}",
        )),
        Row::Add(AddLine::new(LineNum::new(24).unwrap(), "    }")),
        Row::Ctx(CtxLine::new(
            LineNum::new(15).unwrap(),
            LineNum::new(25).unwrap(),
            "    }",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(16).unwrap(),
            LineNum::new(26).unwrap(),
            "",
        )),
    ]);

    let max_rows = max_shift_down_rows(&h, 2, 6);
    assert_eq!(
        max_rows, 2,
        "Should be able to shift 2 rows (closing brace and blank line)"
    );
}

#[test]
fn test_apply_shift_down_identity() {
    let mut h = hunk(vec![
        Row::Del(DelLine::new(LineNum::new(10).unwrap(), "old line")),
        Row::Add(AddLine::new(LineNum::new(20).unwrap(), "line 1")),
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    }")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "")),
        Row::Add(AddLine::new(
            LineNum::new(23).unwrap(),
            "    pub fn next() {}",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(12).unwrap(),
            LineNum::new(24).unwrap(),
            "    }",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(13).unwrap(),
            LineNum::new(25).unwrap(),
            "",
        )),
    ]);

    apply_shift_down_identity(&mut h, 2, 5, 2);

    assert_eq!(h.rows.len(), 7);
    // Released adds (2,3) became context.
    assert!(matches!(h.rows[2], Row::Ctx(_)), "released add becomes ctx");
    assert_eq!(h.rows[2].new_line(), Some(21));
    assert_eq!(h.rows[2].old_line(), Some(12));
    assert!(
        matches!(h.rows[3], Row::Ctx(_)),
        "released blank becomes ctx"
    );
    // Absorbed context (5,6) became adds.
    assert!(matches!(h.rows[5], Row::Add(_)), "absorbed ctx becomes add");
    assert_eq!(h.rows[5].new_line(), Some(24));
    assert!(
        matches!(h.rows[6], Row::Add(_)),
        "absorbed blank becomes add"
    );
    assert_eq!(h.rows[6].new_line(), Some(25));
}

#[test]
fn test_semantic_boundary_score() {
    let rs = lang_profile("rs");
    // Boundary before a function declaration
    let score_fn = semantic_boundary_score(Some("    }"), Some("    pub fn compute() {"), rs);
    // Boundary inside function body
    let score_mid =
        semantic_boundary_score(Some("        let x = 1;"), Some("        let y = 2;"), rs);

    assert!(
        score_fn > score_mid,
        "Boundary before function declaration should score higher than mid-expression boundary"
    );

    // When profile is None, no syntax bonuses are added
    let score_none = semantic_boundary_score(Some("    }"), Some("    pub fn compute() {"), None);
    assert_eq!(score_none, 0);
}

#[test]
fn test_inline_hunk_merging() {
    let mut f = DiffFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: crate::parse::FileStatus::Modified,
        no_content: false,
        new_line_count: 50,
        old_line_count: 50,
        old_content: (1..=50).map(|i| format!("line {i}")).collect(),
        new_content: (1..=50).map(|i| format!("line {i}")).collect(),
        raw_hunks: Vec::new(),
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        hunks: vec![
            Hunk {
                old_start: 10,
                old_count: 3,
                new_start: 10,
                new_count: 3,
                section: String::new(),
                rows: vec![
                    Row::Ctx(CtxLine::new(
                        LineNum::new(10).unwrap(),
                        LineNum::new(10).unwrap(),
                        "line 10",
                    )),
                    Row::Mod {
                        old_line: DelLine::new(LineNum::new(11).unwrap(), "line 11"),
                        new_line: AddLine::new(LineNum::new(11).unwrap(), "line 11 modified"),
                    },
                    Row::Ctx(CtxLine::new(
                        LineNum::new(12).unwrap(),
                        LineNum::new(12).unwrap(),
                        "line 12",
                    )),
                ],
                above_ctx: 1,
                below_ctx: 1,
            },
            Hunk {
                old_start: 15,
                old_count: 3,
                new_start: 15,
                new_count: 3,
                section: String::new(),
                rows: vec![
                    Row::Ctx(CtxLine::new(
                        LineNum::new(15).unwrap(),
                        LineNum::new(15).unwrap(),
                        "line 15",
                    )),
                    Row::Mod {
                        old_line: DelLine::new(LineNum::new(16).unwrap(), "line 16"),
                        new_line: AddLine::new(LineNum::new(16).unwrap(), "line 16 modified"),
                    },
                    Row::Ctx(CtxLine::new(
                        LineNum::new(17).unwrap(),
                        LineNum::new(17).unwrap(),
                        "line 17",
                    )),
                ],
                above_ctx: 1,
                below_ctx: 1,
            },
        ],
    };

    normalize_and_merge_hunks(&mut f, 6);

    assert_eq!(
        f.hunks.len(),
        1,
        "Two hunks separated by only 2 lines (gap < 2*budget) must merge into 1 contiguous hunk"
    );
    assert_eq!(f.hunks[0].old_start, 5); // expanded leading context (line 11 - 6 = line 5)
    assert_eq!(f.hunks[0].rows.len(), 18); // lines 5..=22 (6 above + mod11 + ctx12..15 + mod16 + 6 below)
}

#[test]
fn test_context_budget_normalization() {
    let mut f = DiffFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: crate::parse::FileStatus::Modified,
        no_content: false,
        new_line_count: 30,
        old_line_count: 30,
        old_content: (1..=30).map(|i| format!("line {i}")).collect(),
        new_content: (1..=30).map(|i| format!("line {i}")).collect(),
        raw_hunks: Vec::new(),
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        hunks: vec![Hunk {
            old_start: 10,
            old_count: 1,
            new_start: 10,
            new_count: 1,
            section: String::new(),
            rows: vec![Row::Mod {
                old_line: DelLine::new(LineNum::new(10).unwrap(), "line 10"),
                new_line: AddLine::new(LineNum::new(10).unwrap(), "line 10 mod"),
            }],
            above_ctx: 0,
            below_ctx: 0,
        }],
    };

    normalize_file_context_budget(&mut f, 6);

    let h = &f.hunks[0];
    assert_eq!(h.above_ctx, 6, "Must expand to 6 leading context lines");
    assert_eq!(h.below_ctx, 6, "Must expand to 6 trailing context lines");
    assert_eq!(h.rows.len(), 13); // 6 above + 1 mod + 6 below
    assert_eq!(h.old_start, 4); // line 10 - 6 = line 4
}

#[test]
fn test_semantic_slide_addition_to_function_header() {
    let h = hunk(vec![
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    }")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "")),
        Row::Add(AddLine::new(
            LineNum::new(23).unwrap(),
            "    pub fn new_func() {",
        )),
        Row::Add(AddLine::new(LineNum::new(24).unwrap(), "        step();")),
        Row::Ctx(CtxLine::new(
            LineNum::new(18).unwrap(),
            LineNum::new(25).unwrap(),
            "    }",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(19).unwrap(),
            LineNum::new(26).unwrap(),
            "",
        )),
    ]);

    let profile = lang_profile("rs");
    let shifted = pass_shift_hunk(&h, profile);

    // The leading "}" and blank are released into context; the matching
    // trailing context becomes the additions.
    assert!(
        matches!(shifted.rows[0], Row::Ctx(_)),
        "leading brace is context"
    );
    assert_eq!(shifted.rows[0].text(), "    }");
    assert!(matches!(shifted.rows[1], Row::Ctx(_)), "blank is context");
    assert_eq!(shifted.rows[2].text(), "    pub fn new_func() {");
    assert_eq!(shifted.rows[2].new_line(), Some(23));
    assert_eq!(shifted.rows[3].text(), "        step();");
    assert!(
        matches!(shifted.rows[4], Row::Add(_)),
        "absorbed brace is add"
    );
    assert_eq!(shifted.rows[4].text(), "    }");
    assert!(
        matches!(shifted.rows[5], Row::Add(_)),
        "absorbed blank is add"
    );
    assert_eq!(shifted.rows[5].text(), "");
}

#[test]
fn test_normalize_and_merge_hunks() {
    let mut f = DiffFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: crate::parse::FileStatus::Modified,
        no_content: false,
        new_line_count: 50,
        old_line_count: 50,
        old_content: (1..=50).map(|i| format!("line {i}")).collect(),
        new_content: (1..=50).map(|i| format!("line {i}")).collect(),
        raw_hunks: Vec::new(),
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        hunks: vec![
            Hunk {
                old_start: 10,
                old_count: 1,
                new_start: 10,
                new_count: 1,
                section: String::new(),
                rows: vec![Row::Mod {
                    old_line: DelLine::new(LineNum::new(10).unwrap(), "line 10"),
                    new_line: AddLine::new(LineNum::new(10).unwrap(), "line 10 mod"),
                }],
                above_ctx: 0,
                below_ctx: 0,
            },
            Hunk {
                old_start: 20,
                old_count: 1,
                new_start: 20,
                new_count: 1,
                section: String::new(),
                rows: vec![Row::Mod {
                    old_line: DelLine::new(LineNum::new(20).unwrap(), "line 20"),
                    new_line: AddLine::new(LineNum::new(20).unwrap(), "line 20 mod"),
                }],
                above_ctx: 0,
                below_ctx: 0,
            },
        ],
    };

    // Budget = 6. Gap between line 10 and line 20 is 9 lines (< 2 * 6 = 12).
    // Expanding context causes them to touch and merge into 1 unified hunk.
    normalize_and_merge_hunks(&mut f, 6);

    assert_eq!(
        f.hunks.len(),
        1,
        "Hunks with gap 9 < 2*budget=12 must merge into 1 hunk"
    );
    assert_eq!(
        f.hunks[0].old_start, 4,
        "Must expand leading context by 6 (line 10 - 6 = line 4)"
    );
    assert_eq!(f.hunks[0].above_ctx, 6);
    assert_eq!(f.hunks[0].below_ctx, 6);
}

#[test]
fn test_semantic_slide_json_object_closing_delimiter() {
    let h = hunk(vec![
        Row::Del(DelLine::new(
            LineNum::new(2028).unwrap(),
            "                  \"text\": \"        \"",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2029).unwrap(),
            "                  \"text\": \"    \"",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2030).unwrap(),
            "                },",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2031).unwrap(),
            "                {",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2032).unwrap(),
            "                  \"kind\": \"del\",",
        )),
        Row::Add(AddLine::new(
            LineNum::new(2033).unwrap(),
            "                  \"text\": \")\"",
        )),
        Row::Ctx(CtxLine::new(
            LineNum::new(2029).unwrap(),
            LineNum::new(2034).unwrap(),
            "                },",
        )),
    ]);

    let profile = lang_profile("json");
    let shifted = pass_shift_hunk(&h, profile);

    assert!(
        matches!(shifted.rows[2], Row::Ctx(_)),
        "line 2030 closing brace must be context, got {:?}",
        shifted.rows[2]
    );
    assert_eq!(shifted.rows[2].text(), "                },");
    assert_eq!(shifted.rows[2].new_line(), Some(2030));

    assert!(
        matches!(shifted.rows[6], Row::Add(_)),
        "line 2034 closing brace must be add, got {:?}",
        shifted.rows[6]
    );
    assert_eq!(shifted.rows[6].text(), "                },");
    assert_eq!(shifted.rows[6].new_line(), Some(2034));
}

#[test]
fn test_shift_down_recognizes_python_decorator_boundary() {
    let py = lang_profile("py");
    let h = hunk(vec![
        Row::Del(DelLine::new(LineNum::new(10).unwrap(), "old line")),
        Row::Add(AddLine::new(LineNum::new(20).unwrap(), "    pass")),
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    )")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "@dataclass")),
        Row::Add(AddLine::new(LineNum::new(23).unwrap(), "class Foo:")),
    ]);
    let shift_rows = calculate_needed_shift_down(&h, 1, 5, 2, py);
    assert_eq!(
        shift_rows, 1,
        "Should shift down closing delimiter before decorator boundary"
    );
}
