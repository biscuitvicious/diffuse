use crate::{
    lcs::word_spans,
    merge::{RunKind, merged_runs_from_texts},
    tape::{AlignmentTape, TapeChunk},
};

#[test]
fn test_alignment_tape_basic_invariants() {
    let mut tape = AlignmentTape::default();
    tape.push(TapeChunk::Keep("Hello "));
    tape.push(TapeChunk::Strike("brave "));
    tape.push(TapeChunk::Fill("wonderful "));
    tape.push(TapeChunk::Keep("world!"));

    assert_eq!(tape.project_old(), "Hello brave world!");
    assert_eq!(tape.project_new(), "Hello wonderful world!");

    let runs = tape.to_merged_runs();
    assert_eq!(runs.len(), 4);
    assert_eq!(runs[0].kind, RunKind::Ctx);
    assert_eq!(runs[0].text, "Hello ");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "brave ");
    assert_eq!(runs[2].kind, RunKind::Add);
    assert_eq!(runs[2].text, "wonderful ");
    assert_eq!(runs[3].kind, RunKind::Ctx);
    assert_eq!(runs[3].text, "world!");

    let spans = tape.to_spans();
    assert_eq!(spans.del, vec![(6, 12)]);
    assert_eq!(spans.add, vec![(6, 16)]);
}

#[test]
fn test_diffuse_theme_magic_to_default() {
    let old = "    let rendered = render_pipeline_output(&out, &DiffuseTheme::MAGIC, true);";
    let new = "    let rendered = render_pipeline_output(&out, &DiffuseTheme::DEFAULT, true);";
    let tape = AlignmentTape::from_texts(old, new, None);
    eprintln!("THEME TAPE: {:#?}", tape);
    let runs = tape.to_merged_runs();
    eprintln!("THEME RUNS: {:#?}", runs);
    let del_run = runs.iter().find(|r| r.kind == RunKind::Del).unwrap();
    assert_eq!(del_run.text, "MAGIC");
}

#[test]
fn test_alignment_tape_monoidal_composition() {
    let mut tape1 = AlignmentTape::default();
    tape1.push(TapeChunk::Keep("let "));
    tape1.push(TapeChunk::Strike("x"));
    tape1.push(TapeChunk::Fill("y"));

    let mut tape2 = AlignmentTape::default();
    tape2.push(TapeChunk::Keep(" = "));
    tape2.push(TapeChunk::Strike("10"));
    tape2.push(TapeChunk::Fill("20"));
    tape2.push(TapeChunk::Keep(";"));

    tape1.compose(tape2);

    assert_eq!(tape1.project_old(), "let x = 10;");
    assert_eq!(tape1.project_new(), "let y = 20;");
}

#[test]
fn test_alignment_tape_from_texts_agents_md() {
    let old_line = "The workspace also holds the engine's first consumer: **`extension/pi/`** —";
    let new_line = "The workspace also holds **`extension/pi/`** — the pi review extension";

    let tape = AlignmentTape::from_texts(old_line, new_line, None);

    assert_eq!(tape.project_old(), old_line);
    assert_eq!(tape.project_new(), new_line);

    let spans = tape.to_spans();
    let ws = word_spans(old_line, new_line);
    assert_eq!(spans.del, ws.del);
    assert_eq!(spans.add, ws.add);

    let runs = tape.to_merged_runs();
    let legacy_runs = merged_runs_from_texts(old_line, new_line, &ws);
    assert_eq!(runs, legacy_runs);
}

#[test]
fn test_alignment_tape_whole_line_fallback() {
    let old_line = "completely old content";
    let new_line = "entirely different line";

    let tape = AlignmentTape::from_texts(old_line, new_line, None);
    assert_eq!(tape.project_old(), old_line);
    assert_eq!(tape.project_new(), new_line);

    let spans = tape.to_spans();
    let ws = word_spans(old_line, new_line);
    assert_eq!(spans.del, ws.del);
    assert_eq!(spans.add, ws.add);
}

