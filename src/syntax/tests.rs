//! Ports of the syntax-highlighting tests from test/review.test.ts.

use crate::syntax::{classify_tokens, lang_profile};

// The semantic colors, per the TS test's theme codes.
fn color_of(tok: &str) -> &str {
    match tok {
        "syntaxKeyword" => "31",
        "syntaxComment" => "32",
        "syntaxString" => "33",
        "syntaxNumber" => "34",
        "syntaxType" => "35",
        "syntaxFunction" => "36",
        "syntaxOperator" => "37",
        "text" => "90",
        _ => "90",
    }
}

fn has_color(
    text: &str,
    profile: Option<&crate::syntax::LangProfile>,
    code: &str,
    needle: &str,
) -> bool {
    classify_tokens(text, profile)
        .iter()
        .any(|t| t.text == needle && color_of(&t.color) == code)
}

#[test]
fn syntax_classification() {
    let rust = lang_profile("rs").unwrap();
    let rs = Some(rust);
    assert!(has_color("fn main() {", rs, "31", "fn"), "fn keyword");
    assert!(
        has_color("let mut x = 42;", rs, "31", "let")
            && has_color("let mut x = 42;", rs, "31", "mut"),
        "let/mut keywords"
    );
    assert!(
        has_color("// a comment", rs, "32", "// a comment"),
        "line comment"
    );
    assert!(has_color("let s = \"hi\";", rs, "33", "\"hi\""), "string");
    assert!(has_color("x = 0x1F;", rs, "34", "0x1F"), "hex number");
    assert!(has_color("Foo::new()", rs, "35", "Foo"), "capitalized type");
    assert!(has_color("call_it()", rs, "36", "call_it"), "function call");
    assert!(has_color("plain", rs, "90", "plain"), "plain identifier");
    // A comment after code keeps the rest tokenized.
    let mixed = classify_tokens("fn f() { // note", rs);
    assert!(
        mixed
            .iter()
            .any(|t| t.color == "syntaxComment" && t.text == "// note"),
        "comment after code"
    );
    assert!(
        mixed
            .iter()
            .any(|t| t.color == "syntaxKeyword" && t.text == "fn"),
        "fn still a keyword"
    );
    // Hash comments only for hashComment languages (python vs rust attrs).
    let py = lang_profile("py").unwrap();
    let py_tokens = classify_tokens("# derive", Some(py));
    assert!(
        py_tokens.iter().any(|t| t.color == "syntaxComment"),
        "py hash comment"
    );
    let rust_attr = classify_tokens("#[derive]", rs);
    assert!(
        !rust_attr.iter().any(|t| t.color == "syntaxComment"),
        "rust # is not a comment"
    );
    // Unknown language → plain text.
    let plain = classify_tokens("anything", None);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].color, "text");
    assert_eq!(plain[0].text, "anything");
}
