use crate::{
    lcs::tokenize_line,
    syntax::registry::lang_profile,
    token::{
        MIN_SINGLE_TOKEN_ENTROPY, TokenClass, classify_token, entropy_anchors,
        multi_token_phrase_anchors, token_self_information,
    },
};

#[test]
fn test_token_classification() {
    let rs_profile = lang_profile("rs");

    assert_eq!(classify_token("   ", None), TokenClass::Whitespace);
    assert_eq!(classify_token("\t", None), TokenClass::Whitespace);
    assert_eq!(classify_token(";", None), TokenClass::Delimiter);
    assert_eq!(classify_token("(", None), TokenClass::Delimiter);
    assert_eq!(classify_token(")", None), TokenClass::Delimiter);
    assert_eq!(classify_token("{", None), TokenClass::Delimiter);
    assert_eq!(classify_token("}", None), TokenClass::Delimiter);

    assert_eq!(classify_token("=", None), TokenClass::Operator);
    assert_eq!(classify_token("=>", rs_profile), TokenClass::Operator);
    assert_eq!(classify_token("->", rs_profile), TokenClass::Operator);

    assert_eq!(classify_token("fn", rs_profile), TokenClass::Keyword);
    assert_eq!(classify_token("let", rs_profile), TokenClass::Keyword);
    assert_eq!(classify_token("struct", rs_profile), TokenClass::Keyword);

    assert_eq!(
        classify_token("first_ascii_word", None),
        TokenClass::Identifier
    );
    assert_eq!(
        classify_token("calculate_hash", None),
        TokenClass::Identifier
    );
    assert_eq!(classify_token("MyStruct", None), TokenClass::Identifier);

    assert_eq!(classify_token("123", None), TokenClass::Literal);
    assert_eq!(classify_token("0x42", None), TokenClass::Literal);

    assert_eq!(classify_token("the", None), TokenClass::FunctionWord);
    assert_eq!(classify_token("workspace", None), TokenClass::ProseWord);
    assert_eq!(classify_token("consumer", None), TokenClass::ProseWord);
}

#[test]
fn test_token_self_information() {
    let rs_profile = lang_profile("rs");

    let info_the = token_self_information("the", None);
    let info_a = token_self_information("a", None);
    let info_semicolon = token_self_information(";", None);
    let info_ident = token_self_information("calculate_hash", rs_profile);
    let info_keyword = token_self_information("struct", rs_profile);

    // Short stop words and punctuation must score below
    // MIN_SINGLE_TOKEN_ENTROPY
    assert!(
        info_the < MIN_SINGLE_TOKEN_ENTROPY,
        "info_the ({info_the}) should be < {MIN_SINGLE_TOKEN_ENTROPY}"
    );
    assert!(
        info_a < MIN_SINGLE_TOKEN_ENTROPY,
        "info_a ({info_a}) should be < {MIN_SINGLE_TOKEN_ENTROPY}"
    );
    assert!(
        info_semicolon < MIN_SINGLE_TOKEN_ENTROPY,
        "info_semicolon ({info_semicolon}) should be < {MIN_SINGLE_TOKEN_ENTROPY}"
    );

    // Significant keywords and identifiers must score well above
    // MIN_SINGLE_TOKEN_ENTROPY
    assert!(
        info_ident > MIN_SINGLE_TOKEN_ENTROPY,
        "info_ident ({info_ident}) should be > {MIN_SINGLE_TOKEN_ENTROPY}"
    );
    assert!(
        info_keyword > MIN_SINGLE_TOKEN_ENTROPY,
        "info_keyword ({info_keyword}) should be > {MIN_SINGLE_TOKEN_ENTROPY}"
    );
}

#[test]
fn test_multi_token_phrase_detection() {
    let a: Vec<&str> = tokenize_line("fn calculate_hash(key: &str) -> u64 {").collect();
    let b: Vec<&str> =
        tokenize_line("pub fn calculate_hash(key: &str) -> u64 /* fast */ {").collect();

    let rs_profile = lang_profile("rs");
    let phrase_anchors = multi_token_phrase_anchors(&a, &b, rs_profile);
    assert!(
        !phrase_anchors.is_empty(),
        "Should find multi-token phrase anchors"
    );

    // All anchored token pairs must match identically
    for &(ai, bi) in &phrase_anchors {
        assert_eq!(a[ai], b[bi]);
    }
}

#[test]
fn test_entropy_anchors_agents_md_line_21() {
    let old_line = "The workspace also holds the engine's first consumer: **`extension/pi/`** —";
    let new_line = "The workspace also holds **`extension/pi/`** — the pi review extension";

    let ot: Vec<&str> = tokenize_line(old_line).collect();
    let nt: Vec<&str> = tokenize_line(new_line).collect();

    let anchors = entropy_anchors(&ot, &nt, None);

    // Ensure anchors are monotonic
    for w in anchors.windows(2) {
        assert!(
            w[0].0 < w[1].0,
            "Anchors must strictly increase on old index"
        );
        assert!(
            w[0].1 < w[1].1,
            "Anchors must strictly increase on new index"
        );
    }

    // Verify that "the" from old line ("the engine's") does NOT anchor to "the"
    // on new line ("the pi review")
    for &(oi, ni) in &anchors {
        if ot[oi] == "the" {
            panic!("Orphan 'the' was anchored across phrase boundaries: ({oi}, {ni})");
        }
    }

    // Anchors include high-entropy prefix tokens
    let old_ws_pos = ot.iter().position(|&t| t == "workspace").unwrap();
    let new_ws_pos = nt.iter().position(|&t| t == "workspace").unwrap();
    assert!(anchors.contains(&(old_ws_pos, new_ws_pos)));

    // When used in word_spans, produces exact intended spans without stop-word
    // misalignments
    let spans = crate::lcs::word_spans(old_line, new_line);
    assert_eq!(spans.del, vec![(25, 54)]);
    assert_eq!(spans.add, vec![(46, 70)]);
}

#[test]
fn test_entropy_anchors_subword_not_anchored_to_standalone_token() {
    let old = "    /// Resolve a theme by name (`\"magic\"`, `\"deep_black\"`, `\"legacy\"`).";
    let new = "    /// Resolve a theme by name (`\"default\"`, `\"dark\"`, `\"black\"`).";
    let ot: Vec<&str> = tokenize_line(old).collect();
    let nt: Vec<&str> = tokenize_line(new).collect();
    let anchors = entropy_anchors(&ot, &nt, None);

    // "black" in old is a subword continuation of "deep_black" (preceded by
    // "_"). In new, "black" is a standalone string literal.
    // It must NOT be selected as a single-token anchor.
    for &(oi, ni) in &anchors {
        if ot[oi] == "black" {
            panic!(
                "Subword 'black' from 'deep_black' was falsely anchored to standalone 'black': ({oi}, {ni})"
            );
        }
    }
}

#[test]
fn test_entropy_anchors_prose_word_not_anchored_to_code_span() {
    let old = "    /// falling back to the default `magic` theme if unset or invalid.";
    let new = "    /// falling back to `default` if unset or invalid.";
    let ot: Vec<&str> = tokenize_line(old).collect();
    let nt: Vec<&str> = tokenize_line(new).collect();
    let anchors = entropy_anchors(&ot, &nt, None);

    // "default" in old is a prose word outside backticks.
    // In new, "default" is inside a code span `default`.
    // It must NOT be anchored across container boundaries.
    for &(oi, ni) in &anchors {
        if ot[oi] == "default" {
            panic!("Prose 'default' was falsely anchored to code-span 'default': ({oi}, {ni})");
        }
    }
}