#[test]
fn test_alignment_tape_whitespace_minimum() {
    let old_line = "    return true;";
    let new_line = "        return true;";

    let tape = AlignmentTape::from_texts(old_line, new_line, None);
    assert_eq!(tape.project_old(), old_line);
    assert_eq!(tape.project_new(), new_line);

    let runs = tape.to_merged_runs();
    // 4 spaces Add + (4 spaces + "return true;") Common
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].kind, RunKind::Add);
    assert_eq!(runs[0].text, "    ");
    assert_eq!(runs[1].kind, RunKind::Ctx);
    assert_eq!(runs[1].text, "    return true;");
}

#[test]
fn test_alignment_tape_reversibility_property() {
    let test_pairs = [
        ("", ""),
        ("a", ""),
        ("", "b"),
        ("same", "same"),
        ("foo bar baz", "foo qux baz"),
        ("let x = 1;", "let mut x = 1;"),
        (
            "fn calculate(a: i32) -> i32 {",
            "pub fn calculate(a: i32, b: i32) -> Result<i32> {",
        ),
        ("    if k < floor {", "    if let Some(floor) = floor {"),
        (
            "/// Documentation comment here.",
            "/// Updated documentation comment here!",
        ),
        ("東京特許許可局局長", "東京特許許可局第1号局長"),
        ("emoji 🚀 rocket launch", "emoji 🛸 ufo sighting"),
        (
            "const arr = [1, 2, 3, 4, 5];",
            "const arr = [1, 2, 99, 4, 5, 6];",
        ),
        (
            "<div><span>Nested HTML</span></div>",
            "<div class=\"main\"><span>Nested HTML updated</span></div>",
        ),
    ];

    for (old_text, new_text) in test_pairs {
        let tape = AlignmentTape::from_texts(old_text, new_text, None);
        assert_eq!(
            tape.project_old(),
            old_text,
            "tape.project_old() failed for ({:?}, {:?})",
            old_text,
            new_text
        );
        assert_eq!(
            tape.project_new(),
            new_text,
            "tape.project_new() failed for ({:?}, {:?})",
            old_text,
            new_text
        );
    }
}

#[test]
fn test_alignment_tape_backtick_path_not_fragmented() {
    let old = "5. **Install the extension** — `extension/pi/scripts/install.sh`";
    let new = "5. **Install (dev by default)** — `./scripts/check.sh` already installs";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    let tuples: Vec<(RunKind, &str)> = runs.iter().map(|r| (r.kind, r.text)).collect();

    // Generic alignment aligns identical subwords and punctuation (`scripts/` and
    // `.sh`) without path-specific fragmentation bans:
    assert_eq!(
        tuples,
        vec![
            (RunKind::Ctx, "5. **Install "),
            (RunKind::Del, "the extension"),
            (RunKind::Add, "(dev by default)"),
            (RunKind::Ctx, "** — `"),
            (RunKind::Del, "extension/pi/"),
            (RunKind::Add, "./"),
            (RunKind::Ctx, "scripts/"),
            (RunKind::Del, "install"),
            (RunKind::Add, "check"),
            (RunKind::Ctx, ".sh`"),
            (RunKind::Add, " already installs"),
        ]
    );
}

#[test]
fn test_alignment_tape_backtick_clean_single_edits_preserved() {
    // Flag insertion inside backticks:
    let old1 = "- `cargo clippy -- -D warnings`";
    let new1 = "- `cargo +nightly clippy -- -D warnings`";
    let tape1 = AlignmentTape::from_texts(old1, new1, None);
    let runs1 = tape1.to_merged_runs();
    let add_run = runs1.iter().find(|r| r.kind == RunKind::Add);
    assert!(add_run.is_some());
    assert_eq!(add_run.unwrap().text, "+nightly ");

    // Prefix deletion inside backticks:
    let old2 = "/// `resolved_layers` is populated";
    let new2 = "/// `layers` is populated";
    let tape2 = AlignmentTape::from_texts(old2, new2, None);
    let runs2 = tape2.to_merged_runs();
    let del_run = runs2.iter().find(|r| r.kind == RunKind::Del);
    assert!(del_run.is_some());
    assert_eq!(del_run.unwrap().text, "resolved_");
}

