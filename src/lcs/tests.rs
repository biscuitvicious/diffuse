//! Unit tests for tokenization, LCS, and word-level diff spans.

use super::*;
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
    let ws = word_spans("a => b", "a -> b");
    assert_eq!(
        char_slice("a => b", ws.del[0].0, ws.del[0].1),
        "=>",
        "operator token span"
    );
}

// Backticks split from identifiers (markdown code markers): with
// `` `resolved`` + `layers`` as tokens the shared `layers` can never match
// the add's `` `layers` `` and the WHOLE `` `resolved_layers` `` becomes
// one removal span. Split, the span is just `resolved_` and the mod reads
// `resolved_` overstruck inside the intact code markers.
#[test]
fn backticks_split_from_identifiers() {
    let del = "/// `resolved_layers` is populated during [`prepare`](Filesystem::prepare).";
    let add = "/// `layers` is populated during [`prepare`](Filesystem::prepare).";
    let ws = word_spans(del, add);
    assert_eq!(ws.del.len(), 1, "one removal span");
    assert_eq!(
        char_slice(del, ws.del[0].0, ws.del[0].1),
        "resolved_",
        "the span is just resolved_"
    );
    assert_eq!(ws.add.len(), 0, "nothing is added");
    // A changed identifier inside the markers keeps the markers common.
    let w2 = word_spans("`foo` and `bar`", "`foo` and `baz`");
    assert_eq!(w2.del.len(), 1, "one del");
    assert_eq!(w2.add.len(), 1, "one add");
    assert_eq!(w2.add[0].1 - w2.add[0].0, 3, "only bar→baz");
}

// Delimiters are single-char tokens (parens stay common): `…anchors).` →
// `…anchors),` shares the `)` — only the period becomes the comma. If
// `).`/`),` glued into single tokens the LCS could never match the shared
// paren and the whole `).` would be struck.
#[test]
fn delimiters_are_single_char_tokens() {
    let del = "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors).";
    let add = "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors),";
    let ws = word_spans(del, add);
    assert_eq!(ws.del.len(), 1, "one del span");
    assert_eq!(
        char_slice(del, ws.del[0].0, ws.del[0].1),
        ".",
        "del span is just the period"
    );
    assert_eq!(ws.add.len(), 1, "one add span");
    assert_eq!(
        char_slice(add, ws.add[0].0, ws.add[0].1),
        ",",
        "add span is just the comma"
    );
    // The range operator: `..` and `..=` are operator-class tokens (glued
    // by design — operator changes are one span), so the `..` → `..=`
    // change spans the whole operator + the added bound.
    let rg = word_spans("x[1..]", "x[1..=2]");
    assert_eq!(rg.add.len(), 1, "one add span");
    assert_eq!(
        char_slice("x[1..=2]", rg.add[0].0, rg.add[0].1),
        "..=2",
        "operator span covers the changed operator"
    );
}

// Whole-line span for mostly-rewritten lines (no spurious gaps): an
// append-only change spans just the appended text; a full rewrite is one
// whole-line span; a genuine one-word change stays partial.
#[test]
fn whole_line_span_for_mostly_rewritten_lines() {
    let append = word_spans(
        "same shared prefix content",
        "same shared prefix content plus a new tail here",
    );
    assert_eq!(append.add.len(), 1, "append-only span");
    assert!(append.add[0].0 > 0, "the prefix stays common");
    let rewrite = word_spans(
        "**TOCTOU Anchor Protection**: Every `AnchoredPath` opens an `O_PATH` file descriptor.",
        "**Segment Rule (all precedence)**: each upper captures the contiguous run of read-only layers.",
    );
    assert_eq!(rewrite.add.len(), 1, "rewritten line is one full span");
    let full = "**Segment Rule (all precedence)**: each upper captures the contiguous run of read-only layers.";
    assert_eq!(rewrite.add[0].0, 0);
    assert_eq!(rewrite.add[0].1, full.chars().count(), "the whole line");
    let partial = word_spans("fn old_name(x: u32)", "fn new_name(x: u64)");
    assert!(partial.add.len() >= 2, "partial change stays partial");
    assert_ne!(partial.add[0].0, 0, "the common prefix is not struck");
}

// Unicode whitespace subwords (character class, not individual picks): a
// nbsp (U+00A0, in White_Space) vs a tab, a zero-width space (U+200B) vs a
// space, and an ideographic space (U+3000) vs 3 spaces — the whitespace
// change must NOT swallow the identifier: both a del and an add survive.
#[test]
fn unicode_whitespace_subwords() {
    let cases: [(&str, &str); 3] = [
        ("\u{00a0}self.x", "\tself.x"),
        ("\u{200b}self.x", " self.x"),
        ("\u{3000}self.x", "   self.x"),
    ];
    for (del, add) in cases {
        let ws = word_spans(del, add);
        assert!(
            !ws.del.is_empty() && !ws.add.is_empty(),
            "unicode whitespace subwords: {del:?} vs {add:?}"
        );
    }
}

#[test]
fn test_lcs_tie_break_minimizes_span_fragmentation() {
    let old_line = "if k < floor {";
    let new_line = "if let Some(floor) = floor";
    let ot = tokenize_line(old_line);
    let nt = tokenize_line(new_line);
    let pairs = lcs_pairs(&ot, &nt);
    assert_eq!(
        pairs.get(&6),
        Some(&11),
        "old floor pairs with RHS floor at index 11"
    );
}

#[test]
fn test_patience_unique_token_anchors() {
    let old_line = "fn handle_request(&self, req: Request) -> Response";
    let new_line = "async fn handle_request(&self, cx: Context, req: Request) -> Result<Response>";
    let ot = tokenize_line(old_line);
    let nt = tokenize_line(new_line);
    let anchors = unique_token_anchors(&ot, &nt);
    assert!(!anchors.is_empty(), "patience anchors found");
    let _pairs = lcs_pairs(&ot, &nt);
    let spans = word_spans(old_line, new_line);
    assert!(spans.add.len() >= 2, "adds isolated by patience anchors");
}

#[test]
fn test_add_span_covers_let_some_floor_equals() {
    let old_line = "                if k < floor {";
    let new_line = "                if let Some(floor) = floor";
    let spans = word_spans(old_line, new_line);
    assert_eq!(
        spans.add,
        vec![(19, 37)],
        "addition span covers 'let Some(floor) = ' including trailing space"
    );
    assert_eq!(
        spans.del,
        vec![(19, 23), (28, 30)],
        "del spans cover 'k < ' (including trailing space) and ' {{'"
    );
}

#[test]
fn test_recursive_patience_anchoring() {
    let old_text = "fn transform(user: User, profile: Profile, db: Database) -> Result<Output>";
    let new_text = "pub async fn transform(ctx: Context, user: User, metadata: Metadata, profile: Profile, db: Database) -> Result<Output, Error>";
    let ot = tokenize_line(old_text);
    let nt = tokenize_line(new_text);
    let pairs = lcs_pairs(&ot, &nt);
    assert!(!pairs.is_empty());
    let ws = word_spans(old_text, new_text);
    assert!(
        ws.add.len() >= 3,
        "isolated adds across multiple sub-intervals"
    );
}
