use diffuse_core::{
    merge::RunKind,
    parse::RowKind,
    wire::{OutputRow, OutputRun},
};

use super::*;

#[test]
fn test_format_rows_with_context_no_bar_and_wraps_at_width() {
    let rows = vec![OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(392),
        old_line: None,
        runs: vec![OutputRun::new(
            RunKind::Add,
            "fn for_each_diff_line(row: &crate::core::wire::OutputRow, mut f: impl FnMut(char, String)) {".to_string(),
        )],
        wholesale: false,
        group: None,
    }];

    // Test at width 60: must wrap and must NOT contain indentation '│'
    let formatted_60 = format_rows_with_context(&rows, false, 60);
    assert!(
        formatted_60.len() >= 2,
        "must wrap at 60 columns: got {:?}",
        formatted_60
    );
    assert!(
        formatted_60[0].contains(" 392 +"),
        "first line has diff gutter prefix: {:?}",
        formatted_60[0]
    );
    for line in &formatted_60 {
        assert!(
            !line.contains('│'),
            "must remove the indentation '│' for source lines: got {line:?}"
        );
    }

    // Test at width 120: fits on a single line without wrapping
    let formatted_120 = format_rows_with_context(&rows, false, 120);
    assert_eq!(
        formatted_120.len(),
        1,
        "must fit on single line at width 120: got {:?}",
        formatted_120
    );
    assert!(!formatted_120[0].contains('│'));
    assert!(formatted_120[0].contains(" 392 +"));
}

#[test]
fn test_format_note_text_markdown_highlighting_and_uncolored() {
    use diffuse::ansi::ColorExt;

    let text = "# Issue title\n\nPlease check **important** and `code` details:\n- item 1\n```rust\nlet x = 1;\n```";
    let uncolored = format_note_text(text, false, 0);
    assert_eq!(
        uncolored,
        vec![
            "  # Issue title",
            "  ",
            "  Please check **important** and `code` details:",
            "  - item 1",
            "  ```rust",
            "  let x = 1;",
            "  ```",
        ]
    );

    let colorized = format_note_text(text, true, 0);
    assert_eq!(colorized.len(), 7);
    for line in &colorized {
        assert!(line.starts_with("  "));
    }
    assert!(colorized[0].contains("Issue title"));
    assert!(colorized[0].contains("\x1b[1m"));
    assert!(colorized[0].ends_with("\x1b[0m"));
    assert!(colorized[2].contains("important"));
    assert!(colorized[2].contains("\x1b[1m"));

    let kw_color = DiffuseTheme::dark()
        .token_color(diffuse_syntax::TokenColor::Keyword, RowKind::Ctx)
        .unwrap();
    assert!(colorized[5].contains(&kw_color.fg_code()));
}

#[test]
fn test_format_note_text_wrapping_adds_two_space_padding_to_continuation_lines() {
    let text = "This is a single long line of note text that will exceed forty columns and wrap into multiple lines.";
    let wrapped = format_note_text(text, false, 40);
    assert!(wrapped.len() >= 2);
    for line in &wrapped {
        assert!(
            line.starts_with("  "),
            "every wrapped line must have 2-space padding: {line:?}"
        );
    }

    let wrapped_color = format_note_text(text, true, 40);
    assert_eq!(wrapped_color.len(), wrapped.len());
    for line in &wrapped_color {
        assert!(
            line.starts_with("  "),
            "every wrapped color line must have 2-space padding: {line:?}"
        );
    }
}