#[test]
fn test_paths_with_different_roots_and_basenames_not_fragmented() {
    let old = "Run extension/pi/scripts/install.sh now";
    let new = "Run ./scripts/check.sh now";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    let tuples: Vec<(RunKind, &str)> = runs.iter().map(|r| (r.kind, r.text)).collect();

    // Generic alignment aligns identical subwords and punctuation (`scripts/` and
    // `.sh`) without path-specific heuristics:
    assert_eq!(
        tuples,
        vec![
            (RunKind::Ctx, "Run "),
            (RunKind::Del, "extension/pi/"),
            (RunKind::Add, "./"),
            (RunKind::Ctx, "scripts/"),
            (RunKind::Del, "install"),
            (RunKind::Add, "check"),
            (RunKind::Ctx, ".sh now"),
        ]
    );
}

#[test]
fn test_theme_table_code_span_alignment() {
    let old = "| `--theme <NAME>` | Color theme (`magic` default, `default`, etc.) |";
    let new = "| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();

    assert_eq!(runs.len(), 6);
    assert_eq!(runs[0].kind, RunKind::Ctx);
    assert_eq!(runs[0].text, "| `--theme <NAME>` | Color theme (");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "`magic` default, ");
    assert_eq!(runs[2].kind, RunKind::Ctx);
    assert_eq!(runs[2].text, "`default`, ");
    assert_eq!(runs[3].kind, RunKind::Del);
    assert_eq!(runs[3].text, "etc.");
    assert_eq!(runs[4].kind, RunKind::Add);
    assert_eq!(runs[4].text, "`dark`, `black`");
    assert_eq!(runs[5].kind, RunKind::Ctx);
    assert_eq!(runs[5].text, ") |");
}

#[test]
fn test_theme_line_259() {
    let old = "    /// Resolve a theme by name (`\"magic\"`, `\"deep_black\"`, `\"legacy\"`).";
    let new = "    /// Resolve a theme by name (`\"default\"`, `\"dark\"`, `\"black\"`).";
    let tape = AlignmentTape::from_texts(old, new, None);
    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);

    let runs = tape.to_merged_runs();
    let del_texts: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    let add_texts: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Add)
        .map(|r| r.text)
        .collect();
    assert_eq!(del_texts, vec!["magic", "deep_black", "legacy"]);
    assert_eq!(add_texts, vec!["default", "dark", "black"]);
}

#[test]
fn test_ansi_and_digit_letter_boundary_subword_alignment() {
    // 141mstruct vs 147mstruct: mstruct must be Common
    let old = "    assert!(md.contains(\"\\x1b[38;5;141mstruct\\x1b[39m\"));";
    let new = "    assert!(md.contains(\"\\x1b[38;5;147mstruct\\x1b[39m\"));";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();

    assert!(
        runs.iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.contains("mstruct")),
        "mstruct must be common context, got runs: {:#?}",
        runs
    );
    let del_run = runs.iter().find(|r| r.kind == RunKind::Del).unwrap();
    assert_eq!(del_run.text, "141");
    let add_run = runs.iter().find(|r| r.kind == RunKind::Add).unwrap();
    assert_eq!(add_run.text, "147");

    // 123m vs 456m: m must be Common
    let old2 = "let val = 123m;";
    let new2 = "let val = 456m;";
    let tape2 = AlignmentTape::from_texts(old2, new2, None);
    let runs2 = tape2.to_merged_runs();
    assert!(
        runs2
            .iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.starts_with('m')),
        "m must be common context, got runs: {:#?}",
        runs2
    );
    let del_run2 = runs2.iter().find(|r| r.kind == RunKind::Del).unwrap();
    assert_eq!(del_run2.text, "123");
    let add_run2 = runs2.iter().find(|r| r.kind == RunKind::Add).unwrap();
    assert_eq!(add_run2.text, "456");

    // 173mConfig vs Config: Config must be Common
    let old3 = "    assert!(md.contains(\"\\x1b[38;5;173mConfig\\x1b[39m\"));";
    let new3 = "    assert!(md.contains(\"Config\"));";
    let tape3 = AlignmentTape::from_texts(old3, new3, None);
    let runs3 = tape3.to_merged_runs();
    assert!(
        runs3
            .iter()
            .any(|r| r.kind == RunKind::Ctx && r.text == "Config"),
        "Config must be common context, got runs: {:#?}",
        runs3
    );
}

