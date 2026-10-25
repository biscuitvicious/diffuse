//! Unit tests for tokenization, LCS, and word-level diff spans.

use super::*;
use crate::merge::{RunKind, merged_runs};
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
        "64",
        "add span 1"
    );
    assert_eq!(
        char_slice(add_text, ws.add[2].0, ws.add[2].1),
        "64",
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
        vec![(19, 36)],
        "addition span covers 'let Some(floor) ='"
    );
    assert_eq!(
        spans.del,
        vec![(19, 22), (28, 30)],
        "del spans cover 'k <' and ' {{', with ' floor' common"
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
            r#"([\p{White_Space}\u200b]+|[<>!=+\-*\/\\|&^%~?:._]+|[()\[\]{};,@#`"]|[0-9]+)"#,
        )
        .unwrap();
        let mut out = Vec::new();
        let mut last = 0;
        fn push_seg<'a>(out: &mut Vec<&'a str>, seg: &'a str) {
            let bytes = seg.as_bytes();
            let mut start = 0;
            for i in 1..bytes.len() {
                if (bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase())
                    || (i + 1 < bytes.len()
                        && bytes[i - 1].is_ascii_uppercase()
                        && bytes[i].is_ascii_uppercase()
                        && bytes[i + 1].is_ascii_lowercase())
                {
                    out.push(&seg[start..i]);
                    start = i;
                }
            }
            if start < seg.len() {
                out.push(&seg[start..]);
            }
        }
        for m in re.find_iter(text) {
            if m.start() > last {
                let seg = &text[last..m.start()];
                if !seg.is_empty() {
                    push_seg(&mut out, seg);
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
                push_seg(&mut out, seg);
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
fn test_tokenize_line_camel_case() {
    let tokens: Vec<&str> = tokenize_line("mConfig XMLParser getUserName FxHashMap").collect();
    assert_eq!(
        tokens,
        vec![
            "m", "Config", " ", "XML", "Parser", " ", "get", "User", "Name", " ", "Fx", "Hash",
            "Map"
        ]
    );
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
    let profile = diffuse_syntax::registry::lang_profile("rs");
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
    assert_eq!(spans.del.len(), 1, "deletion must cover .set_timeout(10)");
    assert_eq!(
        char_slice(old_line, spans.del[0].0, spans.del[0].1),
        ".set_timeout(10)"
    );
}

#[test]
fn test_lcs_tie_breaker_skips_in_b_for_longer_affinity() {
    let a_tokens = vec!["target", "tail", "tail", "tail", "tail", "tail"];
    let mut b_tokens = vec!["target"];
    b_tokens.extend(["filler"; 5]);
    b_tokens.push("target");
    b_tokens.extend(["tail"; 5]);

    let pairs = raw_lcs_pairs(&a_tokens, &b_tokens);
    assert_eq!(pairs.get(&0), Some(&6));
}

#[test]
fn test_lcs_symmetric_tie_breaker_json_runs() {
    let old_line = r#"{"t":"line","kind":"mod","newLine":111,"oldLine":86,"text":"  run with the extension).","delText":"  and expectations).","spans":{"del":[[0,20]],"add":[[0,26]]},"runs":[{"kind":"del","text":"  and expectations)."},{"kind":"add","color":"text","text":"  run with the extension)."}]}"#;
    let new_line = r#"{"t":"line","kind":"mod","newLine":111,"oldLine":86,"text":"  run with the extension).","delText":"  and expectations).","spans":{"del":[[0,20]],"add":[[0,26]]},"runs":[{"kind":"common","color":"text","text":"  "},{"kind":"del","text":"and expectations)."},{"kind":"add","color":"text","text":"run with the extension)."}]}"#;
    let profile = diffuse_syntax::registry::lang_profile("json");
    let ws = word_spans_with_profile(old_line, new_line, profile);

    let r = crate::parse::Row::mod_with_spans(
        crate::parse::DelLine::new(90, old_line),
        crate::parse::AddLine::new(90, new_line),
        ws,
    );
    let runs = merged_runs(&r);
    // Find the run that adds the new object: it must start cleanly with
    // {"kind":"common" rather than splitting "kind" from its value.
    let add_run = runs
        .iter()
        .find(|run| matches!(run.kind, RunKind::Add))
        .expect("should have an add run");
    assert_eq!(
        add_run.text,
        "{\"kind\":\"common\",\"color\":\"text\",\"text\":\"  \"},"
    );
}

#[test]
fn test_token_product_cap_get_set() {
    assert_eq!(token_product_cap(), DEFAULT_TOKEN_PRODUCT_CAP);
    assert_eq!(DEFAULT_TOKEN_PRODUCT_CAP, 250_000);

    set_token_product_cap(50_000);
    assert_eq!(token_product_cap(), 50_000);

    set_token_product_cap(DEFAULT_TOKEN_PRODUCT_CAP);
    assert_eq!(token_product_cap(), DEFAULT_TOKEN_PRODUCT_CAP);
}

#[test]
fn test_word_spans_empty_inputs() {
    let ws_both_empty = word_spans("", "");
    assert_eq!(ws_both_empty.del, vec![(0, 0)]);
    assert_eq!(ws_both_empty.add, vec![(0, 0)]);

    let ws_old_empty = word_spans("", "new content");
    assert_eq!(ws_old_empty.del, vec![(0, 0)]);
    assert_eq!(ws_old_empty.add, vec![(0, "new content".chars().count())]);

    let ws_new_empty = word_spans("old content", "");
    assert_eq!(ws_new_empty.del, vec![(0, "old content".chars().count())]);
    assert_eq!(ws_new_empty.add, vec![(0, 0)]);
}

#[test]
fn test_word_spans_with_cap_exceeded_on_untrimmed_middle() {
    // 10 tokens on old, 10 tokens on new, product = 100
    let old = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let new = "lambda mu nu xi omicron pi rho sigma tau upsilon";

    // When cap is 50 (< 100), word diffing falls back:
    let ws_capped = word_spans_with_profile_and_cap(old, new, None, 50);
    assert_eq!(ws_capped.del, vec![(0, old.chars().count())]);
    assert_eq!(ws_capped.add, vec![(0, new.chars().count())]);

    // When cap is 200 (> 100), word diffing runs normally:
    let ws_uncapped = word_spans_with_profile_and_cap(old, new, None, 200);
    assert_eq!(ws_uncapped.del, vec![(0, old.chars().count())]);
    assert_eq!(ws_uncapped.add, vec![(0, new.chars().count())]);
}

#[test]
fn test_long_json_record_subline_highlighting_with_raised_cap() {
    let old_line = "{\"t\":\"line\",\"kind\":\"mod\",\"newLine\":170,\"oldLine\":164,\"text\":\"      rewrites). Break only for syntax (no `undefined`, no dynamic typing)\",\"delText\":\"  Break only for syntax (no `undefined`, no dynamic typing) and naming\",\"spans\":{\"del\":[[0,2],[59,70]],\"add\":[[0,17]]},\"runs\":[{\"kind\":\"del\",\"text\":\"  \"},{\"kind\":\"add\",\"color\":\"text\",\"text\":\"      rewrites). \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"Break only for syntax (no `undefined`, no dynamic typing)\"},{\"kind\":\"del\",\"text\":\" and naming\"}]}";
    let new_line = "{\"t\":\"line\",\"kind\":\"mod\",\"newLine\":170,\"oldLine\":164,\"text\":\"      rewrites). Break only for syntax (no `undefined`, no dynamic typing)\",\"delText\":\"  Break only for syntax (no `undefined`, no dynamic typing) and naming\",\"spans\":{\"del\":[[0,2],[59,70]],\"add\":[[0,17]]},\"runs\":[{\"kind\":\"add\",\"color\":\"text\",\"text\":\"    \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"  \"},{\"kind\":\"add\",\"color\":\"text\",\"text\":\"rewrites). \"},{\"kind\":\"common\",\"color\":\"text\",\"text\":\"Break only for syntax (no `undefined`, no dynamic typing)\"},{\"kind\":\"del\",\"text\":\" and naming\"}]}";

    // With old cap of 40,000: falls back to whole-line strike/fill:
    let ws_old_cap = word_spans_with_profile_and_cap(old_line, new_line, None, 40_000);
    assert_eq!(ws_old_cap.del, vec![(0, old_line.chars().count())]);
    assert_eq!(ws_old_cap.add, vec![(0, new_line.chars().count())]);

    // With new cap of 250,000: produces granular subline spans:
    let ws_new_cap = word_spans_with_profile_and_cap(old_line, new_line, None, 250_000);
    assert_ne!(ws_new_cap.del, vec![(0, old_line.chars().count())]);
    assert_ne!(ws_new_cap.add, vec![(0, new_line.chars().count())]);
    assert!(!ws_new_cap.del.is_empty());
    assert!(!ws_new_cap.add.is_empty());
}

#[test]
fn test_agents_md_subword_alignment() {
    let old_line = "The workspace also holds the engine's first consumer: **`extension/pi/`** —";
    let new_line = "The workspace also holds **`extension/pi/`** — the pi review extension";
    let ws = word_spans(old_line, new_line);
    // "the engine's first consumer: " deleted from old line
    assert_eq!(ws.del, vec![(25, 54)]);
    // " the pi review extension" added to new line
    assert_eq!(ws.add, vec![(46, 70)]);
}

#[test]
fn test_do_not_split_path_on_isolated_dot() {
    let old = "  `extension/pi/scripts/install.sh` syncs review.ts + the binary into";
    let new = "  `extension/pi/scripts/install.sh` only if requested); `./scripts/install.sh`";
    let ws = word_spans(old, new);
    // Should NOT match the dot between review.ts and install.sh!
    // The changed segment should be a single contiguous deletion and addition.
    assert_eq!(ws.del, vec![(36, 69)]);
    assert_eq!(ws.add, vec![(36, 78)]);
}

#[test]
fn test_path_dot_matching_extension_preserves_common_ext() {
    let old = "let f = open(\"input.rs\");";
    let new = "let f = open(\"output.rs\");";
    let ws = word_spans(old, new);
    assert_eq!(ws.del, vec![(14, 19)]); // "input"
    assert_eq!(ws.add, vec![(14, 20)]); // "output"
}

#[test]
fn test_path_dot_matching_basename_preserves_common_prefix() {
    let old = "let script = \"install.sh\";";
    let new = "let script = \"install.bash\";";
    let ws = word_spans(old, new);
    assert_eq!(ws.del, vec![(22, 24)]); // "sh"
    assert_eq!(ws.add, vec![(22, 26)]); // "bash"
}

#[test]
fn test_path_dot_with_slashes_unmatched_extension() {
    let old = "load(\"dir/foo.custom1\");";
    let new = "load(\"other/bar.custom2\");";
    let ws = word_spans(old, new);
    // Generic alignment replaces differing prefixes and digit suffixes while
    // matching common `.custom` without domain-specific path rules:
    assert_eq!(ws.del, vec![(6, 13), (20, 21)]);
    assert_eq!(ws.add, vec![(6, 15), (22, 23)]);
}

#[test]
fn test_unweighted_lcs_fragments_unknown_extension_dot() {
    let a = ["foo", ".", "custom1"];
    let b = ["bar", ".", "custom2"];
    let pairs = raw_lcs_pairs(&a, &b);
    // In current code, because "custom1" is not in the hardcoded table of 40
    // extensions, raw_lcs_pairs fragments the edit and matches the lone
    // dot! Under Gotoh affine gap scoring, this isolated dot will be
    // cleanly rejected.
    assert!(
        pairs.is_empty(),
        "Expected wholesale replacement without matching isolated dot between unknown extensions, got: {pairs:?}"
    );
}

#[test]
fn test_leading_whitespace_does_not_match_internal_whitespace() {
    let a = ["    ", "foo"];
    let b = ["bar", "    ", "foo"];
    let pairs = raw_lcs_pairs(&a, &b);
    assert_eq!(
        pairs.get(&0),
        None,
        "Leading indentation must not match internal formatting whitespace: {pairs:?}"
    );
}

#[test]
fn test_trailing_whitespace_does_not_match_leading_whitespace() {
    let a = ["foo", "   "];
    let b = ["   ", "foo"];
    let pairs = raw_lcs_pairs(&a, &b);
    assert_eq!(
        pairs.get(&1),
        None,
        "Trailing whitespace must not match leading indentation: {pairs:?}"
    );
}

#[test]
fn test_scratch_arena_equivalence_and_reuse() {
    let a1 = ["alpha", "beta", "gamma", "delta"];
    let b1 = ["alpha", "gamma", "delta", "epsilon"];
    let p1 = raw_lcs_pairs(&a1, &b1);

    // Call with different sizes to exercise resize and dirty buffer overwrite
    let a2 = ["x", "y"];
    let b2 = ["x", "z", "y"];
    let p2 = raw_lcs_pairs(&a2, &b2);

    let p1_repeat = raw_lcs_pairs(&a1, &b1);
    assert_eq!(p1, p1_repeat);

    let p2_repeat = raw_lcs_pairs(&a2, &b2);
    assert_eq!(p2, p2_repeat);
}

#[test]
fn test_note_lines_addition_alignment() {
    // Verifies that contiguous snake_case subwords retain internal lexical
    // cohesion rather than splitting across adjacent binary operator
    // insertions.
    let old = "            nav_counts.push((div_lines + row_segs + note_lines).max(1));";
    let new = "            nav_counts.push((div_lines + row_segs + note_lines + trailing_gap_lines).max(1));";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "            nav_counts.push((div_lines + row_segs + note_lines{+ + trailing_gap_lines+}).max(1));"
    );
}

#[test]
fn test_token_is_alnum_replacement() {
    // Verifies that compound identifiers whose prefixes and suffixes both
    // differ undergo wholesale replacement rather than splicing
    // coincidental internal subwords.
    let old = "        && token_is_alnum(t1)";
    let new = "        && is_subword_token(t1)";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "        && [-token_is_alnum-]{+is_subword_token+}(t1)"
    );
}

#[test]
fn test_prefix_gutter_comment_alignment() {
    let old = "/// Generate plain 6-character prefix gutter (` 123 + `, ` 123≈+`, ` 123≈-`).";
    let new = "/// Generate plain 6-character prefix gutter (` 123 +`, ` 123 -`, ` 123 ~`, `";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "/// Generate plain 6-character prefix gutter (` 123 +[- -]`, ` 123[-≈+-]{+ -+}`, ` 123[-≈--]{+ ~`, +}`[-).-]"
    );
}

