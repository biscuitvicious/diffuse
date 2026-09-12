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