#[test]
fn test_subword_cohesion_identifier_not_split_across_del() {
    let old = "import { existsSync, readFileSync } from \"node:fs\";";
    let new = "import { existsSync } from \"node:fs\";";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    assert!(
        runs.iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.contains("existsSync")),
        "existsSync must remain intact as common context, got runs: {:#?}",
        runs
    );
}

#[test]
fn test_unindented_closing_delimiter_recognized_as_common() {
    let old = "                },";
    let new = "            },";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    assert!(
        runs.iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.ends_with("},")),
        "bracket '}},' must be common context, got runs: {:#?}",
        runs
    );
    assert!(
        !runs
            .iter()
            .any(|r| (r.kind == RunKind::Del || r.kind == RunKind::Add) && r.text.contains('}')),
        "bracket '}}' must not be struck or filled, got runs: {:#?}",
        runs
    );

    // Also verify closing brace alone
    let old_brace = "                }";
    let new_brace = "            }";
    let tape_brace = AlignmentTape::from_texts(old_brace, new_brace, None);
    let runs_brace = tape_brace.to_merged_runs();
    assert!(
        runs_brace
            .iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.ends_with('}')),
        "brace '}}' must be common context, got runs: {:#?}",
        runs_brace
    );

    // Also verify closing paren with indentation increase
    let old_paren = "    )";
    let new_paren = "        )";
    let tape_paren = AlignmentTape::from_texts(old_paren, new_paren, None);
    let runs_paren = tape_paren.to_merged_runs();
    assert!(
        runs_paren
            .iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.ends_with(')')),
        "paren ')' must be common context, got runs: {:#?}",
        runs_paren
    );
}

#[test]
fn test_line_77_style_methods_no_fragmentation() {
    let old = "    assert_eq!(s_add.tint_style(), Style::new().bg(Color::Fixed(22)));";
    let new = "    assert_eq!(add_style.tint_bg, theme.palette.add_tint_bg);";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    let run_tuples: Vec<(RunKind, &str)> = runs.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        run_tuples,
        vec![
            (RunKind::Ctx, "    assert_eq!("),
            (RunKind::Del, "s_add"),
            (RunKind::Add, "add_style"),
            (RunKind::Ctx, ".tint_"),
            (RunKind::Del, "style()"),
            (RunKind::Add, "bg"),
            (RunKind::Ctx, ", "),
            (RunKind::Del, "Style::new().bg(Color::Fixed(22))"),
            (RunKind::Add, "theme.palette.add_tint_bg"),
            (RunKind::Ctx, ");"),
        ]
    );
}

#[test]
fn test_line_200_target_vis_no_chimera() {
    let old = "                        let target_vis = ((row_in_vh as f64 / (vh - 1) as f64) * max_scroll as f64)";
    let new = "                    let target_vis = Self::scrollbar_scroll_for_row(vh, max_scroll, row_relative);";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();
    let run_tuples: Vec<(RunKind, &str)> = runs.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        run_tuples,
        vec![
            (RunKind::Del, "    "),
            (RunKind::Ctx, "                    let target_vis = "),
            (RunKind::Del, "((row_in_vh as f64 / "),
            (RunKind::Add, "Self::scrollbar_scroll_for_row"),
            (RunKind::Ctx, "(vh"),
            (RunKind::Del, " - 1) as f64) *"),
            (RunKind::Add, ","),
            (RunKind::Ctx, " max_scroll"),
            (RunKind::Del, " as f64"),
            (RunKind::Add, ", row_relative"),
            (RunKind::Ctx, ")"),
            (RunKind::Add, ";"),
        ]
    );
}

