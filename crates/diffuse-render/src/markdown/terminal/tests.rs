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
    assert!(
        out[0].contains("\x1b[1;4m"),
        "heading bold+underline in {out:?}"
    );
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
    assert!(texts[0].starts_with("• ✅ "), "{texts:?}");
    for cont in &texts[1..] {
        assert!(cont.starts_with("     "), "{texts:?}");
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

#[test]
fn test_heading_title_bold_and_underlined_markers_not() {
    let lines = markdown_lines("### Title here", 40);
    assert_eq!(lines.len(), 1);
    let out = render_markdown_line(&lines[0], "", true, &theme());
    // Stacked bold + underline on the title text …
    assert!(out.contains("\x1b[1;4m"), "{out:?}");
    // … but the ### marker run carries no SGR.
    let marker_end = out.find("###").expect("marker present");
    assert!(!out[..marker_end].contains("\x1b[1;4m"), "{out:?}");
}

#[test]
fn test_link_inside_heading_dedupes_underline() {
    let lines = markdown_lines("### See [docs](https://example.com) now", 80);
    let out = lines
        .iter()
        .map(|l| render_markdown_line(l, "", true, &theme()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!out.contains("4;4"), "no doubled underline: {out:?}");
    assert!(out.contains("https://example.com"), "{out:?}");
}

#[test]
fn test_task_list_checkbox_glyphs_mapping() {
    let cases = [
        ("- [ ] empty task", "• ⬜ empty task"),
        ("- [·] in progress task", "• 🔄 in progress task"),
        ("- [x] checked task", "• ✅ checked task"),
        (
            "- [X] uppercase checked task",
            "• ✅ uppercase checked task",
        ),
    ];
    for (src, want) in cases {
        let lines = markdown_lines(src, 40);
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts, vec![want], "source {src:?} must render as {want:?}");
        let rendered_text = rendered(src, 40).join("\n");
        assert!(
            !rendered_text.contains("[ ]")
                && !rendered_text.contains("[·]")
                && !rendered_text.contains("[x]")
                && !rendered_text.contains("[X]"),
            "bracket source must not appear in rendered output: {rendered_text:?}"
        );
    }
}

#[test]
fn test_mixed_task_list_renders_in_source_order() {
    let src = "\
- [ ] First todo
- [·] Second in progress
- [x] Third done
- [X] Fourth capital done
- Fifth normal bullet item";

    let lines = markdown_lines(src, 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        texts,
        vec![
            "• ⬜ First todo",
            "• 🔄 Second in progress",
            "• ✅ Third done",
            "• ✅ Fourth capital done",
            "• Fifth normal bullet item",
        ]
    );

    let joined = texts.concat();
    assert!(!joined.contains("[ ]"));
    assert!(!joined.contains("[·]"));
    assert!(!joined.contains("[x]"));
    assert!(!joined.contains("[X]"));
}

#[test]
fn test_task_list_continuation_indent_for_all_states() {
    let items = [
        (
            "- [ ] A long empty task that wraps across multiple lines",
            "• ⬜ ",
        ),
        (
            "- [·] A long progress task that wraps across multiple lines",
            "• 🔄 ",
        ),
        (
            "- [x] A long checked task that wraps across multiple lines",
            "• ✅ ",
        ),
    ];

    for (src, prefix) in items {
        let lines = markdown_lines(src, 16);
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert!(texts.len() > 1, "must wrap into multiple lines: {texts:?}");
        assert!(
            texts[0].starts_with(prefix),
            "first line must start with {prefix:?}: {:?}",
            texts[0]
        );
        for (i, cont) in texts[1..].iter().enumerate() {
            assert!(
                cont.starts_with("     "),
                "continuation line {i} must indent 5 spaces past marker chain (bullet + 2-col glyph + space): {cont:?}"
            );
        }
    }
}

#[test]
fn test_ordered_task_list_glyphs_and_alignment() {
    let src = "\
1. [ ] First ordered
2. [·] Second ordered
10. [x] Tenth ordered";

    let lines = markdown_lines(src, 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        texts,
        vec![
            " 1. ⬜ First ordered",
            " 2. 🔄 Second ordered",
            "10. ✅ Tenth ordered",
        ]
    );

    // Numbered task list wrapping
    let wrap_src = "10. [x] A very long task line that should wrap";
    let wrap_lines = markdown_lines(wrap_src, 18);
    let wrap_texts: Vec<String> = wrap_lines.iter().map(plain).collect();
    assert!(wrap_texts[0].starts_with("10. ✅ "));
    for cont in &wrap_texts[1..] {
        // "10. " is 4 cols + "✅ " is 3 cols = 7 cols indent
        assert!(
            cont.starts_with("       "),
            "ordered task continuation must indent 7 spaces: {cont:?}"
        );
    }
}

#[test]
fn test_task_list_with_inline_formatting() {
    let src = "- [x] **Bold** task with `code` and *italic* and ~~strike~~";
    let lines = markdown_lines(src, 80);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        texts,
        vec!["• ✅ Bold task with code and italic and strike"]
    );

    let out = rendered(src, 80).join("\n");
    assert!(out.contains("✅"), "glyph in {out:?}");
    assert!(out.contains("\x1b[1m"), "bold in {out:?}");
    assert!(out.contains("Bold"), "bold text in {out:?}");
    assert!(out.contains("\x1b[3m"), "italic in {out:?}");
    assert!(out.contains("italic"), "italic text in {out:?}");
    assert!(out.contains("\x1b[9m"), "strike in {out:?}");
    assert!(out.contains("strike"), "strike text in {out:?}");
    assert!(!out.contains("**"), "no markdown markers: {out:?}");
    assert!(!out.contains("~~"), "no markdown markers: {out:?}");
    assert!(!out.contains('`'), "no backticks: {out:?}");
    assert!(!out.contains("[x]"), "no bracket source: {out:?}");
}

