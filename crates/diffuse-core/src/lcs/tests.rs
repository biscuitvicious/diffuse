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
    let ot: Vec<&str> = tokenize_line(old_line).collect();
    let nt: Vec<&str> = tokenize_line(new_line).collect();
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
    let ot: Vec<&str> = tokenize_line(old_line).collect();
    let nt: Vec<&str> = tokenize_line(new_line).collect();
    let anchors = unique_token_anchors(&ot, &nt, None);
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
    let ot: Vec<&str> = tokenize_line(old_text).collect();
    let nt: Vec<&str> = tokenize_line(new_text).collect();
    let pairs = lcs_pairs(&ot, &nt);
    assert!(!pairs.is_empty());
    let ws = word_spans(old_text, new_text);
    assert!(
        ws.add.len() >= 3,
        "isolated adds across multiple sub-intervals"
    );
}

#[test]
fn test_semantic_delimiter_atomicity_and_literals() {
    // 1. Lifetime updates: &'a str -> &'static str
    let old1 = "fn get_name(&self) -> &'a str;";
    let new1 = "fn get_name(&self) -> &'static str;";
    let lifetime_ws = word_spans(old1, new1);
    assert_eq!(lifetime_ws.del.len(), 1);
    assert_eq!(lifetime_ws.add.len(), 1);
    assert_eq!(
        char_slice(new1, lifetime_ws.add[0].0, lifetime_ws.add[0].1),
        "'static"
    );

    // 2. String literal content update: "Alice" -> "Bob"
    let old2 = "let name = \"Alice\";";
    let new2 = "let name = \"Bob\";";
    let str_ws = word_spans(old2, new2);
    assert_eq!(str_ws.del.len(), 1);
    assert_eq!(str_ws.add.len(), 1);
    assert_eq!(char_slice(new2, str_ws.add[0].0, str_ws.add[0].1), "Bob");

    // 3. String literal to macro expansion: "Alice" -> format!("{user}")
    let old3 = "let name = \"Alice\";";
    let new3 = "let name = format!(\"{user}\");";
    let macro_ws = word_spans(old3, new3);
    assert_eq!(macro_ws.del.len(), 1);
    assert_eq!(
        char_slice(old3, macro_ws.del[0].0, macro_ws.del[0].1),
        "Alice"
    );
}

#[test]
fn test_tokenize_line_parity_with_regex() {
    fn tokenize_regex(text: &str) -> Vec<&str> {
        let re = regex::Regex::new(
            r#"([\p{White_Space}\u200b]+|[<>!=+\-*\/\\|&^%~?:._]+|[()\[\]{};,@#`"])"#,
        )
        .unwrap();
        let mut out = Vec::new();
        let mut last = 0;
        for m in re.find_iter(text) {
            if m.start() > last {
                let seg = &text[last..m.start()];
                if !seg.is_empty() {
                    out.push(seg);
                }
            }
            let seg = m.as_str();
            if !seg.is_empty() {
                out.push(seg);
            }
            last = m.end();
        }
        if last < text.len() {
            let seg = &text[last..];
            if !seg.is_empty() {
                out.push(seg);
            }
        }
        out
    }

    let cases = [
        "",
        "   ",
        "abc",
        "let mut x: u32 = 123 + 456;",
        "fn calculate(a: &str, b: &[u8]) -> Result<Option<Vec<String>>, Box<dyn Error>> {",
        "// this is a comment with \t tabs \n and multiple spaces",
        "let unicode = \"こんにちは世界 — 🚀 \u{200b} zero width\";",
        "if (a >= b && c != d || !e) { return *ptr->val; }",
        "`code_block` and ```inline``` with @decorator and #tag;",
        "::path::to::Item<T, U>::new(..)",
        "a => b -> c +++ --- === !=== <=>= //*/*\\",
        "struct Foo<T: 'static + Send + Sync> { field: HashMap<String, (u32, i64)> }",
        "word\u{00A0}nbsp\u{1680}ogham\u{2000}enquad\u{2001}emquad\u{2002}ensp\u{2003}emsp",
        "thin\u{2009}hair\u{200A}line\u{2028}para\u{2029}narrow\u{202F}math\u{205F}ideo\u{3000}zw\u{200B}end",
        "let\u{3000}x\u{00A0}=\u{2002}1;",
    ];

    for case in cases {
        let actual: Vec<&str> = tokenize_line(case).collect();
        let expected = tokenize_regex(case);
        assert_eq!(
            actual, expected,
            "tokenize_line mismatch for case: {:?}",
            case
        );
    }
}