#[test]
fn test_opposite_boundary_subword_inversion_not_matched() {
    // Suffix vs Prefix: "s_add" vs "add_style" -> "add" is not matched across
    // inverted boundaries
    let tape1 = AlignmentTape::from_texts("s_add", "add_style", None);
    let runs1 = tape1.to_merged_runs();
    let tuples1: Vec<(RunKind, &str)> = runs1.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        tuples1,
        vec![(RunKind::Del, "s_add"), (RunKind::Add, "add_style")]
    );

    // Prefix vs Suffix: "add_style" vs "s_add"
    let tape2 = AlignmentTape::from_texts("add_style", "s_add", None);
    let runs2 = tape2.to_merged_runs();
    let tuples2: Vec<(RunKind, &str)> = runs2.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        tuples2,
        vec![(RunKind::Del, "add_style"), (RunKind::Add, "s_add")]
    );

    // Prefix vs Prefix: "row_in_vh" vs "row_relative" -> common prefix "row_"
    // preserved
    let tape3 = AlignmentTape::from_texts("row_in_vh", "row_relative", None);
    let runs3 = tape3.to_merged_runs();
    let tuples3: Vec<(RunKind, &str)> = runs3.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        tuples3,
        vec![
            (RunKind::Ctx, "row_"),
            (RunKind::Del, "in_vh"),
            (RunKind::Add, "relative"),
        ]
    );

    // Suffix vs Suffix: "old_row" vs "new_row" -> common suffix "_row"
    // preserved
    let tape4 = AlignmentTape::from_texts("old_row", "new_row", None);
    let runs4 = tape4.to_merged_runs();
    let tuples4: Vec<(RunKind, &str)> = runs4.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        tuples4,
        vec![
            (RunKind::Del, "old"),
            (RunKind::Add, "new"),
            (RunKind::Ctx, "_row"),
        ]
    );

    // Standalone vs Prefix: "row" vs "row_relative" -> common "row" preserved
    let tape5 = AlignmentTape::from_texts("row", "row_relative", None);
    let runs5 = tape5.to_merged_runs();
    let tuples5: Vec<(RunKind, &str)> = runs5.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(
        tuples5,
        vec![(RunKind::Ctx, "row"), (RunKind::Add, "_relative")]
    );

    // Standalone vs Suffix: "row" vs "old_row" -> common "row" preserved
    let tape6 = AlignmentTape::from_texts("row", "old_row", None);
    let runs6 = tape6.to_merged_runs();
    let tuples6: Vec<(RunKind, &str)> = runs6.iter().map(|r| (r.kind, r.text)).collect();
    assert_eq!(tuples6, vec![(RunKind::Add, "old_"), (RunKind::Ctx, "row")]);
}

#[test]
fn test_common_whitespace_does_not_overpower_dissimilar_lines() {
    let old = "                                            })";
    let new = "                                            wrapped_glyph_hit(&txt, pat, seg_off, col, cw)";
    let tape = AlignmentTape::from_texts(old, new, None);
    assert_eq!(
        tape.chunks(),
        &[TapeChunk::Strike(old), TapeChunk::Fill(new)]
    );
}

#[test]
fn test_prefix_suffix_peeling_equivalence() {
    let old = "    let result = execute_computation(arg1, old_param, arg3);";
    let new = "    let result = execute_computation(arg1, new_param, arg3);";
    let tape = AlignmentTape::from_texts(old, new, None);

    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);
    assert_eq!(tape.reverse().reverse(), tape);

    let runs = tape.to_merged_runs();
    let old_extracted: String = runs
        .iter()
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    let new_extracted: String = runs
        .iter()
        .filter(|r| r.kind == RunKind::Ctx || r.kind == RunKind::Add)
        .map(|r| r.text)
        .collect();
    assert_eq!(old_extracted, old);
    assert_eq!(new_extracted, new);

    // Trailing change (like stress test)
    let old_tail = "    let value = 42;";
    let new_tail = "    let value = 42;~";
    let tape_tail = AlignmentTape::from_texts(old_tail, new_tail, None);
    assert_eq!(tape_tail.project_old(), old_tail);
    assert_eq!(tape_tail.project_new(), new_tail);
    assert_eq!(tape_tail.reverse().reverse(), tape_tail);

    // Leading change (common suffix)
    let old_head = "prev_calc(42);";
    let new_head = "next_calc(42);";
    let tape_head = AlignmentTape::from_texts(old_head, new_head, None);
    assert_eq!(tape_head.project_old(), old_head);
    assert_eq!(tape_head.project_new(), new_head);
    assert_eq!(tape_head.reverse().reverse(), tape_head);

    // Trailing whitespace modification
    let old_ws = "let x = 1;   ";
    let new_ws = "let x = 1;   // comment";
    let tape_ws = AlignmentTape::from_texts(old_ws, new_ws, None);
    assert_eq!(tape_ws.project_old(), old_ws);
    assert_eq!(tape_ws.project_new(), new_ws);
    assert_eq!(tape_ws.reverse().reverse(), tape_ws);
}

