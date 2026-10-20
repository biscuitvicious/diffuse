//! Ports of the syntax-highlighting tests from test/review.test.ts.

use std::{
    ptr::{self, addr_eq},
    string::ToString,
};

use crate::{color::TokenColor, prelude::*};

// The semantic colors, per the TS test's theme codes.
fn color_of(c: TokenColor) -> &'static str {
    match c {
        TokenColor::Keyword => "31",
        TokenColor::Comment => "32",
        TokenColor::String => "33",
        TokenColor::Number => "34",
        TokenColor::Type => "35",
        TokenColor::Function => "36",
        TokenColor::Operator => "37",
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
            .any(|t| t.color == TokenColor::Comment && t.text == "// note"),
        "comment after code"
    );
    assert!(
        mixed
            .iter()
            .any(|t| t.color == TokenColor::Keyword && t.text == "fn"),
        "fn still a keyword"
    );
    // Hash comments only for hashComment languages (python vs rust attrs).
    let py = lang_profile("py").unwrap();
    let py_tokens = classify_tokens("# derive", Some(py));
    assert!(
        py_tokens.iter().any(|t| t.color == TokenColor::Comment),
        "py hash comment"
    );
    let rust_attr = classify_tokens("#[derive]", rs);
    assert!(
        !rust_attr.iter().any(|t| t.color == TokenColor::Comment),
        "rust # is not a comment"
    );
    // Unknown language → plain text.
    let plain = classify_tokens("anything", None);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].color, TokenColor::Default);
    assert_eq!(plain[0].text, "anything");

    // Backticks inside doc comments get accent backticks + Rust syntax tokens.
    let doc_bt = classify_tokens("/// Uses `VFS::resolve` to read.", rs);
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == TokenColor::Comment && t.text.contains("Uses"))
    );

    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == TokenColor::Delimiter && t.text == "`")
    );
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == TokenColor::Type && t.text == "VFS")
    );
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == TokenColor::Operator && t.text == ":")
    );
    assert!(
        doc_bt
            .iter()
            .any(|t| t.color == TokenColor::Comment && t.text == " to read.")
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
    v.iter().map(ToString::to_string).collect()
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
fn colors_of<'a>(text: &'a str, profile: Option<&LangProfile>) -> Vec<(TokenColor, &'a str)> {
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
    // backtick delimiters are dlm
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Delimiter && t == "`"),
        "backticks delimiter"
    );
    // inner `if` / `k` classify as Rust keyword (kwd), not comment
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Keyword && t == "if"),
        "`if` inside backticks renders as a Rust keyword"
    );
    // the `**not bold**` text outside backticks stays com — no bld
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Comment && t.contains("**not bold**")),
        "`**bold**` in a plain comment (outside backticks) stays comment-colored"
    );
    // `[not link](u)` outside backticks stays com — no url
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Comment && t.contains("[not link]")),
        "`[link](url)` in a plain comment stays comment-colored"
    );
    // no bld / ita / stk / lnk / url tokens at all
    assert!(
        toks.iter().all(|&(c, _)| !matches!(
            c,
            TokenColor::Bold
                | TokenColor::Italic
                | TokenColor::Strike
                | TokenColor::Link
                | TokenColor::Url
        )),
        "no markdown emphasis tokens in a plain comment"
    );
}

#[test]
fn doc_comment_full_markdown_renders() {
    let rs = lang_profile("rs").unwrap();
    // A `///` doc comment: `**bold**` renders as dlm `**` + bld body,
    // and `` `code` `` renders as dlm backticks + Rust inner.
    let toks = colors_of("/// **bold** and `code`", Some(rs));
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Bold && t == "bold"),
        "doc bold renders"
    );
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Delimiter && t == "`"),
        "doc backticks delimiter"
    );
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Keyword || t == "code"),
        "doc inner code classified"
    );
}

#[test]
fn test_doc_comment_table() {
    let rs = lang_profile("rs").unwrap();
    let toks = classify_line("/// | Col A | Col B |", Some(rs), None);
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|')),
        "pipes in doc comment table must be table borders"
    );
    let align_toks = classify_line("/// |---|---|", Some(rs), None);
    assert!(
        align_toks
            .iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|')),
        "pipes in doc comment alignment row must be table borders"
    );
    assert!(
        align_toks
            .iter()
            .all(|t| t.color == TokenColor::TableBorder || t.color == TokenColor::Comment),
        "alignment row must only contain table_border or comment prefix tokens"
    );
}

#[test]
fn test_doc_comment_table_with_header_and_body() {
    let rs = lang_profile("rs").unwrap();
    let header = "/// | Col A | Col B |";
    let align = "/// |---|---|";
    let body = "/// | 1 | 2 |";

    let h_toks = classify_line_with_next(header, Some(align), Some(rs), None);
    assert!(
        h_toks
            .iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|')),
        "header pipes must be table borders"
    );
    assert!(
        h_toks.iter().any(|t| t.color == TokenColor::TableHeader
            && t.sgr == Some(SGR_BOLD_UNDERLINE)
            && t.text.contains("Col A")),
        "header cells must be table_header with bold+underline"
    );

    let a_toks = classify_line_with_next(align, Some(body), Some(rs), None);
    assert!(
        a_toks
            .iter()
            .all(|t| t.color == TokenColor::TableBorder || t.color == TokenColor::Comment),
        "alignment row must be table borders"
    );

    let b_toks = classify_line_with_next(body, None, Some(rs), None);
    assert!(
        b_toks
            .iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|')),
        "body pipes must be table borders"
    );
    assert!(
        b_toks
            .iter()
            .any(|t| t.color == TokenColor::Comment && t.text.contains('1')),
        "body cells must be com"
    );
}

