//! Unit tests for note-agnostic Markdown classification.

use super::*;
use std::borrow::Cow;

fn text_of<'a>(span: &'a Span<'a>) -> &'a str {
    match span {
        Span::Text(t) => t,
        _ => panic!("expected Text, got {span:?}"),
    }
}

fn para_text(block: &Block<'_>) -> String {
    match block {
        Block::Para { content } => content
            .spans()
            .iter()
            .map(|s| match s {
                Span::Text(t) => t.to_string(),
                _ => String::new(),
            })
            .collect(),
        _ => panic!("expected Para, got {block:?}"),
    }
}

#[test]
fn test_paragraph_joins_soft_wrapped_lines() {
    let flow = blocks("line one\nline two\nline three");
    assert_eq!(flow.len(), 1);
    assert_eq!(para_text(&flow.blocks()[0]), "line one line two line three");
}

#[test]
fn test_heading_strips_markers_and_levels() {
    let flow = blocks("### Title here");
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::Head { level, content } => {
            assert_eq!(*level, 3);
            assert_eq!(text_of(&content.spans()[0]), "Title here");
        }
        other => panic!("expected Head, got {other:?}"),
    }
}

#[test]
fn test_single_list_holds_all_items_with_blank_before() {
    let flow = blocks("- one\n\n- two\n- three");
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 3);
            assert!(!items[0].blank_before);
            assert!(items[1].blank_before);
            assert!(!items[2].blank_before);
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_numbered_list_markers_in_order() {
    let flow = blocks("1. One\n2. Two\n10. Ten");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].marker, ListMarker::Numbered(_)));
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_blank_inside_item_vs_list_ending() {
    // Blank + indented content stays inside the item.
    let flow = blocks("- one\n\n  continued\n- two");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 2);
            assert!(items[0].children.blocks().len() >= 2);
        }
        other => panic!("expected List, got {other:?}"),
    }
    // Blank + outer content ends the list.
    let flow = blocks("- one\n\nouter paragraph");
    assert!(matches!(flow.blocks()[0], Block::List { .. }));
    assert!(matches!(flow.blocks()[2], Block::Para { .. }));
}

#[test]
fn test_quote_joined_blank_is_child_vs_plain_blank_separates() {
    let flow = blocks("> a\n>\n> b");
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::Quote { children } => {
            assert!(children.blocks().iter().any(|b| matches!(b, Block::Blank)));
        }
        other => panic!("expected Quote, got {other:?}"),
    }
    let flow = blocks("> a\n\nplain");
    assert!(matches!(flow.blocks()[0], Block::Quote { .. }));
    assert!(matches!(flow.blocks()[1], Block::Blank));
    assert!(matches!(flow.blocks()[2], Block::Para { .. }));
}

#[test]
fn test_nested_quote_inside_quote() {
    let flow = blocks("> outer\n>> nested");
    match &flow.blocks()[0] {
        Block::Quote { children } => {
            assert!(
                children
                    .blocks()
                    .iter()
                    .any(|b| matches!(b, Block::Quote { .. }))
            );
        }
        other => panic!("expected Quote, got {other:?}"),
    }
}

#[test]
fn test_fence_lang_extraction_without_markers() {
    let flow = blocks("```rust\nlet x = 1;\n```");
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::Code { lang, lines } => {
            assert_eq!(lang.as_deref(), Some("rust"));
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].as_ref(), "let x = 1;");
        }
        other => panic!("expected Code, got {other:?}"),
    }
}

#[test]
fn test_unterminated_fence_still_emits_code() {
    let flow = blocks("```py\nprint(1)");
    assert_eq!(flow.len(), 1);
    assert!(matches!(flow.blocks()[0], Block::Code { .. }));
}

#[test]
fn test_rule_detection() {
    let flow = blocks("para\n\n---\n\npara");
    assert!(flow.blocks().iter().any(|b| matches!(b, Block::Rule)));
}

#[test]
fn test_consecutive_blanks_condense_to_one() {
    let flow = blocks("a\n\n\n\nb");
    let blanks = flow
        .blocks()
        .iter()
        .filter(|b| matches!(b, Block::Blank))
        .count();
    assert_eq!(blanks, 1);
}