#[test]
fn test_path_change_local_to_parenthetical_addition() {
    let old = "Defaults to ~/.pi/review/notes.json.\"";
    let new =
        "Defaults to $XDG_STATE_HOME/diffuse/notes.json (or ~/.local/state/diffuse/notes.json).\"";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "Defaults to [-~/.pi/review-]{+$XDG_STATE_HOME/diffuse+}/notes.json{+ (or ~/.local/state/diffuse/notes.json)+}.\""
    );
}

#[test]
fn test_pi_review_path_alignment() {
    let old = "   TypeScript. It lives at `~/lab/pi-review/review.ts` (the functions";
    let new = "   TypeScript. It lives at `extension/pi/review.ts` in this workspace (the";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "   TypeScript. It lives at `[-~/lab-]{+extension+}/pi[--review-]/review.ts` {+in this workspace +}(the[- functions-]"
    );
}

#[test]
fn test_unicode_camel_case_tokenization() {
    let tokens: Vec<&str> = tokenize_line("мояФункция").collect();
    assert_eq!(tokens, vec!["моя", "Функция"]);

    let tokens: Vec<&str> = tokenize_line("ПРКлиент").collect();
    assert_eq!(tokens, vec!["ПР", "Клиент"]);

    let tokens: Vec<&str> = tokenize_line("monÉlément").collect();
    assert_eq!(tokens, vec!["mon", "Élément"]);
}

