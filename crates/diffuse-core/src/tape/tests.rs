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
    assert_eq!(runs[0].kind, RunKind::Common);
    assert_eq!(runs[0].text, "Hello ");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "brave ");
    assert_eq!(runs[2].kind, RunKind::Add);
    assert_eq!(runs[2].text, "wonderful ");
    assert_eq!(runs[3].kind, RunKind::Common);
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
    assert_eq!(runs[1].kind, RunKind::Common);
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

    // Check that inside the backtick delimiters, the path is NOT fragmented
    // into multiple pieces like `extension/pi/` + `./` + `scripts/` + `install`
    // + `check` + `.sh`. It must be: `Keep("`")`,
    // `Strike("extension/pi/scripts/install.sh")`,
    // `Fill("./scripts/check.sh")`, `Keep("`")`
    let old_path_run = runs
        .iter()
        .find(|r| r.text == "extension/pi/scripts/install.sh");
    assert!(
        old_path_run.is_some(),
        "Old path must be an intact Strike chunk, got: {:#?}",
        runs
    );
    assert_eq!(old_path_run.unwrap().kind, RunKind::Del);

    let new_path_run = runs.iter().find(|r| r.text == "./scripts/check.sh");
    assert!(
        new_path_run.is_some(),
        "New path must be an intact Fill chunk, got: {:#?}",
        runs
    );
    assert_eq!(new_path_run.unwrap().kind, RunKind::Add);
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

    // Intermediate directory `scripts` or extension `.sh` should NOT match
    // across two paths whose root directory and filename both differ.
    let scripts_common = runs
        .iter()
        .any(|r| r.kind == RunKind::Common && r.text.contains("scripts"));
    assert!(
        !scripts_common,
        "Intermediate scripts directory should not match across different paths: {:#?}",
        runs
    );
}

#[test]
fn test_theme_table_code_span_alignment() {
    let old = "| `--theme <NAME>` | Color theme (`magic` default, `default`, etc.) |";
    let new = "| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |";
    let tape = AlignmentTape::from_texts(old, new, None);
    let runs = tape.to_merged_runs();

    assert_eq!(runs.len(), 6);
    assert_eq!(runs[0].kind, RunKind::Common);
    assert_eq!(runs[0].text, "| `--theme <NAME>` | Color theme (");
    assert_eq!(runs[1].kind, RunKind::Del);
    assert_eq!(runs[1].text, "`magic` default, ");
    assert_eq!(runs[2].kind, RunKind::Common);
    assert_eq!(runs[2].text, "`default`, ");
    assert_eq!(runs[3].kind, RunKind::Del);
    assert_eq!(runs[3].text, "etc.");
    assert_eq!(runs[4].kind, RunKind::Add);
    assert_eq!(runs[4].text, "`dark`, `black`");
    assert_eq!(runs[5].kind, RunKind::Common);
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
            .any(|r| r.kind == RunKind::Common && r.text.contains("mstruct")),
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
            .any(|r| r.kind == RunKind::Common && r.text.starts_with('m')),
        "m must be common context, got runs: {:#?}",
        runs2
    );
    let del_run2 = runs2.iter().find(|r| r.kind == RunKind::Del).unwrap();
    assert_eq!(del_run2.text, "123");
    let add_run2 = runs2.iter().find(|r| r.kind == RunKind::Add).unwrap();
    assert_eq!(add_run2.text, "456");
}