#[test]
fn plain_comment_without_backtick_is_uniform_comment() {
    let rs = lang_profile("rs").unwrap();
    // No backtick, no markdown triggers → the whole line is one com
    // token.
    let toks = colors_of(
        "// a plain comment with **stars** and *italics* but no backticks",
        Some(rs),
    );
    assert_eq!(toks.len(), 1, "one token");
    assert_eq!(toks[0].0, TokenColor::Comment);
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
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|')),
        "table pipes must be styled as table borders"
    );
    assert!(
        toks.iter().any(|t| {
            t.color == TokenColor::TableHeader
                && t.sgr == Some(SGR_BOLD_UNDERLINE)
                && t.text.contains("Col A")
        }),
        "table head must be styled as table_header with bold+underline"
    );
}

#[test]
fn test_markdown_table_alignment_and_inline_markup() {
    let table =
        "| **Name** | `Type` | Description |\n| :--- | :---: | ---: |\n| foo | `u32` | A number |";
    let toks = markdown_tokens(table);

    // Pipes and alignment row must be table borders
    let pipes_and_delims: Vec<_> = toks
        .iter()
        .filter(|t| t.color == TokenColor::TableBorder)
        .collect();
    assert!(
        !pipes_and_delims.is_empty(),
        "must have table_border borders and alignment row"
    );
    assert!(pipes_and_delims.iter().any(|t| t.text.contains("---")));

    // Header cells:
    assert!(toks.iter().any(|t| t.text == "Name"
        && t.color == TokenColor::Bold
        && t.sgr == Some(SGR_BOLD_UNDERLINE)));
    assert!(toks.iter().any(|t| t.text == "Type"
        && t.color == TokenColor::Code
        && t.sgr == Some(SGR_BOLD_UNDERLINE)));
    assert!(toks.iter().any(|t| t.text == "Description"
        && t.color == TokenColor::TableHeader
        && t.sgr == Some(SGR_BOLD_UNDERLINE)));

    // Body cells:
    assert!(
        toks.iter()
            .any(|t| t.text == "foo" && t.color.is_default() && t.sgr.is_none())
    );
    assert!(
        toks.iter()
            .any(|t| t.text == "A number" && t.color.is_default() && t.sgr.is_none())
    );

    // Reconstruct byte-for-byte
    let reconstructed: String = toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed, table, "byte-for-byte preservation");
}

#[test]
fn test_markdown_single_line_table_row_and_alignment_row() {
    let row = "| Col A | Col B |";
    let toks = markdown_tokens(row);
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text == "|"),
        "single-line table row must classify pipes as table borders"
    );
    let reconstructed: String = toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed, row);

    let align = "| :--- | :---: | ---: |";
    let align_toks = markdown_tokens(align);
    assert!(
        align_toks
            .iter()
            .all(|t| t.color == TokenColor::TableBorder),
        "single-line table alignment row must be entirely table borders"
    );
    let reconstructed_align: String = align_toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed_align, align);
}

