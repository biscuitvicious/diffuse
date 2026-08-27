//! Ports of the syntax-highlighting tests from test/review.test.ts.

use std::ptr::addr_eq;

use crate::prelude::*;

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
        _ => "90",
    }
}

fn has_color(text: &str, profile: Option<&LangProfile>, code: &str, needle: &str) -> bool {
    classify_tokens(text, profile)
        .iter()
        .any(|t| t.text == needle && color_of(t.color) == code)
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

    // Backticks inside doc comments get accent backticks + Rust syntax tokens.
    let doc_bt = classify_tokens("/// Uses `VFS::resolve` to read.", rs);
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == "syntaxComment" && t.text.contains("Uses"))
    );

    assert!(doc_bt.iter().any(|t| t.color == "accent" && t.text == "`"));
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == "syntaxType" && t.text == "VFS")
    );
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == "syntaxOperator" && t.text == ":")
    );
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == "syntaxComment" && t.text == " to read.")
    );
}

// Rust doc-comment fences with doctest execution modifiers — `no_run`,
// `ignore`, `should_panic`, `compile_fail`, `edition2021`, `ignore_linux`,
// and combos like `rust,no_run,edition2021` — still contain valid Rust
// syntax and must open a region that RESOLVES to the `rs` profile. The
// fence regex's capture group must accept commas, and `lang_profile` must
// recognize the modifier list as Rust (bare `no_run`/`edition*` tags imply
// Rust inside a `.rs` doc). The region's `lang` carries the raw tag; the
// profile is resolved per-row via `lang_profile(lang)`.
fn lines(v: &[&str]) -> Vec<String> {
    v.iter().map(std::string::ToString::to_string).collect()
}

/// Assert the region's tag resolves to a target profile key.
fn region_is(rs: &LangProfile, doc: &[String], want: &str) {
    let regions = fence_regions(doc, Some(rs));
    assert_eq!(regions.len(), 1, "should open exactly one region");
    let lang = regions[0].lang.as_deref().expect("region has a lang");
    let resolved = lang_profile(lang).expect("lang resolves to a profile");
    let want_prof = lang_profile(want).unwrap();
    assert!(
        addr_eq(resolved, want_prof),
        "tag `{lang}` must resolve to the `{want}` profile"
    );
}

#[test]
fn rust_doc_fence_with_execution_modifier_opens_rust_region() {
    let rs = lang_profile("rs").unwrap();
    let cases = [
        "```rust",
        "```rust,no_run",
        "```no_run",
        "```should_panic",
        "```compile_fail",
        "```ignore",
        "```edition2021",
        "```ignore_linux",
        "```rust,no_run,edition2021",
        "```no_run,ignore",
    ];
    for tag in cases {
        let doc = lines(&[
            "/// leading prose.",
            &format!("/// {tag}"),
            "/// let x = 42;",
            "/// ```",
            "/// trailing prose.",
        ]);
        region_is(rs, &doc, "rs");
        // The code line (line 3) is inside the region bounded by the
        // opening fence (line 2) and closing fence (line 4).
        let regions = fence_regions(&doc, Some(rs));
        assert_eq!(regions[0].start, 3);
        assert_eq!(regions[0].end, 3);
    }
}

#[test]
fn rust_doc_fence_unlabeled_defaults_to_rust() {
    let rs = lang_profile("rs").unwrap();
    let doc = lines(&["/// ```", "/// let x = 5;", "/// ```"]);
    region_is(rs, &doc, "rs");
}

#[test]
fn rust_doc_fence_named_other_language_overrides_rust_default() {
    // A ` ```python ` example embedded in a rust crate's doc highlights
    // as python, not rust — the language alias takes precedence over the
    // implied-rust default.
    let rs = lang_profile("rs").unwrap();
    let doc = lines(&["/// ```python", "/// def f(): pass", "/// ```"]);
    region_is(rs, &doc, "py");
}

#[test]
fn md_fence_with_rust_modifier_opens_rust_region() {
    let md = lang_profile("md").unwrap();
    let doc = lines(&["```rust,no_run", "let x = 42;", "```"]);
    let regions = fence_regions(&doc, Some(md));
    assert_eq!(regions.len(), 1);
    let lang = regions[0].lang.as_deref().expect("region has a lang");
    assert!(
        addr_eq(lang_profile(lang).unwrap(), lang_profile("rs").unwrap()),
        "md fence tag `{lang}` must resolve to the rs profile"
    );
}