#[test]
fn test_quote_inside_list_item() {
    let flow = blocks("- item\n\n  > quoted");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert!(
                items[0]
                    .children
                    .blocks()
                    .iter()
                    .any(|b| matches!(b, Block::Quote { .. }))
            );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_nested_lists() {
    let flow = blocks("- outer\n  - inner");
    assert!(matches!(flow.blocks()[0], Block::List { .. }));
}

#[test]
fn test_bold_italic_strike_have_no_markers() {
    let inline = spans("**bold** and *italic* and ~~strike~~");
    let rendered: String = inline
        .spans()
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.to_string(),
            Span::Bold(inner) => inner.spans().iter().map(span_debug_text).collect(),
            Span::Italic(inner) => inner.spans().iter().map(span_debug_text).collect(),
            Span::Strike(inner) => inner.spans().iter().map(span_debug_text).collect(),
            _ => String::new(),
        })
        .collect();
    assert!(!rendered.contains('*') && !rendered.contains('~'));
    assert!(matches!(inline.spans()[0], Span::Bold(_)));
    assert!(inline.spans().iter().any(|s| matches!(s, Span::Italic(_))));
    assert!(inline.spans().iter().any(|s| matches!(s, Span::Strike(_))));
}

fn span_debug_text(s: &Span<'_>) -> String {
    match s {
        Span::Text(t) => t.to_string(),
        _ => String::new(),
    }
}

#[test]
fn test_code_span_drops_backticks() {
    let inline = spans("use `code` here");
    assert!(inline.spans().iter().any(|s| matches!(
        s,
        Span::Code(c) if c.as_ref() == "code"
    )));
}

#[test]
fn test_link_produces_label_and_url() {
    let inline = spans("[label](https://example.com)");
    assert!(inline.spans().iter().any(|s| matches!(
        s,
        Span::Link { label, url } if url.as_ref() == "https://example.com" && !label.spans().is_empty()
    )));
}

#[test]
fn test_task_markers_classify_three_states() {
    for (src, want) in [
        ("[ ] todo", TaskState::Empty),
        ("[·] doing", TaskState::InProgress),
        ("[x] done", TaskState::Checked),
    ] {
        let inline = spans(src);
        assert!(
            matches!(inline.spans().first(), Some(Span::Task(s)) if *s == want),
            "src={src:?} got={:?}",
            inline.spans()
        );
    }
}

#[test]
fn test_task_markers_in_list_items() {
    let flow = blocks("- [x] done\n- [ ] todo");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 2);
            let first_para = items[0]
                .children
                .blocks()
                .iter()
                .find_map(|b| match b {
                    Block::Para { content } => Some(content),
                    _ => None,
                })
                .expect("first item has Para");
            assert!(matches!(first_para.spans().first(), Some(Span::Task(_))));
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_literal_star_in_prose_survives() {
    let inline = spans("2 * 3 = 6");
    assert!(inline.spans().iter().any(|s| match s {
        Span::Text(t) => t.contains('*'),
        _ => false,
    }));
}

#[test]
fn test_nested_emphasis_nests_spans() {
    let inline = spans("*a **b** c*");
    assert!(inline.spans().iter().any(|s| matches!(
        s,
        Span::Italic(inner) if inner.spans().iter().any(|c| matches!(c, Span::Bold(_)))
    )));
}

#[test]
fn test_html_passes_through_as_text() {
    let inline = spans("a <b>tag</b> here");
    assert!(!inline.spans().is_empty());
}

#[test]
fn test_spans_borrow_without_rebuild() {
    let inline = spans("plain prose");
    assert!(matches!(inline.spans()[0], Span::Text(Cow::Borrowed(_))));
}