#[test]
fn test_standalone_task_markers() {
    let src = "[ ] Standalone todo\n\n[·] Standalone doing\n\n[x] Standalone done";
    let lines = markdown_lines(src, 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        texts,
        vec![
            "⬜ Standalone todo",
            "",
            "🔄 Standalone doing",
            "",
            "✅ Standalone done",
        ]
    );
}

#[test]
fn test_empty_content_task_items() {
    let src = "- [ ]\n- [·]\n- [x]";
    let lines = markdown_lines(src, 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(texts, vec!["• ⬜ ", "• 🔄 ", "• ✅ "]);
}

#[test]
fn test_bulletless_task_list_renders_on_separate_lines_without_bullets() {
    let src = "[ ] Unselected\n[·] In progress\n[x] Completed\n[X] Completed #2";
    let lines = markdown_lines(src, 40);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        texts,
        vec![
            "⬜ Unselected",
            "🔄 In progress",
            "✅ Completed",
            "✅ Completed #2",
        ]
    );

    let joined = texts.concat();
    assert!(!joined.contains('•'), "must not contain bullet: {joined:?}");
    assert!(!joined.contains("[ ]"));
    assert!(!joined.contains("[·]"));
    assert!(!joined.contains("[x]"));
    assert!(!joined.contains("[X]"));
}

#[test]
fn test_bulletless_task_list_continuation_indent() {
    let src = "[ ] A long task description that wraps across multiple lines";
    let lines = markdown_lines(src, 16);
    let texts: Vec<String> = lines.iter().map(plain).collect();
    assert!(texts.len() > 1, "must wrap into multiple lines: {texts:?}");
    assert!(texts[0].starts_with("⬜ "));
    for (i, cont) in texts[1..].iter().enumerate() {
        assert!(
            cont.starts_with("   "),
            "continuation line {i} must indent 3 spaces past 2-col glyph + space: {cont:?}"
        );
        assert!(
            !cont.starts_with("    "),
            "indent must be exactly 3 spaces: {cont:?}"
        );
    }
}