// Plain (`//`, `/*`, `#`) comments are NOT markdown: only inline `` `code` ``
// spans render (accent backticks + Rust-classified inner); no `**bold**`,
// `*italic*`, `~~strike~~`, `[link](url)`. Doc comments (`///`, `//!`) ARE
// markdown — full emphasis. The split lives in
// `tokenize_comment_with_backticks`.
fn colors_of<'a>(text: &'a str, profile: Option<&LangProfile>) -> Vec<(&'static str, &'a str)> {
    classify_tokens(text, profile)
        .into_iter()
        .map(|t| (t.color, t.text))
        .collect()
}

#[test]
fn plain_comment_backtick_renders_as_rust_no_markdown() {
    let rs = lang_profile("rs").unwrap();
    // A `//` comment: the `` `if`/`k` `` backtick spans render as accent +
    // Rust-classified, but the surrounding text (and the `**not bold**`,
    // `*not italic*`, `[not link](url)`) stays comment-colored — no emphasis.
    let toks = colors_of(
        "// keep its `if`/`k`/`floor {` fragments; **not bold** *not italic* [not link](u)",
        Some(rs),
    );
    // backtick delimiters are accent
    assert!(
        toks.iter().any(|&(c, t)| c == "accent" && t == "`"),
        "backticks accent"
    );
    // inner `if` / `k` classify as Rust keyword (syntaxKeyword), not comment
    assert!(
        toks.iter().any(|&(c, t)| c == "syntaxKeyword" && t == "if"),
        "`if` inside backticks renders as a Rust keyword"
    );
    // the `**not bold**` text outside backticks stays syntaxComment — no mdBold
    assert!(
        toks.iter()
            .any(|&(c, t)| c == "syntaxComment" && t.contains("**not bold**")),
        "`**bold**` in a plain comment (outside backticks) stays comment-colored"
    );
    // `[not link](u)` outside backticks stays syntaxComment — no mdUrl
    assert!(
        toks.iter()
            .any(|&(c, t)| c == "syntaxComment" && t.contains("[not link]")),
        "`[link](url)` in a plain comment stays comment-colored"
    );
    // no mdBold / mdItalic / mdStrike / mdLink / mdUrl tokens at all
    assert!(
        toks.iter()
            .all(|&(c, _)| !matches!(c, "mdBold" | "mdItalic" | "mdStrike" | "mdLink" | "mdUrl")),
        "no markdown emphasis tokens in a plain comment"
    );
}

#[test]
fn doc_comment_full_markdown_renders() {
    let rs = lang_profile("rs").unwrap();
    // A `///` doc comment: `**bold**` renders as accent `**` + mdBold body,
    // and `` `code` `` renders as accent backticks + Rust inner.
    let toks = colors_of("/// **bold** and `code`", Some(rs));
    assert!(
        toks.iter().any(|&(c, t)| c == "mdBold" && t == "bold"),
        "doc bold renders"
    );
    assert!(
        toks.iter().any(|&(c, t)| c == "accent" && t == "`"),
        "doc backticks accent"
    );
    assert!(
        toks.iter()
            .any(|&(c, t)| c == "syntaxKeyword" || t == "code"),
        "doc inner code classified"
    );
}

#[test]
fn plain_comment_without_backtick_is_uniform_comment() {
    let rs = lang_profile("rs").unwrap();
    // No backtick, no markdown triggers → the whole line is one syntaxComment
    // token.
    let toks = colors_of(
        "// a plain comment with **stars** and *italics* but no backticks",
        Some(rs),
    );
    assert_eq!(toks.len(), 1, "one token");
    assert_eq!(toks[0].0, "syntaxComment");
}

#[test]
fn fence_regions_unclosed_ignore_block_does_not_invert_subsequent_prose() {
    let rs = lang_profile("rs").unwrap();
    let lines = vec![
        "/// ```ignore".to_string(),
        "/// let x = 1;".to_string(),
        "/// ```ignore".to_string(),
        "/// let y = 2;".to_string(),
        "/// ```".to_string(),
        "/// **Closure / `fn` item (typed handle)** — a single serialized payload in,".to_string(),
        "/// ```ignore".to_string(),
        "/// let z = 3;".to_string(),
        "/// ```".to_string(),
    ];
    let regions = fence_regions(&lines, Some(rs));
    let line6_region = region_at(&regions, 6);
    assert!(
        line6_region.is_none(),
        "line 6 prose comment is NOT inside a code fence region"
    );
}