#[test]
fn test_unicode_subline_alignment() {
    let old = "    let résultat_ancien = calculer_total(valeur);";
    let new = "    let résultat_nouveau = calculer_total(valeur);";
    let (_, runs) = word_spans_and_runs_with_profile(old, new, None);
    let formatted: String = runs
        .iter()
        .map(|(kind, text)| match kind {
            RunKind::Ctx => text.clone(),
            RunKind::Del | RunKind::MovedSrc => format!("[-{text}-]"),
            RunKind::MovedTgt => format!("[>{text}>]"),
            RunKind::Add => format!("{{+{text}+}}"),
        })
        .collect();
    assert_eq!(
        formatted,
        "    let résultat_[-ancien-]{+nouveau+} = calculer_total(valeur);"
    );
}

#[test]
fn test_lcs_does_not_fragment_on_unbalanced_container_delimiter() {
    let old_line = r#""fn test() {\n    let x = 1;\n""#;
    let new_line = r#""{prefix}    let x = 1;\n""#;
    let spans = word_spans(old_line, new_line);
    assert_eq!(
        spans.del.len(),
        1,
        "expected 1 contiguous deletion span: {:?}",
        spans.del
    );
    assert_eq!(
        spans.add.len(),
        1,
        "expected 1 contiguous addition span: {:?}",
        spans.add
    );
    assert_eq!(
        char_slice(old_line, spans.del[0].0, spans.del[0].1),
        r#"fn test() {\n"#
    );
    assert_eq!(
        char_slice(new_line, spans.add[0].0, spans.add[0].1),
        "{prefix}"
    );
}