#[test]
fn test_markdown_table_header_style_stacking_and_no_underlined_spaces() {
    let md = lang_profile("md").unwrap();
    let header_line = "| **Name** | `Type` | Description |";
    let next_line = "| :--- | :---: | ---: |";

    // 1. Single-line classification with next alignment line (diff hunk mode)
    let single_toks = classify_line_with_next(header_line, Some(next_line), Some(md), None);
    let reconstructed_single: String = single_toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed_single, header_line);

    // Whitespace tokens must not be underlined
    for t in &single_toks {
        if t.text.trim().is_empty() {
            assert_eq!(
                t.sgr, None,
                "whitespace token {:?} must not be underlined",
                t.text
            );
        }
    }

    // `Name` must be bld with stacked bold+underline
    let name_tok = single_toks
        .iter()
        .find(|t| t.text == "Name")
        .expect("Name token");
    assert_eq!(name_tok.color, TokenColor::Bold);
    assert_eq!(name_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    // `Type` must be code with stacked bold+underline
    let type_tok = single_toks
        .iter()
        .find(|t| t.text == "Type")
        .expect("Type token");
    assert_eq!(type_tok.color, TokenColor::Code);
    assert_eq!(type_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    // `Description` must be table_header with bold+underline
    let desc_tok = single_toks
        .iter()
        .find(|t| t.text == "Description")
        .expect("Description token");
    assert_eq!(desc_tok.color, TokenColor::TableHeader);
    assert_eq!(desc_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    // All `|` must be table borders
    for t in single_toks.iter().filter(|t| t.text == "|") {
        assert_eq!(t.color, TokenColor::TableBorder);
        assert_eq!(t.sgr, None);
    }

    // Delimiters around `**Name**` and `` `Type` `` must be bold and underlined
    let star_toks: Vec<_> = single_toks.iter().filter(|t| t.text == "**").collect();
    assert_eq!(star_toks.len(), 2, "must have 2 star tokens in single line");
    for t in star_toks {
        assert_eq!(t.color, TokenColor::Delimiter);
        assert_eq!(t.sgr, Some(SGR_BOLD_UNDERLINE));
    }

    let tick_toks: Vec<_> = single_toks.iter().filter(|t| t.text == "`").collect();
    assert_eq!(tick_toks.len(), 2, "must have 2 tick tokens in single line");
    for t in tick_toks {
        assert_eq!(t.color, TokenColor::Delimiter);
        assert_eq!(t.sgr, Some(SGR_BOLD_UNDERLINE));
    }

    // 2. Multi-line document classification (editor mode)
    let doc = format!("{header_line}\n{next_line}\n| foo | `u32` | desc |");
    let doc_lines = markdown_document_tokens(&doc);
    let doc_header_toks = &doc_lines[0];
    let reconstructed_doc: String = doc_header_toks.iter().map(|t| t.text).collect();
    assert_eq!(reconstructed_doc, header_line);

    for t in doc_header_toks {
        if t.text.trim().is_empty() {
            assert_eq!(
                t.sgr, None,
                "doc whitespace token {:?} must not be underlined",
                t.text
            );
        }
    }

    let doc_name_tok = doc_header_toks
        .iter()
        .find(|t| t.text == "Name")
        .expect("Name token");
    assert_eq!(doc_name_tok.color, TokenColor::Bold);
    assert_eq!(doc_name_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    let doc_type_tok = doc_header_toks
        .iter()
        .find(|t| t.text == "Type")
        .expect("Type token");
    assert_eq!(doc_type_tok.color, TokenColor::Code);
    assert_eq!(doc_type_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    let doc_desc_tok = doc_header_toks
        .iter()
        .find(|t| t.text == "Description")
        .expect("Description token");
    assert_eq!(doc_desc_tok.color, TokenColor::TableHeader);
    assert_eq!(doc_desc_tok.sgr, Some(SGR_BOLD_UNDERLINE));

    let doc_star_toks: Vec<_> = doc_header_toks.iter().filter(|t| t.text == "**").collect();
    assert_eq!(
        doc_star_toks.len(),
        2,
        "must have 2 star tokens in doc header"
    );
    for t in doc_star_toks {
        assert_eq!(t.color, TokenColor::Delimiter);
        assert_eq!(t.sgr, Some(SGR_BOLD_UNDERLINE));
    }

    let doc_tick_toks: Vec<_> = doc_header_toks.iter().filter(|t| t.text == "`").collect();
    assert_eq!(
        doc_tick_toks.len(),
        2,
        "must have 2 tick tokens in doc header"
    );
    for t in doc_tick_toks {
        assert_eq!(t.color, TokenColor::Delimiter);
        assert_eq!(t.sgr, Some(SGR_BOLD_UNDERLINE));
    }
}

#[test]
fn test_markdown_document_tokens() {
    let doc = "# Header\n\n| Col A | Col B |\n|---|---|\n| 1 | 2 |";
    let lines = markdown_document_tokens(doc);
    assert_eq!(lines.len(), 5);
    // Line 0: Header
    assert!(
        lines[0]
            .iter()
            .any(|t| t.color == TokenColor::Delimiter && t.text == "#")
    );
    assert!(lines[0].iter().any(|t| t.color == TokenColor::Delimiter
        && t.sgr == Some(SGR_BOLD)
        && t.text == "Header"));
    // Line 1: Empty
    assert!(lines[1].is_empty());
    // Line 2: Table Head
    assert!(
        lines[2]
            .iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|'))
    );
    assert!(lines[2].iter().any(|t| t.color == TokenColor::TableHeader
        && t.sgr == Some(SGR_BOLD_UNDERLINE)
        && t.text == "Col A"));
    // Line 3: Table Delimiter
    assert!(lines[3].iter().all(|t| t.color == TokenColor::TableBorder));
    // Line 4: Table Body
    assert!(
        lines[4]
            .iter()
            .any(|t| t.color == TokenColor::TableBorder && t.text.contains('|'))
    );
    assert!(
        lines[4]
            .iter()
            .any(|t| t.color.is_default() && t.text == "1")
    );
    assert!(
        lines[4]
            .iter()
            .all(|t| t.color == TokenColor::TableBorder || t.color.is_default())
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
            .any(|t| t.color == TokenColor::Delimiter && t.text.contains('>')),
        "blockquote chevron must be delimiter"
    );
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::Function && t.sgr == Some(SGR_BOLD)),
        "note alert must have fun bold styling"
    );

    let warning = "> [!WARNING]\n> Be careful";
    let w_toks = markdown_tokens(warning);
    assert!(
        w_toks
            .iter()
            .any(|t| t.color == TokenColor::Keyword && t.sgr == Some(SGR_BOLD)),
        "warning alert must have kwd bold styling"
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
            .any(|t| t.color == TokenColor::Keyword && t.text == "<summary>")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::Keyword && t.text == "<b>")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::Bold && t.text == "plan/Markdown Output Format.md")
    );
    assert!(
        toks.iter()
            .any(|t| t.color == TokenColor::Code && t.text == "(deleted: -118 +0 ~0)")
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
    assert_eq!(toks[0].color, TokenColor::Rule);
    assert_eq!(toks[0].sgr, Some(SGR_BOLD));
}

#[test]
fn test_all_profiles_operators_sorted() {
    for prof in all_registered_profiles() {
        let mut sorted = prof.operators.to_vec();
        sorted.sort();
        assert_eq!(
            prof.operators,
            sorted.as_slice(),
            "operators in profile `{}` not sorted. Expected:\n{:?}",
            prof.name,
            sorted
        );
        for &op in prof.operators {
            assert!(
                prof.is_operator_anchor(op),
                "is_operator_anchor failed for `{op}` in profile `{}`",
                prof.name
            );
        }
        assert!(
            !prof.is_operator_anchor("not_an_operator"),
            "non-operator returned true for profile `{}`",
            prof.name
        );
    }
}

#[test]
fn test_registered_extensions_resolve() {
    for &ext in REGISTERED_EXTENSIONS {
        assert!(
            lang_profile(ext).is_some(),
            "registered extension `{ext}` must resolve to a valid LangProfile"
        );
    }
}

#[test]
fn test_first_word() {
    assert_eq!(first_word("let x = 1;"), Some("let"));
    assert_eq!(first_word("    fn main()"), Some("fn"));
    assert_eq!(first_word("//! comment"), None);
    assert_eq!(first_word(".map()"), None);
    assert_eq!(first_word("+ x"), None);
    assert_eq!(first_word("   "), None);
    assert_eq!(first_word("!!!"), None);
}

#[test]
fn test_unicode_word_primitives_and_classification() {
    // 1. Unicode word character recognition
    assert!(is_word_char('a'));
    assert!(is_word_char('_'));
    assert!(is_word_char('1'));
    assert!(is_word_char('é'));
    assert!(is_word_char('α'));
    assert!(is_word_char('Ж'));
    assert!(is_word_char('日'));
    assert!(!is_word_char(' '));
    assert!(!is_word_char(';'));
    assert!(!is_word_char('+'));

    // 2. Unicode word prefix and first_word extraction
    assert_eq!(word_prefix("résumé_count + 1"), "résumé_count");
    assert_eq!(word_prefix("über_42;"), "über_42");
    assert_eq!(word_prefix("αριθμός_1()"), "αριθμός_1");
    assert_eq!(word_prefix("пользователь_id"), "пользователь_id");
    assert_eq!(word_prefix("ユーザー名 = val"), "ユーザー名");
    assert_eq!(first_word("   const résumé = 10;"), Some("const"));
    assert_eq!(first_word("   résumé_total"), Some("résumé_total"));
    assert_eq!(first_word("   // comment"), None);
    assert_eq!(first_word("   "), None);

    // 3. Hand-rolled token scanner preserves full multi-byte identifiers
    let rs = lang_profile("rs");
    let tokens = classify_tokens("let résumé = 42;", rs);
    assert!(
        tokens
            .iter()
            .any(|t| t.text == "résumé" && t.color == TokenColor::Default),
        "résumé should be scanned as single identifier token: {tokens:?}"
    );

    let tokens_greek = classify_tokens("let μέγιστο = 100;", rs);
    assert!(
        tokens_greek
            .iter()
            .any(|t| t.text == "μέγιστο" && t.color == TokenColor::Default),
        "μέγιστο should be scanned as single identifier token: {tokens_greek:?}"
    );

    let tokens_cjk = classify_tokens("let ユーザー_id = 1;", rs);
    assert!(
        tokens_cjk
            .iter()
            .any(|t| t.text == "ユーザー_id" && t.color == TokenColor::Default),
        "ユーザー_id should be scanned as single identifier token: {tokens_cjk:?}"
    );

    // 4. PascalCase types across Unicode uppercase
    let tokens_type = classify_tokens("let x: RésuméType = val;", rs);
    assert!(
        tokens_type
            .iter()
            .any(|t| t.text == "RésuméType" && t.color == TokenColor::Type),
        "RésuméType should be classified as TokenColor::Type: {tokens_type:?}"
    );

    let tokens_cyrillic_type = classify_tokens("let s: Счетчик = val;", rs);
    assert!(
        tokens_cyrillic_type
            .iter()
            .any(|t| t.text == "Счетчик" && t.color == TokenColor::Type),
        "Счетчик should be classified as TokenColor::Type: {tokens_cyrillic_type:?}"
    );
}

#[test]
fn test_close_delimiter_and_profile_syntax_matching() {
    let rs = lang_profile("rs").expect("rust profile");
    let py = lang_profile("py").expect("python profile");
    let sh = lang_profile("sh").expect("shell profile");
    let c = lang_profile("c").expect("c profile");

    // Rust / Braces closing tails
    assert_eq!(
        rs.classify_close_tail("    };"),
        crate::profile::CloseDelimiter::BraceSemi
    );
    assert_eq!(
        rs.classify_close_tail("    },"),
        crate::profile::CloseDelimiter::BraceSemi
    );
    assert_eq!(
        rs.classify_close_tail("    }"),
        crate::profile::CloseDelimiter::Brace
    );
    assert_eq!(
        rs.classify_close_tail("    );"),
        crate::profile::CloseDelimiter::ParenSemi
    );
    assert_eq!(
        rs.classify_close_tail("    ),"),
        crate::profile::CloseDelimiter::ParenSemi
    );
    assert_eq!(
        rs.classify_close_tail("    )"),
        crate::profile::CloseDelimiter::Paren
    );
    assert_eq!(
        rs.classify_close_tail("    ];"),
        crate::profile::CloseDelimiter::BracketSemi
    );
    assert_eq!(
        rs.classify_close_tail("    ],"),
        crate::profile::CloseDelimiter::BracketSemi
    );
    assert_eq!(
        rs.classify_close_tail("    ]"),
        crate::profile::CloseDelimiter::Bracket
    );
    assert_eq!(
        rs.classify_close_tail("    let x = 1;"),
        crate::profile::CloseDelimiter::None
    );

    // Exact close
    assert_eq!(
        rs.classify_exact_close("};"),
        crate::profile::CloseDelimiter::BraceSemi
    );
    assert_eq!(
        rs.classify_exact_close("}"),
        crate::profile::CloseDelimiter::Brace
    );
    assert_eq!(
        rs.classify_exact_close(");"),
        crate::profile::CloseDelimiter::ParenSemi
    );
    assert_eq!(
        rs.classify_exact_close(")"),
        crate::profile::CloseDelimiter::Paren
    );
    assert_eq!(
        rs.classify_exact_close("let x = 1;"),
        crate::profile::CloseDelimiter::None
    );

    // Python / IndentColon
    assert_eq!(
        py.classify_close_tail("    )"),
        crate::profile::CloseDelimiter::Paren
    );
    assert_eq!(
        py.classify_close_tail("    ]"),
        crate::profile::CloseDelimiter::Bracket
    );
    assert_eq!(
        py.classify_close_tail("    }"),
        crate::profile::CloseDelimiter::Brace
    );
    assert_eq!(
        py.classify_close_tail("    pass"),
        crate::profile::CloseDelimiter::None
    );

    // Shell / Keywords
    assert_eq!(
        sh.classify_close_tail("fi"),
        crate::profile::CloseDelimiter::Keyword
    );
    assert_eq!(
        sh.classify_close_tail("done"),
        crate::profile::CloseDelimiter::Keyword
    );
    assert_eq!(
        sh.classify_close_tail("esac"),
        crate::profile::CloseDelimiter::Keyword
    );
    assert_eq!(
        sh.classify_close_tail("}"),
        crate::profile::CloseDelimiter::Brace
    );
    assert_eq!(
        sh.classify_close_tail("echo hello"),
        crate::profile::CloseDelimiter::None
    );

    // Direct tail match across indentation shifts parameterized with min_chars
    assert!(rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::ParenSemi,
        crate::profile::CloseDelimiter::ParenSemi,
        10,
        8
    ));
    assert!(!rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::ParenSemi,
        crate::profile::CloseDelimiter::BraceSemi,
        10,
        8
    ));
    assert!(!rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::None,
        crate::profile::CloseDelimiter::None,
        10,
        8
    ));
    assert!(!rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::ParenSemi,
        crate::profile::CloseDelimiter::ParenSemi,
        5,
        8
    ));
    // Test custom min_chars threshold
    assert!(rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::ParenSemi,
        crate::profile::CloseDelimiter::ParenSemi,
        5,
        4
    ));
    assert!(!rs.is_direct_tail_match(
        crate::profile::CloseDelimiter::ParenSemi,
        crate::profile::CloseDelimiter::ParenSemi,
        5,
        6
    ));

    // Python only allows expressions across indentation shifts, not block
    // delimiters
    assert!(py.is_direct_tail_match(
        crate::profile::CloseDelimiter::Paren,
        crate::profile::CloseDelimiter::Paren,
        10,
        8
    ));
    assert!(!py.is_direct_tail_match(
        crate::profile::CloseDelimiter::Brace,
        crate::profile::CloseDelimiter::Brace,
        10,
        8
    ));

    // Shell allows keywords, paren, bracket
    assert!(sh.is_direct_tail_match(
        crate::profile::CloseDelimiter::Keyword,
        crate::profile::CloseDelimiter::Keyword,
        10,
        8
    ));
    assert!(sh.is_direct_tail_match(
        crate::profile::CloseDelimiter::Paren,
        crate::profile::CloseDelimiter::Paren,
        10,
        8
    ));
    assert!(sh.is_direct_tail_match(
        crate::profile::CloseDelimiter::Bracket,
        crate::profile::CloseDelimiter::Bracket,
        10,
        8
    ));
    assert!(!sh.is_direct_tail_match(
        crate::profile::CloseDelimiter::Brace,
        crate::profile::CloseDelimiter::Brace,
        10,
        8
    ));

    // Default methods on CloseDelimiter
    assert!(
        crate::profile::CloseDelimiter::default_is_direct_tail_match(
            crate::profile::CloseDelimiter::ParenSemi,
            crate::profile::CloseDelimiter::ParenSemi,
            10,
            8
        )
    );
    assert!(
        !crate::profile::CloseDelimiter::default_is_direct_tail_match(
            crate::profile::CloseDelimiter::None,
            crate::profile::CloseDelimiter::None,
            10,
            8
        )
    );
    assert!(
        !crate::profile::CloseDelimiter::default_is_direct_tail_match(
            crate::profile::CloseDelimiter::ParenSemi,
            crate::profile::CloseDelimiter::ParenSemi,
            5,
            8
        )
    );

    // Exact close and direct tail for keywords / indent languages
    assert_eq!(
        sh.classify_exact_close("fi"),
        crate::profile::CloseDelimiter::Keyword
    );
    assert_eq!(
        py.classify_exact_close(")"),
        crate::profile::CloseDelimiter::Paren
    );

    // Declarative Delimiter Tables
    assert!(!rs.exact_delimiters.is_empty());
    assert!(!rs.tail_delimiters.is_empty());
    assert!(!rs.trailing_delimiters.is_empty());
    assert!(rs.trailing_delimiters.contains(&';'));
    assert!(c.trailing_delimiters.contains(&';'));
    assert!(c.is_direct_tail_match(
        crate::profile::CloseDelimiter::BraceSemi,
        crate::profile::CloseDelimiter::BraceSemi,
        10,
        8
    ));
    assert!(py.trailing_delimiters.contains(&':'));
    assert!(!py.trailing_delimiters.contains(&';'));

    // Trailing delimiter trimming
    assert_eq!(
        rs.trim_trailing_delimiters("if !run.is_empty() {"),
        "if !run.is_empty("
    );
    assert_eq!(rs.trim_trailing_delimiters("let x = 1;"), "let x = 1");
    assert_eq!(rs.trim_trailing_delimiters("use std::{"), "use std::");
    assert_eq!(rs.trim_trailing_delimiters("use std::"), "use std::");
    assert_eq!(py.trim_trailing_delimiters("def foo():"), "def foo(");
}