#[test]
fn test_method_call_parens_insertion_before_trailing_comma() {
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let old = "                            ar.del_text, er.del_text";
    let new = "                            ar.del_text(),";
    let tape = AlignmentTape::from_texts(old, new, profile);
    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);
    let runs = tape.to_merged_runs();
    assert_eq!(runs.len(), 4);
    assert_eq!(runs[0].kind, RunKind::Ctx);
    assert_eq!(runs[0].text, "                            ar.del_text");
    assert_eq!(runs[1].kind, RunKind::Add);
    assert_eq!(runs[1].text, "()");
    assert_eq!(runs[2].kind, RunKind::Ctx);
    assert_eq!(runs[2].text, ",");
    assert_eq!(runs[3].kind, RunKind::Del);
    assert_eq!(runs[3].text, " er.del_text");
}

#[test]
fn test_let_pairs_let_matched_tape() {
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let old = "    let pairs = crate::lcs::lcs_pairs_ids(&ot.ids, nt_ids, ws, alnum);";
    let new = "    let matched =";
    let tape = AlignmentTape::from_texts(old, new, profile);
    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);
    let runs = tape.to_merged_runs();
    assert_eq!(runs.len(), 5);
    assert_eq!(runs[0].kind, RunKind::Ctx);
    assert_eq!(runs[0].text, "    let ");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "pairs");
    assert_eq!(runs[2].kind, RunKind::Add);
    assert_eq!(runs[2].text, "matched");
    assert_eq!(runs[3].kind, RunKind::Ctx);
    assert_eq!(runs[3].text, " =");
    assert_eq!(runs[4].kind, RunKind::Del);
    assert_eq!(
        runs[4].text,
        " crate::lcs::lcs_pairs_ids(&ot.ids, nt_ids, ws, alnum);"
    );
}

#[test]
fn test_tuple_call_to_struct_literal_not_interleaved() {
    let profile = diffuse_syntax::registry::lang_profile("rs");

    // Case 1: tuple variant constructor call with literal 3
    let old = "            state.execute(TuiCommand::MouseScrollDown(3));";
    let new = "            state.execute(TuiCommand::MouseScrollDown { n: 3, row, col });";

    let tape = AlignmentTape::from_texts(old, new, profile);
    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);
    let runs = tape.to_merged_runs();

    let del_runs: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    let add_runs: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Add)
        .map(|r| r.text)
        .collect();

    assert_eq!(del_runs, vec!["(3)"]);
    assert_eq!(add_runs, vec![" { n: 3, row, col }"]);

    // Case 2: tuple pattern match with identifier `n`
    let old2 = "            TuiCommand::MouseScrollDown(n) => {";
    let new2 = "            TuiCommand::MouseScrollDown { n, row, col } => {";

    let tape2 = AlignmentTape::from_texts(old2, new2, profile);
    assert_eq!(tape2.project_old(), old2);
    assert_eq!(tape2.project_new(), new2);
    let runs2 = tape2.to_merged_runs();

    let del_runs2: Vec<&str> = runs2
        .iter()
        .filter(|r| r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    let add_runs2: Vec<&str> = runs2
        .iter()
        .filter(|r| r.kind == RunKind::Add)
        .map(|r| r.text)
        .collect();

    assert_eq!(del_runs2, vec!["(n)"]);
    assert_eq!(add_runs2, vec![" { n, row, col }"]);
}

#[test]
fn test_tape_uncrosses_interleaved_delimiter_chunks() {
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let old = "    let val = compute(foo[idx]);";
    let new = "    let val = compute(foo(idx));";

    let tape = AlignmentTape::from_texts(old, new, profile);
    assert_eq!(tape.project_old(), old);
    assert_eq!(tape.project_new(), new);
    let runs = tape.to_merged_runs();

    // `[idx]` to `(idx)` must be replaced atomically as `-[idx]+(idx)`
    // without interleaving `[` and `(`.
    let del_runs: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Del)
        .map(|r| r.text)
        .collect();
    let add_runs: Vec<&str> = runs
        .iter()
        .filter(|r| r.kind == RunKind::Add)
        .map(|r| r.text)
        .collect();

    assert_eq!(del_runs, vec!["[idx]"]);
    assert_eq!(add_runs, vec!["(idx)"]);
}