#[test]
fn test_lcs_does_not_mix_method_signature_parens_with_call_parens() {
    let old_line =
        r#"        "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),"#;
    let new_line =
        r#"        format!("{prefix}    let x = 1;\n    // old comment — em dash\n}}\n"),"#;
    let spans = word_spans(old_line, new_line);
    let first_del = char_slice(old_line, spans.del[0].0, spans.del[0].1);
    assert!(
        first_del.contains("fn test()"),
        "expected 'fn test()' to be part of the first deletion span without parens matched, got: {first_del:?}, all dels: {:?}",
        spans.del
    );
    let old_theme = "    pub fn fallback_named_color(name: &str) -> Option<&'static str> {";
    let new_theme = "    pub fn auto_detect() -> Self {";
    let (_, runs_theme) = word_spans_and_runs_with_profile(old_theme, new_theme, None);
    assert!(
        runs_theme
            .iter()
            .any(|(k, t)| *k == RunKind::Ctx && t.contains('(')),
        "expected '(' to match as context between method declarations, got: {runs_theme:?}"
    );
    assert!(
        runs_theme
            .iter()
            .any(|(k, t)| *k == RunKind::Ctx && t.contains(')')),
        "expected ')' to match as context between method declarations, got: {runs_theme:?}"
    );

    let old_line_grid = "        for cell in count_cells(&file.counts) {";
    let new_line_grid = "        for cell in count_cells(&file.counts, &theme.palette) {";
    let (_, runs_grid) = word_spans_and_runs_with_profile(old_line_grid, new_line_grid, None);
    assert!(
        runs_grid
            .iter()
            .any(|(k, t)| *k == RunKind::Ctx && t.contains(')')),
        "expected ')' to match as context in count_cells, got: {runs_grid:?}"
    );
    assert!(
        runs_grid
            .iter()
            .any(|(k, t)| *k == RunKind::Ctx && t.contains('{')),
        "expected '{{' to match as context in count_cells, got: {runs_grid:?}"
    );

    let del_text =
        "    pub fn new(output: PipelineOutput, repo: String, notes_path: PathBuf) -> Self {";
    let add_nxt = "    ) -> Self {";
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let tape = crate::tape::AlignmentTape::from_texts_with_cap(del_text, add_nxt, profile, 40000);
    let runs_aux = tape.to_merged_runs();
    assert!(
        runs_aux
            .iter()
            .any(|r| r.kind == RunKind::Ctx && r.text.starts_with(')')),
        "expected ')' to match as context between multiline decl closing and full signature, got: {runs_aux:?}"
    );
}