#[test]
fn test_continuation_phrase_profile_and_default() {
    let rs = lang_profile("rs").expect("rust profile");

    // Rust profile continuation phrases
    assert!(rs.is_continuation_phrase("=> {"));
    assert!(rs.is_continuation_phrase(" => {"));
    assert!(rs.is_continuation_phrase("-> {"));
    assert!(rs.is_continuation_phrase(") {"));
    assert!(rs.is_continuation_phrase("] {"));
    assert!(rs.is_continuation_phrase("=>"));
    assert!(rs.is_continuation_phrase("->"));

    // Unicode variants
    assert!(rs.is_continuation_phrase("=> ｛"));
    assert!(rs.is_continuation_phrase("） ｛"));

    // Negative tests: delimiters alone or with commas are NOT continuation phrases
    assert!(!rs.is_continuation_phrase("}"));
    assert!(!rs.is_continuation_phrase(")"));
    assert!(!rs.is_continuation_phrase("};"));
    assert!(!rs.is_continuation_phrase("),"));
    assert!(!rs.is_continuation_phrase("],"));
    assert!(!rs.is_continuation_phrase("let"));
    assert!(!rs.is_continuation_phrase("foo"));
    assert!(!rs.is_continuation_phrase("\"    let y = 2;\""));
    assert!(!rs.is_continuation_phrase("| "));
    assert!(!rs.is_continuation_phrase("::"));
    assert!(!rs.is_continuation_phrase(""));

    // Universal default fallback (no profile)
    use crate::profile::default_is_continuation_phrase;
    assert!(default_is_continuation_phrase("=> {"));
    assert!(default_is_continuation_phrase(") {"));
    assert!(default_is_continuation_phrase("=> ｛"));
    assert!(!default_is_continuation_phrase("};"));
    assert!(!default_is_continuation_phrase("),"));
    assert!(!default_is_continuation_phrase("let"));
    assert!(!default_is_continuation_phrase("foo"));
}