#[test]
fn test_markdown_exact_text_preservation_invariant() {
    let samples = [
        "# Heading 1",
        "### Subheading with `inline code` and **bold**",
        "| Header A | Header B |\n|---|---|\n| cell 1 | cell 2 |",
        "- [ ] open task item\n- [x] completed task item",
        "> [!NOTE]\n> This is a helpful callout note with [link](https://example.com).",
        "> [!WARNING]\n> High risk operation ahead.",
        "<details open><summary>Summary Title</summary>Content</details>",
        "Plain text with *italic*, ~~strikethrough~~, and `code` span.",
        "---",
        "***\n___",
    ];

    for sample in samples {
        let mut reconstructed = String::new();
        for_each_markdown_token(sample, |t| {
            reconstructed.push_str(t.text);
        });
        assert_eq!(
            reconstructed, sample,
            "Reconstructed token stream must match original text byte-for-byte"
        );
    }
}

#[test]
fn test_markdown_tables_and_borders() {
    let table = "| Col A | Col B |\n|---|---|\n| 1 | 2 |";
    let toks = markdown_tokens(table);
    assert!(
        toks.iter()
            .any(|t| t.color == "border" && t.text.contains('|')),
        "table pipes must be styled as border"
    );
    assert!(
        toks.iter()
            .any(|t| t.sgr == Some(MD_SGR_BOLD) && t.text.contains("Col A")),
        "table head must be bold"
    );
}

#[test]
fn test_markdown_task_list_markers() {
    let task = "- [ ] todo\n- [x] done";
    let toks = markdown_tokens(task);
    assert!(
        toks.iter().any(|t| t.text.contains("[ ]")),
        "unchecked task list marker"
    );
    assert!(
        toks.iter().any(|t| t.text.contains("[x]")),
        "checked task list marker"
    );
}

#[test]
fn test_markdown_github_alerts_and_blockquotes() {
    let note = "> [!NOTE]\n> Important note body";
    let toks = markdown_tokens(note);
    assert!(
        toks.iter()
            .any(|t| t.color == "accent" && t.text.contains('>')),
        "blockquote chevron must be accent"
    );
    assert!(
        toks.iter()
            .any(|t| t.color == "syntaxFunction" && t.sgr == Some(MD_SGR_BOLD)),
        "note alert must have syntaxFunction bold styling"
    );

    let warning = "> [!WARNING]\n> Be careful";
    let w_toks = markdown_tokens(warning);
    assert!(
        w_toks
            .iter()
            .any(|t| t.color == "syntaxKeyword" && t.sgr == Some(MD_SGR_BOLD)),
        "warning alert must have syntaxKeyword bold styling"
    );
}

#[test]
fn test_markdown_html_tags() {
    let html = "<summary><b>plan/Markdown Output Format.md</b> <code>(deleted: -118 +0 ~0)</code></summary>";
    let toks = markdown_tokens(html);
    let reconstructed: String = toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed, html);

    assert!(
        toks.iter()
            .any(|t| t.color == "syntaxKeyword" && t.text == "<summary>")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == "syntaxKeyword" && t.text == "<b>")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == "mdBold" && t.text == "plan/Markdown Output Format.md")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == "syntaxString" && t.text == "(deleted: -118 +0 ~0)")
    );
}

#[test]
fn test_markdown_agents_md_lines() {
    let lines = [
        "Project-wide conventions live in the root [`AGENTS.md`](../AGENTS.md); this",
        "1. **`<name>.json.gz`** (or `<name>.json`) — the frozen *input* fixture: the captured `git diff`,",
        "2. **`<name>.wire.json.gz`** (or `<name>.wire.json`) — the frozen *wire-contract output* (the",
        "3. **`<name>.render.txt.gz`** (or `<name>.render.txt`) — the frozen *terminal rendered* output.",
        "The Rust acceptance (`tests/golden.rs`) proves the engine + the wire",
    ];

    for line in lines {
        let toks = markdown_tokens(line);
        let reconstructed: String = toks.iter().map(|t| t.text).collect();
        println!("Original:      {line:?}");
        println!("Reconstructed: {reconstructed:?}");
        assert_eq!(reconstructed, line);
    }
}

#[test]
fn test_markdown_list_item_with_code() {
    let line = "1. Export `pub mod markdown;` and `pub use markdown::{MarkdownOptions, render_file_markdown, render_markdown_output};` from `diffuse-render`.";
    let toks = markdown_tokens(line);
    for t in &toks {
        eprintln!("tok: text={:?} color={:?}", t.text, t.color);
    }
}

#[test]
fn test_markdown_horizontal_rule_separator() {
    let md = "---\n";
    let toks = markdown_tokens(md);
    assert_eq!(toks.len(), 1);
    assert!(toks[0].text.starts_with("---"));
    assert_eq!(toks[0].color, "mdRule");
    assert_eq!(toks[0].sgr, Some(MD_SGR_BOLD));
}