#[test]
fn container_delimiter_depth_preference() {
    let old_import = "    path::Path,";
    let new_import = "    path::{Path, PathBuf},";
    let profile = diffuse_syntax::registry::lang_profile("rs");
    let tape = crate::tape::AlignmentTape::from_texts(old_import, new_import, profile);
    let runs = tape.to_merged_runs();
    assert_eq!(
        runs,
        vec![
            crate::merge::Run {
                kind: RunKind::Ctx,
                text: "    path::",
            },
            crate::merge::Run {
                kind: RunKind::Add,
                text: "{",
            },
            crate::merge::Run {
                kind: RunKind::Ctx,
                text: "Path",
            },
            crate::merge::Run {
                kind: RunKind::Add,
                text: ", PathBuf}",
            },
            crate::merge::Run {
                kind: RunKind::Ctx,
                text: ",",
            },
        ]
    );
}

#[test]
fn test_unicode_delimiter_tokenization() {
    let text = "fn foo（x: i32） -> 【bar】；";
    let tokens: Vec<&str> = tokenize_line(text).collect();
    assert_eq!(
        tokens,
        vec![
            "fn", " ", "foo", "（", "x", ":", " ", "i", "32", "）", " ", "->", " ", "【", "bar",
            "】", "；"
        ]
    );
}

#[test]
fn test_token_is_salient_profile_literals_and_keywords() {
    let go = diffuse_syntax::registry::lang_profile("go");
    let rs = diffuse_syntax::registry::lang_profile("rs");

    // Rust: keywords
    assert!(!crate::lcs::token_is_salient("true", rs));
    assert!(!crate::lcs::token_is_salient("false", rs));
    assert!(!crate::lcs::token_is_salient("match", rs));
    assert!(crate::lcs::token_is_salient("my_var", rs));

    // Go: literals vs keywords vs identifiers
    assert!(!crate::lcs::token_is_salient("true", go));
    assert!(!crate::lcs::token_is_salient("false", go));
    assert!(!crate::lcs::token_is_salient("nil", go));
    assert!(!crate::lcs::token_is_salient("func", go));
    assert!(crate::lcs::token_is_salient("myVar", go));

    // Without profile fallback: all alphanumerics are salient
    assert!(crate::lcs::token_is_salient("true", None));
    assert!(crate::lcs::token_is_salient("false", None));
    assert!(crate::lcs::token_is_salient("ident", None));
    assert!(!crate::lcs::token_is_salient("   ", None));
    assert!(!crate::lcs::token_is_salient("==", None));
}