#[test]
fn test_json_and_jsonl_syntax_profile() {
    let json_prof = lang_profile("json").expect("json profile");
    let jsonl_prof = lang_profile("jsonl").expect("jsonl profile");
    assert!(
        ptr::eq(json_prof, jsonl_prof),
        "json and jsonl must share the same canonical profile instance"
    );

    // Fence tag normalization
    assert_eq!(fence_lang_ext("json"), "json");
    assert_eq!(fence_lang_ext("jsonl"), "json");

    // Registered extensions
    assert!(REGISTERED_EXTENSIONS.contains(&"json"));
    assert!(REGISTERED_EXTENSIONS.contains(&"jsonl"));

    // Operator anchor
    assert!(json_prof.is_operator_anchor(":"));
    assert!(!json_prof.is_operator_anchor("="));

    // Keyword detection
    assert!(json_prof.is_keyword("true"));
    assert!(json_prof.is_keyword("false"));
    assert!(json_prof.is_keyword("null"));
    assert!(!json_prof.is_keyword("undefined"));

    // Closing delimiter classification
    assert_eq!(
        json_prof.classify_close_tail("    },"),
        crate::profile::CloseDelimiter::BraceSemi
    );
    assert_eq!(
        json_prof.classify_close_tail("    }"),
        crate::profile::CloseDelimiter::Brace
    );
    assert_eq!(
        json_prof.classify_close_tail("    ],"),
        crate::profile::CloseDelimiter::BracketSemi
    );
    assert_eq!(
        json_prof.classify_close_tail("    ]"),
        crate::profile::CloseDelimiter::Bracket
    );

    // Token classification for JSON
    let multiline_json = r#"  "enabled": true,"#;
    let tokens = classify_tokens(multiline_json, Some(json_prof));
    assert!(
        tokens
            .iter()
            .any(|t| t.color == TokenColor::String && t.text == "\"enabled\""),
        "property name must be str"
    );
    assert!(
        tokens
            .iter()
            .any(|t| t.color == TokenColor::Keyword && t.text == "true"),
        "boolean literal must be kwd"
    );

    // Token classification for JSONL row
    let jsonl_line = r#"{"id": 42, "active": false, "data": null, "name": "diffuse"}"#;
    let tokens_l = classify_tokens(jsonl_line, Some(jsonl_prof));
    assert!(
        tokens_l
            .iter()
            .any(|t| t.color == TokenColor::Number && t.text == "42"),
        "number must be snb"
    );
    assert!(
        tokens_l
            .iter()
            .any(|t| t.color == TokenColor::Keyword && t.text == "false"),
        "false must be kwd"
    );
    assert!(
        tokens_l
            .iter()
            .any(|t| t.color == TokenColor::Keyword && t.text == "null"),
        "null must be kwd"
    );
    assert!(
        tokens_l
            .iter()
            .any(|t| t.color == TokenColor::String && t.text == "\"diffuse\""),
        "string must be str"
    );
    assert!(
        tokens_l
            .iter()
            .any(|t| t.color == TokenColor::Operator && t.text == ":"),
        "colon must be opr"
    );
}

