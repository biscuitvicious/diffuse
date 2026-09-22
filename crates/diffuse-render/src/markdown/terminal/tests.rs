//! Unit tests for terminal Markdown line assembly and reflow.

use super::*;
use crate::theme::DiffuseTheme;

fn theme() -> DiffuseTheme {
    DiffuseTheme::DEFAULT
}

/// Plain text of a rendered line (prefix excluded).
fn plain(line: &MarkdownLine) -> String {
    line.runs.iter().map(|r| r.text.to_string()).collect()
}

fn rendered(text: &str, width: usize) -> Vec<String> {
    markdown_lines(text, width)
        .iter()
        .map(|l| render_markdown_line(l, "", true, &theme()))
        .collect()
}

#[test]
fn test_emphasis_markers_absent_sgr_present() {
    let lines = rendered("**bold** and *italic* and ~~strike~~", 80);
    let joined = lines.join("\n");
    assert!(
        !joined.contains("**") && !joined.contains("~~"),
        "{joined:?}"
    );
    assert!(joined.contains("\x1b[1m"), "bold SGR in {joined:?}");
    assert!(joined.contains("\x1b[3m"), "italic SGR in {joined:?}");
    assert!(joined.contains("\x1b[9m"), "strike SGR in {joined:?}");
}

#[test]
fn test_bullet_replaces_dash_and_numbers_right_align() {
    let lines = markdown_lines("1. One\n2. Two\n10. Ten", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts[0], " 1. One");
    assert_eq!(texts[2], "10. Ten");
    let dash = markdown_lines("- item", 40);
    assert!(plain(&dash[0]).starts_with('•'));
}

#[test]
fn test_rule_exact_width_with_blanks() {
    let lines = markdown_lines("para\n\n---\n\npara", 12);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    let rule_idx = texts
        .iter()
        .position(|t| t.contains('─'))
        .expect("rule line");
    assert_eq!(texts[rule_idx], "─".repeat(12));
    assert_eq!(texts[rule_idx - 1], "");
    assert_eq!(texts[rule_idx + 1], "");
}

#[test]
fn test_heading_prefix_bold_and_blanks() {
    // A lone heading trims its forced blanks at the top level.
    let lines = markdown_lines("### Title", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts, vec!["### Title"]);
    let out = rendered("### Title", 40);
    assert!(out[0].contains("\x1b[1m"), "heading bold in {out:?}");
}

#[test]
fn test_heading_forces_blank_after() {
    let lines = markdown_lines("### Title\npara", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts[0], "### Title");
    assert_eq!(texts[1], "");
    assert_eq!(texts[2], "para");
}

#[test]
fn test_soft_wrapped_paragraph_fills_width() {
    // Hard wrap fills each line to the budget (trailing word spaces ride
    // along — same slicing semantics as the existing segment renderer).
    let lines = markdown_lines("line one\nline two\nline three", 14);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts, vec!["line one line ", "two line three"]);
}

#[test]
fn test_list_continuation_aligns_after_bullet() {
    let lines = markdown_lines("- List item which is long", 12);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(texts[0].starts_with("• "), "{texts:?}");
    for cont in &texts[1..] {
        assert!(cont.starts_with("  "), "{texts:?}");
    }
}

#[test]
fn test_heading_continuation_aligns_after_marker() {
    let lines = markdown_lines("### Really long heading here", 12);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(texts[0].starts_with("### "), "{texts:?}");
    for cont in &texts[1..] {
        assert!(cont.starts_with("    "), "{texts:?}");
    }
}

#[test]
fn test_checked_task_wraps_after_marker_chain() {
    let lines = markdown_lines("- [x] A long task item here", 12);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(texts[0].starts_with("• ☑ "), "{texts:?}");
    for cont in &texts[1..] {
        assert!(cont.starts_with("    "), "{texts:?}");
    }
    assert!(!texts.concat().contains("[x]"));
}

#[test]
fn test_fence_content_highlighted_without_markers() {
    let lines = markdown_lines("```rust\nlet x = 1;\n```", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts, vec!["let x = 1;"]);
    let out = rendered("```rust\nlet x = 1;\n```", 40).join("\n");
    assert!(!out.contains("```"), "{out:?}");
    // Fence info string is metadata, not content.
    assert!(!out.contains("rust"), "{out:?}");
}

#[test]
fn test_blockquote_bars_and_nesting() {
    let lines = markdown_lines("> quote\n>> nested", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(
        texts[0].starts_with('▎') && !texts[0].starts_with("▎▎"),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t.starts_with("▎▎")), "{texts:?}");
}

#[test]
fn test_quote_joined_blank_renders_bars_only() {
    let lines = markdown_lines("> a\n>\n> b", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts.len(), 3);
    assert_eq!(texts[1], "▎");
}

#[test]
fn test_plain_blank_separates_quotes() {
    let lines = markdown_lines("> a\n\nplain", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(texts[0].starts_with('▎'));
    assert_eq!(texts[1], "");
    assert_eq!(texts[2], "plain");
}

#[test]
fn test_loose_gaps_render_between_items() {
    let lines = markdown_lines("- one\n\n- two", 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts.len(), 3);
    assert_eq!(texts[1], "");
}

#[test]
fn test_links_render_label_and_url() {
    let out = rendered("[label](https://example.com)", 80).join("\n");
    assert!(
        out.contains("label") && out.contains("https://example.com"),
        "{out:?}"
    );
    assert!(!out.contains("[label]"), "{out:?}");
    assert!(out.contains("\x1b[4m"), "underline SGR in {out:?}");
}

#[test]
fn test_code_spans_drop_backticks() {
    let out = rendered("use `code` here", 80).join("\n");
    assert!(!out.contains('`'), "{out:?}");
    assert!(out.contains("code"), "{out:?}");
}

#[test]
fn test_cow_discipline_borrowed_prefixes_owned_splits() {
    // No wrapping: content runs copy exact source slices, prefixes borrow.
    let lines = markdown_lines("> hi\n\n- item", 80);
    let mut borrowed = 0usize;
    let mut owned_exact = 0usize;
    let src = "> hi\n\n- item";
    for line in &lines {
        for run in &line.runs {
            match &run.text {
                Cow::Borrowed(_) => borrowed += 1,
                Cow::Owned(s) => {
                    assert!(src.contains(s.as_str()), "split or rebuilt: {s:?}");
                    owned_exact += 1;
                }
            }
        }
    }
    assert!(borrowed >= 2, "bars/bullets borrow statics");
    assert!(owned_exact >= 2);
    // Hard wrap introduces Owned splits at boundaries.
    let wrapped = markdown_lines("word ".repeat(20).trim_end(), 10);
    assert!(wrapped.len() > 1);
    let total_width: usize = wrapped
        .iter()
        .map(|l| crate::width::display_width(plain(l)))
        .max()
        .unwrap_or(0);
    assert!(total_width <= 10);
}