#[test]
fn test_char_slice_and_char_len() {
    // ASCII tests
    let s_ascii = "hello world";
    assert_eq!(char_len(s_ascii), 11);
    assert_eq!(char_slice(s_ascii, 0, 5), "hello");
    assert_eq!(char_slice(s_ascii, 6, 11), "world");
    assert_eq!(char_slice(s_ascii, 0, 0), "");
    assert_eq!(char_slice(s_ascii, 5, 5), "");
    assert_eq!(char_slice(s_ascii, 8, 4), "");
    assert_eq!(char_slice(s_ascii, 6, 50), "world");
    assert_eq!(char_slice(s_ascii, 50, 100), "");

    // Unicode tests
    let s_uni = "🦀 rocket 🚀 — world";
    assert_eq!(char_len(s_uni), 18);
    assert_eq!(char_slice(s_uni, 0, 1), "🦀");
    assert_eq!(char_slice(s_uni, 2, 8), "rocket");
    assert_eq!(char_slice(s_uni, 9, 10), "🚀");
    assert_eq!(char_slice(s_uni, 11, 12), "—");
    assert_eq!(char_slice(s_uni, 13, 18), "world");
    assert_eq!(char_slice(s_uni, 0, 0), "");
    assert_eq!(char_slice(s_uni, 5, 5), "");
    assert_eq!(char_slice(s_uni, 10, 2), "");
    assert_eq!(char_slice(s_uni, 13, 100), "world");
    assert_eq!(char_slice(s_uni, 50, 100), "");
}

#[test]
fn test_subword_token_length_weighting_preserves_operators() {
    let old_line = "            if h.rows[di].text().trim() == at {";
    let new_line = "            if h.rows[di].text() == a.text() {";
    let profile = crate::syntax::registry::lang_profile("rs");
    let ws = word_spans_with_profile(old_line, new_line, profile);

    assert_eq!(ws.del.len(), 2, "deleted .trim() and at");
    assert_eq!(char_slice(old_line, ws.del[0].0, ws.del[0].1), ".trim()");
    assert_eq!(char_slice(old_line, ws.del[1].0, ws.del[1].1), "at");

    assert_eq!(ws.add.len(), 1, "added a.text()");
    assert_eq!(char_slice(new_line, ws.add[0].0, ws.add[0].1), "a.text()");
}

#[test]
fn test_del_identifier_with_common_prefix() {
    let del = "                    } else if is_closing || is_continuation || matches_del {";
    let add = "                    } else if is_continuation || matches_del {";
    let ws = word_spans(del, add);
    assert_eq!(ws.add.len(), 0);
    assert_eq!(ws.del.len(), 1);
    assert_eq!(char_slice(del, ws.del[0].0, ws.del[0].1), "is_closing || ");
}