#[test]
fn test_extract_property_key() {
    let json = lang_profile("json").unwrap();
    assert_eq!(
        json.extract_property_key("\"text\": \"hello\""),
        Some("text")
    );
    assert_eq!(
        json.extract_property_key("  \"kind\": \"addcommon\","),
        Some("kind")
    );
    assert_eq!(
        json.extract_property_key("\"color\": \"text\""),
        Some("color")
    );
    assert_eq!(json.extract_property_key("}"), None);

    let toml = lang_profile("toml").unwrap();
    assert_eq!(
        toml.extract_property_key("version = \"0.1.0\""),
        Some("version")
    );
    assert_eq!(
        toml.extract_property_key("diffuse-core = { path = \"...\" }"),
        Some("diffuse-core")
    );
    assert_eq!(toml.extract_property_key("[package]"), None);

    let yaml = lang_profile("yaml").unwrap();
    assert_eq!(yaml.extract_property_key("name: diffuse"), Some("name"));

    let rust = lang_profile("rs").unwrap();
    assert_eq!(rust.extract_property_key("pub text: String,"), Some("text"));
    assert_eq!(
        rust.extract_property_key("pub(crate) score: f64,"),
        Some("score")
    );
    assert_eq!(rust.extract_property_key("field: 123,"), Some("field"));
    assert_eq!(rust.extract_property_key("let x = 5;"), None);

    let c = lang_profile("c").unwrap();
    assert_eq!(c.extract_property_key("int x = 5;"), None);
}