#[test]
fn test_table_header_rows_and_alignment() {
    let flow = blocks("| Name | Age |\n| :--- | ---: |\n| Ann | 30 |\n| **Bo** | 7 |");
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::Table {
            header,
            rows,
            alignments,
        } => {
            assert_eq!(alignments, &vec![ColumnAlign::Left, ColumnAlign::Right]);
            assert_eq!(header.len(), 2);
            assert_eq!(rows.len(), 2);
            assert!(matches!(header[0].spans()[0], Span::Text(_)));
            assert!(matches!(rows[1][0].spans()[0], Span::Bold(_)));
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn test_table_center_alignment() {
    let flow = blocks("| A |\n| :---: |\n| x |");
    match &flow.blocks()[0] {
        Block::Table { alignments, .. } => {
            assert_eq!(alignments, &vec![ColumnAlign::Center]);
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn test_inline_whitespace_folds_to_spaces() {
    let inline = spans("a\tb\nc");
    let joined: String = inline
        .spans()
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.to_string(),
            _ => String::new(),
        })
        .collect();
    assert!(!joined.contains(['\t', '\n']), "{joined:?}");
}

#[test]
fn test_code_span_keeps_inner_spaces() {
    // CommonMark strips one surrounding space; the classifier must not strip
    // beyond that (``  y  `` keeps one).
    let inline = spans("use ``  y  `` here");
    assert!(inline.spans().iter().any(|s| matches!(
        s,
        Span::Code(c) if c.as_ref() == " y "
    )));
}

/// Contract: item ranges include their trailing blanks, which is how loose
/// inter-item gaps are detected. Pinned against pulldown-cmark 0.13.
#[test]
fn test_pulldown_item_range_includes_trailing_blank() {
    use pulldown_cmark::{Options, Parser};
    let src = "- one\n\n- two";
    let opts = Options::ENABLE_TASKLISTS | Options::ENABLE_GFM;
    let items: Vec<std::ops::Range<usize>> = Parser::new_ext(src, opts)
        .into_offset_iter()
        .filter_map(|(ev, range)| match ev {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Item) => Some(range),
            _ => None,
        })
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(&src[items[0].clone()], "- one\n\n");
}

/// Contract: tight items and task markers carry no `Paragraph` events.
#[test]
fn test_pulldown_bare_inline_under_item() {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    for src in ["- one\n\nouter", "- [x] done"] {
        let opts = Options::ENABLE_TASKLISTS | Options::ENABLE_GFM;
        let events: Vec<Event<'_>> = Parser::new_ext(src, opts).collect();
        let mut depth = 0;
        let mut bare = false;
        for ev in &events {
            match ev {
                Event::Start(Tag::Item) => depth += 1,
                Event::End(TagEnd::Item) => depth -= 1,
                Event::Start(Tag::Paragraph) if depth > 0 => {
                    panic!("unexpected Paragraph in tight/task item for {src:?}")
                }
                Event::Text(_) | Event::TaskListMarker(_) if depth > 0 => bare = true,
                _ => {}
            }
        }
        assert!(bare, "expected bare inline for {src:?}");
    }
}

#[test]
fn test_unicode_whitespace_folds_to_single_space() {
    // NBSP, em space, and ideographic space all fold; runs collapse.
    let inline = spans("a b c　d");
    let joined: String = inline
        .spans()
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.to_string(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(joined, "a b c d");
}

#[test]
fn test_whitespace_runs_collapse() {
    let inline = spans("a  b\t\tc");
    let joined: String = inline
        .spans()
        .iter()
        .map(|s| match s {
            Span::Text(t) => t.to_string(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(joined, "a b c");
}

#[test]
fn test_task_marker_with_nbsp_separator() {
    let inline = spans("[x] done");
    assert!(matches!(
        inline.spans().first(),
        Some(Span::Task(TaskState::Checked))
    ));
}

#[test]
fn test_task_marker_in_progress_in_list_items() {
    let flow = blocks("- [·] doing\n- [ ] todo\n- [x] done");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 3);
            let first_para = items[0]
                .children
                .blocks()
                .iter()
                .find_map(|b| match b {
                    Block::Para { content } => Some(content),
                    _ => None,
                })
                .expect("first item has Para");
            assert_eq!(
                first_para.spans().first(),
                Some(&Span::Task(TaskState::InProgress))
            );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn test_standalone_paragraph_task_markers() {
    let flow = blocks("[ ] todo\n\n[·] doing\n\n[x] done");
    assert_eq!(flow.len(), 5); // 3 lists + 2 blanks
    let states = [TaskState::Empty, TaskState::InProgress, TaskState::Checked];
    let lists: Vec<&Block<'_>> = flow
        .blocks()
        .iter()
        .filter(|b| matches!(b, Block::List { .. }))
        .collect();
    assert_eq!(lists.len(), 3);
    for (block, want_state) in lists.iter().zip(states) {
        match block {
            Block::List { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].marker, ListMarker::None);
                let para = items_para(&items[0]);
                assert_eq!(para.spans().first(), Some(&Span::Task(want_state)));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn test_empty_task_list_items() {
    let flow = blocks("- [ ]\n- [·]\n- [x]");
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 3);
            let states = [TaskState::Empty, TaskState::InProgress, TaskState::Checked];
            for (item, want_state) in items.iter().zip(states) {
                let para = items_para(item);
                assert_eq!(para.spans(), &[Span::Task(want_state)]);
            }
        }
        other => panic!("expected List, got {other:?}"),
    }
}

fn items_para<'a>(item: &'a ListItem<'a>) -> &'a Inline<'a> {
    item.children
        .blocks()
        .iter()
        .find_map(|b| match b {
            Block::Para { content } => Some(content),
            _ => None,
        })
        .expect("item has Para")
}

#[test]
fn test_bulletless_task_list_items() {
    let src = "[ ] Unselected\n[·] In progress\n[x] Completed\n[X] Completed #2";
    let flow = blocks(src);
    assert_eq!(flow.len(), 1);
    match &flow.blocks()[0] {
        Block::List { items } => {
            assert_eq!(items.len(), 4);
            let states = [
                TaskState::Empty,
                TaskState::InProgress,
                TaskState::Checked,
                TaskState::Checked,
            ];
            for (item, want_state) in items.iter().zip(states) {
                assert_eq!(item.marker, ListMarker::None);
                let para = items_para(item);
                assert_eq!(para.spans().first(), Some(&Span::Task(want_state)));
            }
        }
        other => panic!("expected List, got {other:?}"),
    }
}