#[test]
fn test_repeated_identifier_prefers_longer_contiguous_match() {
    let del =
        "        let first_ascii_word = first_ascii_word(text).map(|w| arena.interner.intern(w));";
    let add = "            first_ascii_word(text).map(|w| arena.interner.intern(w))";
    let ws = word_spans(del, add);

    // The second "first_ascii_word" (the function call) must be matched.
    // Everything before it in del is unmatched as one contiguous run — the 8
    // leading spaces token is not equal to the 12-space token in add, so it
    // too lands in the deletion.
    assert_eq!(ws.del.len(), 2);
    assert_eq!(
        char_slice(del, ws.del[0].0, ws.del[0].1),
        "        let first_ascii_word = "
    );
    assert_eq!(char_slice(del, ws.del[1].0, ws.del[1].1), ";");

    // The 12-space indentation token in add is unmatched (no identical token in
    // del).
    assert_eq!(ws.add.len(), 1);
    assert_eq!(char_slice(add, ws.add[0].0, ws.add[0].1), "            ");
}

#[test]
fn test_repeated_identifier_raw_lcs_subproblem() {
    // Directly test raw_lcs_pairs_with_lookahead on the sub-segment that
    // lcs_pairs_anchored produces for the prefix before the "text" anchor.
    // Note: '_' is an operator char, so "first_ascii_word" tokenizes into
    // ["first", "_", "ascii", "_", "word"] — the variable-binding occurrence
    // spans a[3..7], the function-call occurrence a[11..15], with "(" at
    // a[16]. b = ["            ", "first", "_", "ascii", "_", "word", "("].
    let a_sub: Vec<&str> =
        tokenize_line("        let first_ascii_word = first_ascii_word(").collect();
    let b_sub: Vec<&str> = tokenize_line("            first_ascii_word(").collect();
    let pairs = raw_lcs_pairs_with_lookahead(&a_sub, &b_sub, Some("text"), Some("text"));
    // Must match the SECOND "first_ascii_word" (the function call) and the
    // trailing "(", NOT the variable-binding occurrence.
    let a_matched: Vec<usize> = {
        let mut v: Vec<usize> = pairs.keys().copied().collect();
        v.sort();
        v
    };
    assert_eq!(
        a_matched,
        vec![11, 12, 13, 14, 15, 16],
        "must match the function-call occurrence (a[11..15]) + '(', not the \
         variable binding (a[3..7]); got {a_matched:?}"
    );
}

#[test]
fn test_lcs_preserves_intact_parentheses_over_subsequent_identical_delimiter() {
    let old_line = "if !run.is_empty() && !moved_dels.contains(&run[0]) {";
    let new_line = "if !run.is_empty() {";
    let ot: Vec<&str> = tokenize_line(old_line).collect();
    let nt: Vec<&str> = tokenize_line(new_line).collect();
    let pairs = raw_lcs_pairs(&ot, &nt);
    // In ot, token 7 is ')' from 'is_empty()' and token 21 is ')' from
    // 'contains(&run[0])'. In nt, token 7 is ')' from 'is_empty()'.
    // The LCS must pair nt[7] with ot[7], not ot[21].
    assert_eq!(
        pairs.get(&7),
        Some(&7),
        "closing paren of is_empty() must pair with intact paren, not distant delimiter"
    );
    let spans = word_spans(old_line, new_line);
    assert_eq!(spans.add.len(), 0, "no added spans");
    assert_eq!(spans.del.len(), 1, "one contiguous deletion span");
    assert_eq!(
        char_slice(old_line, spans.del[0].0, spans.del[0].1),
        "&& !moved_dels.contains(&run[0]) "
    );
}