#[test]
fn test_terminal_return_expression_rules() {
    let rs = lang_profile("rs").unwrap();
    // In Rust, expressions without trailing semicolons are terminal returns
    assert!(rs.is_terminal_return_expr("None"));
    assert!(rs.is_terminal_return_expr("Some(result)"));
    assert!(rs.is_terminal_return_expr("return 42;"));
    assert!(rs.is_terminal_return_expr("    return None;"));
    // With trailing semicolon in Rust, it is a statement returning (), NOT a
    // terminal return value
    assert!(!rs.is_terminal_return_expr("None;"));
    assert!(!rs.is_terminal_return_expr("let x = 1;"));
    assert!(!rs.is_terminal_return_expr("alpha();"));
    assert!(!rs.is_terminal_return_expr("}"));
    assert!(!rs.is_terminal_return_expr("})"));
    assert!(!rs.is_terminal_return_expr("});"));
    assert!(!rs.is_terminal_return_expr("}),"));
    assert!(!rs.is_terminal_return_expr(")"));
    assert!(!rs.is_terminal_return_expr(");"));
    assert!(!rs.is_terminal_return_expr("]"));
    assert!(!rs.is_terminal_return_expr("]);"));
    assert!(!rs.is_terminal_return_expr(""));

    let c = lang_profile("c").unwrap();
    // In C, returns must be explicit
    assert!(c.is_terminal_return_expr("return 42;"));
    assert!(c.is_terminal_return_expr("return;"));
    assert!(!c.is_terminal_return_expr("None"));
    assert!(!c.is_terminal_return_expr("42;"));
    assert!(!c.is_terminal_return_expr("int x = 1;"));
}

#[test]
fn test_language_syntax_attributes_and_bypasses() {
    let rs = lang_profile("rs").unwrap();
    let py = lang_profile("py").unwrap();
    let ts = lang_profile("ts").unwrap();
    let c = lang_profile("c").unwrap();
    let go = lang_profile("go").unwrap();
    let sh = lang_profile("sh").unwrap();
    let toml = lang_profile("toml").unwrap();

    // Attribute lines
    assert!(rs.is_attribute_line("#[derive(Clone)]"));
    assert!(rs.is_attribute_line("#![no_std]"));
    assert!(!rs.is_attribute_line("let x = 5;"));
    assert!(py.is_attribute_line("@dataclass"));
    assert!(py.is_attribute_line("    @property"));
    assert!(!py.is_attribute_line("def foo():"));
    assert!(ts.is_attribute_line("@Injectable()"));
    assert!(c.is_attribute_line("[[nodiscard]]"));
    assert!(go.is_attribute_line("//go:noinline"));

    // Continuation lines
    assert!(rs.is_continuation_line("&& is_valid"));
    assert!(rs.is_continuation_line("|| is_other"));
    assert!(rs.is_continuation_line(".filter(|x| x > 0)"));
    assert!(py.is_continuation_line("and is_valid"));
    assert!(py.is_continuation_line("or is_other"));
    assert!(py.is_continuation_line("\\"));
    assert!(sh.is_continuation_line("| grep foo"));

    // Import statements
    assert!(py.is_import_statement("import sys"));
    assert!(py.is_import_statement("from os import path"));
    assert!(go.is_import_statement("import \"fmt\""));
    assert!(rs.is_import_statement("use std::sync::Arc;"));
    assert!(rs.is_import_statement("pub use std::path::Path;"));
    assert!(sh.is_import_statement("source ./env.sh"));

    // Expression open
    assert!(rs.is_expression_open("{"));
    assert!(rs.is_expression_open("("));
    assert!(rs.is_expression_open("["));
    assert!(!rs.is_expression_open("let x = 1;"));

    // Section headers
    assert!(toml.section_headers);
    assert!(!rs.section_headers);
    assert!(!py.section_headers);

    // Decl break with attributes and comments
    assert!(rs.is_decl_break("/// Doc comment"));
    assert!(rs.is_decl_break("#[derive(Debug)]"));
    assert!(py.is_decl_break("@dataclass"));
    assert!(py.is_decl_break("# A python comment"));
}

#[test]
fn test_quote_and_fence_border_constants() {
    assert_eq!(TokenColor::QuoteBorder.wire_code(), Some("qbo"));
    assert_eq!(TokenColor::FenceBorder.wire_code(), Some("fbo"));
}

