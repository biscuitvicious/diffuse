
use super::*;

// Port of the word-level diff test block from test/review.test.ts.
#[test]
fn word_diff_highlighting() {
    // Underscores and operators are separate tokens, so
    // identifier/operator edits highlight the changed part only.
    let ws = word_spans(
        "fn old_name(x: u32) -> u32 {",
        "fn new_name(x: u64) -> u64 {",
    );
    assert_eq!(ws.add.len(), 3, "word diff add spans");
    assert_eq!(ws.del.len(), 3, "word diff del spans");
    let add_text = "fn new_name(x: u64) -> u64 {";
    assert_eq!(
        char_slice(add_text, ws.add[0].0, ws.add[0].1),
        "new",
        "add span 0"
    );
    assert_eq!(
        char_slice(add_text, ws.add[1].0, ws.add[1].1),
        "u64",
        "add span 1"
    );
    assert_eq!(
        char_slice(add_text, ws.add[2].0, ws.add[2].1),
        "u64",
        "add span 2"
    );
}

#[test]
fn identical_lines_have_no_spans() {
    let same = word_spans("abc def", "abc def");
    assert_eq!(same.add.len(), 0);
    assert_eq!(same.del.len(), 0);
}

#[test]
fn one_changed_word_each_side() {
    let word_diff = word_spans("hello world", "goodbye world");
    assert_eq!(word_diff.add.len(), 1);
    assert_eq!(word_diff.del.len(), 1);
}

#[test]
fn operator_token_granularity() {
    let op = word_spans("a -> b", "a => b");
    let op_text = "a => b";
    assert_eq!(
        char_slice(op_text, op.add[0].0, op.add[0].1),
        "=>",
        "operator token span"
    );
}