#[test]
fn test_lcs_preserves_contiguous_chained_method_calls() {
    let old_line = "builder.set_timeout(10).set_retries(3).build();";
    let new_line = "builder.set_retries(3).build();";
    let ot: Vec<&str> = tokenize_line(old_line).collect();
    let nt: Vec<&str> = tokenize_line(new_line).collect();
    let pairs = raw_lcs_pairs(&ot, &nt);
    // In ot: 0: builder, 1: ., 2: set, 3: _, 4: timeout, 5: (, 6: 10, 7: ), 8:
    // ., 9: set, 10: _, 11: retries ... In nt: 0: builder, 1: ., 2: set, 3:
    // _, 4: retries, 5: (, 6: 3, 7: ), 8: ., 9: build ... set_retries in
    // old_line (tokens 9..=11) should pair with set_retries in new_line (tokens
    // 2..=4).
    assert_eq!(
        pairs.get(&9),
        Some(&2),
        "set in set_retries pairs correctly"
    );
    assert_eq!(
        pairs.get(&11),
        Some(&4),
        "retries in set_retries pairs correctly"
    );
    let spans = word_spans(old_line, new_line);
    assert_eq!(spans.add.len(), 0, "no added spans");
    assert_eq!(spans.del.len(), 1, "deletion must cover set_timeout(10).");
    assert_eq!(
        char_slice(old_line, spans.del[0].0, spans.del[0].1),
        "set_timeout(10)."
    );
}

#[test]
fn test_lcs_max_tie_lookahead_bound() {
    assert_eq!(MAX_TIE_LOOKAHEAD, 16);
}

#[test]
fn test_contiguous_affinity_metrics() {
    let a = ["foo", " ", "bar", "baz", "qux"];
    let b = ["foo", " ", "bar", "baz", "qux"];
    // Centered at index 2 ("bar"):
    // backward: " " (ws, skipped), "foo" (1 token)
    // forward: "bar", "baz", "qux" (3 tokens) -> total 4
    assert_eq!(contiguous_affinity(&a, &b, 2, 2), 4);

    // Test candidate_contiguous_affinity:
    // With whitespace token in backward walk
    let a2 = ["ctx", "committed", "target", " ", "cand", "suffix"];
    let b2 = ["other", "target", " ", "cand", "suffix"];
    // committed_i = 1 ("committed"), last_committed_j = 0 ("other")
    // k = 4 ("cand"), j = 3 ("cand")
    // backward walk:
    // ki=4, bj=3: a2[3]=" ", b2[2]=" " -> matches, is_ws -> backward not
    // incremented ki=3, bj=2: a2[2]="target", b2[1]="target" -> matches,
    // !is_ws -> backward=1 ki=2, bj=1: ki-1=1 <= committed_i (1) -> stops!
    // forward walk: "cand", "suffix" -> 2 tokens
    // total: 1 + 2 = 3
    assert_eq!(candidate_contiguous_affinity(&a2, &b2, 1, 0, 4, 3), 3);

    // Stopped by last_committed_j:
    assert_eq!(candidate_contiguous_affinity(&a2, &b2, 0, 2, 4, 3), 2);

    // Stopped by token mismatch:
    let a3 = ["x", "mismatch_a", "cand"];
    let b3 = ["x", "mismatch_b", "cand"];
    assert_eq!(candidate_contiguous_affinity(&a3, &b3, 0, 0, 2, 2), 1);
}

#[test]
fn test_lcs_max_tie_lookahead_clamping() {
    assert_eq!(MAX_TIE_LOOKAHEAD, 16);

    // Construct lines where a matching token repeats beyond MAX_TIE_LOOKAHEAD
    // a has 31 tokens, with "target" at index 0 and index 25 (> 16 away).
    let mut a_tokens = vec!["target"];
    a_tokens.extend(["filler"; 24]);
    a_tokens.push("target");
    a_tokens.extend(["tail"; 5]);

    let b_tokens = vec!["target", "tail", "tail", "tail", "tail", "tail"];
    // At i=0, j=0 ("target"), the alternative at k=25 has longer forward run
    // (tail*5), BUT k=25 is beyond lookahead_limit (0 + 1 + 16 = 17).
    // So the lookahead limit clamps the search to < 17, and "target" at 0 is
    // matched!
    let pairs = raw_lcs_pairs(&a_tokens, &b_tokens);
    assert_eq!(
        pairs.get(&0),
        Some(&0),
        "bounded lookahead does not skip past MAX_TIE_LOOKAHEAD"
    );
}