#[test]
fn test_rust_lifetimes_and_char_literals() {
    let rs = lang_profile("rs").unwrap();

    // 1. Rust lifetimes must NOT parse as string or char literal!
    // The `'` should be operator/dlm and `a` should be identifier.
    let toks = colors_of("&'a str", Some(rs));
    assert!(
        toks.iter()
            .all(|&(c, _)| c != TokenColor::String && c != TokenColor::CharLiteral),
        "lifetime &'a must not parse as string or char literal: got {toks:?}"
    );
    assert!(
        toks.iter()
            .any(|&(c, t)| c == TokenColor::Default && t == "a"),
        "lifetime identifier 'a' should be classified as identifier/default: got {toks:?}"
    );

    let toks_static = colors_of("&'static str", Some(rs));
    assert!(
        toks_static
            .iter()
            .all(|&(c, _)| c != TokenColor::String && c != TokenColor::CharLiteral),
        "lifetime &'static must not parse as string or char literal: got {toks_static:?}"
    );
    assert!(
        toks_static
            .iter()
            .any(|&(c, t)| c == TokenColor::Keyword && t == "static"),
        "lifetime 'static' should classify 'static' as keyword: got {toks_static:?}"
    );

    // 2. Valid Rust char literals
    for valid in &[
        "'a'",
        r"'\''",
        r"'\\'",
        r"'\n'",
        r"'\t'",
        r"'\0'",
        r"'\x7f'",
        r"'\u{1F980}'",
        "'🦀'",
    ] {
        let toks_char = colors_of(valid, Some(rs));
        assert_eq!(
            toks_char,
            vec![(TokenColor::CharLiteral, *valid)],
            "char literal {valid} should parse as TokenColor::CharLiteral"
        );
    }

    // 3. Invalid Rust char literals: multi-char strings or invalid escapes
    for invalid in &["'abc'", "''", r"'\x80'", r"'\u{110000}'"] {
        let toks_inv = colors_of(invalid, Some(rs));
        assert!(
            toks_inv
                .iter()
                .all(|&(c, _)| c != TokenColor::CharLiteral && c != TokenColor::String),
            "invalid char literal {invalid} should not parse as TokenColor::CharLiteral or String: got {toks_inv:?}"
        );
    }

    // 4. Languages with single-quote strings (e.g. Python, JS) must still parse
    //    'abc' as String
    let py = lang_profile("py").unwrap();
    let toks_py = colors_of("'hello'", Some(py));
    assert_eq!(
        toks_py,
        vec![(TokenColor::String, "'hello'")],
        "python single-quoted string should remain TokenColor::String"
    );

    // 5. C-family char literals (e.g. octal \177, \xFF)
    let c = lang_profile("c").unwrap();
    let toks_c = colors_of(r"'\xFF'", Some(c));
    assert_eq!(toks_c, vec![(TokenColor::CharLiteral, r"'\xFF'")]);
    let toks_c_inv = colors_of("'hello'", Some(c));
    assert!(
        toks_c_inv
            .iter()
            .all(|&(col, _)| col != TokenColor::CharLiteral && col != TokenColor::String),
        "multi-char 'hello' in C should not parse as CharLiteral or String"
    );

    // 6. Go rune literals
    let go = lang_profile("go").unwrap();
    let toks_go = colors_of(r"'\u0041'", Some(go));
    assert_eq!(toks_go, vec![(TokenColor::CharLiteral, r"'\u0041'")]);

    // 7. JSON does not support single quotes at all
    let json = lang_profile("json").unwrap();
    let toks_json = colors_of("'hello'", Some(json));
    assert!(
        toks_json
            .iter()
            .all(|&(col, _)| col != TokenColor::String && col != TokenColor::CharLiteral),
        "JSON should not parse single quotes as String or CharLiteral"
    );
}

#[test]
fn test_is_statement_terminator() {
    let rs = lang_profile("rs").unwrap();
    assert!(rs.is_statement_terminator(";"));
    assert!(rs.is_statement_terminator(");"));
    assert!(rs.is_statement_terminator("};"));
    assert!(rs.is_statement_terminator("];"));
    assert!(rs.is_statement_terminator(")"));
    assert!(rs.is_statement_terminator("}"));
    assert!(rs.is_statement_terminator("]"));
    assert!(!rs.is_statement_terminator("let x = 1"));
    assert!(!rs.is_statement_terminator(","));
}

#[test]
fn test_is_opening_delimiter() {
    let rs = lang_profile("rs").unwrap();
    assert!(rs.is_opening_delimiter("("));
    assert!(rs.is_opening_delimiter("{"));
    assert!(rs.is_opening_delimiter("["));
    assert!(!rs.is_opening_delimiter(";"));
    assert!(!rs.is_opening_delimiter("foo"));

    // Unicode opening delimiters
    assert!(rs.is_opening_delimiter("（"));
    assert!(rs.is_opening_delimiter("【"));
    assert!(rs.is_opening_delimiter("«"));
    assert!(rs.is_opening_delimiter("“"));
    assert!(rs.is_opening_delimiter("⟦"));
}

#[test]
fn test_is_closing_delimiter_unicode() {
    let rs = lang_profile("rs").unwrap();
    assert!(rs.is_closing_delimiter(")"));
    assert!(rs.is_closing_delimiter("}"));
    assert!(rs.is_closing_delimiter("]"));
    assert!(rs.is_closing_delimiter(";"));

    // Unicode closing delimiters & statement terminators
    assert!(rs.is_closing_delimiter("）"));
    assert!(rs.is_closing_delimiter("】"));
    assert!(rs.is_closing_delimiter("»"));
    assert!(rs.is_closing_delimiter("”"));
    assert!(rs.is_closing_delimiter("⟧"));
    assert!(rs.is_closing_delimiter("；"));
    assert!(rs.is_statement_terminator("；"));
}
