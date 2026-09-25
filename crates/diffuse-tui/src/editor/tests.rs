use super::*;

#[test]
fn test_resolve_vim_single_line() {
    let (args, detached) = resolve_editor_args("vim", "src/main.rs", 42, None);
    assert_eq!(args, vec!["vim", "+42", "src/main.rs"]);
    assert!(!detached);
}

#[test]
fn test_resolve_vim_range() {
    let (args, detached) = resolve_editor_args("nvim", "src/main.rs", 42, Some((10, 15)));
    assert_eq!(
        args,
        vec!["nvim", "-c", "10G", "-c", "normal! V5j", "src/main.rs"]
    );
    assert!(!detached);
}

#[test]
fn test_resolve_code_detached() {
    let (args, detached) = resolve_editor_args("code", "src/main.rs", 100, None);
    assert_eq!(args, vec!["code", "--goto", "src/main.rs:100:1"]);
    assert!(detached);
}

#[test]
fn test_resolve_helix() {
    let (args, detached) = resolve_editor_args("hx", "src/lib.rs", 25, None);
    assert_eq!(args, vec!["hx", "src/lib.rs:25"]);
    assert!(!detached);
}

#[test]
fn test_editor_new_empty_has_one_empty_line_and_origin_cursor() {
    let editor = Editor::new("");
    assert_eq!(editor.text(), "");
    assert_eq!(editor.lines(), &[""]);
    assert_eq!(editor.cursor(), (0, 0));
}

#[test]
fn test_editor_new_with_multiline_text() {
    let editor = Editor::new("hello\nworld");
    assert_eq!(editor.lines(), &["hello", "world"]);
    assert_eq!(editor.cursor(), (0, 0));
    assert_eq!(editor.text(), "hello\nworld");
}

#[test]
fn test_editor_insert_char_and_advance_cursor() {
    let mut editor = Editor::new("");
    editor.insert_char('a');
    editor.insert_char('b');
    editor.insert_char('c');
    assert_eq!(editor.text(), "abc");
    assert_eq!(editor.cursor(), (0, 3));
}

#[test]
fn test_editor_insert_unicode_characters() {
    let mut editor = Editor::new("");
    editor.insert_char('🦀');
    editor.insert_char(' ');
    editor.insert_char('é');
    assert_eq!(editor.text(), "🦀 é");
    assert_eq!(editor.cursor(), (0, 3)); // 3 chars
}

#[test]
fn test_editor_insert_newline_splits_line() {
    let mut editor = Editor::new("helloworld");
    editor.set_cursor(0, 5);
    editor.insert_newline();
    assert_eq!(editor.lines(), &["hello", "world"]);
    assert_eq!(editor.cursor(), (1, 0));
    assert_eq!(editor.text(), "hello\nworld");
}

#[test]
fn test_editor_delete_backward_within_line() {
    let mut editor = Editor::new("abc");
    editor.set_cursor(0, 2); // between b and c
    editor.delete_backward();
    assert_eq!(editor.text(), "ac");
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn test_editor_delete_backward_at_line_start_joins_lines() {
    let mut editor = Editor::new("hello\nworld");
    editor.set_cursor(1, 0);
    editor.delete_backward();
    assert_eq!(editor.text(), "helloworld");
    assert_eq!(editor.lines(), &["helloworld"]);
    assert_eq!(editor.cursor(), (0, 5));
}

#[test]
fn test_editor_delete_forward_within_line() {
    let mut editor = Editor::new("abc");
    editor.set_cursor(0, 1); // on 'b'
    editor.delete_forward();
    assert_eq!(editor.text(), "ac");
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn test_editor_delete_forward_at_line_end_joins_lines() {
    let mut editor = Editor::new("hello\nworld");
    editor.set_cursor(0, 5);
    editor.delete_forward();
    assert_eq!(editor.text(), "helloworld");
    assert_eq!(editor.lines(), &["helloworld"]);
    assert_eq!(editor.cursor(), (0, 5));
}

#[test]
fn test_editor_move_left_and_right() {
    let mut editor = Editor::new("a\nb");
    editor.set_cursor(0, 0);
    editor.move_left(); // at start, no-op
    assert_eq!(editor.cursor(), (0, 0));

    editor.move_right(); // to (0, 1)
    assert_eq!(editor.cursor(), (0, 1));

    editor.move_right(); // wraps to (1, 0)
    assert_eq!(editor.cursor(), (1, 0));

    editor.move_left(); // wraps back to (0, 1)
    assert_eq!(editor.cursor(), (0, 1));
}

#[test]
fn test_editor_move_home_and_end() {
    let mut editor = Editor::new("hello world");
    editor.set_cursor(0, 5);
    editor.move_home();
    assert_eq!(editor.cursor(), (0, 0));
    editor.move_end();
    assert_eq!(editor.cursor(), (0, 11));
}

#[test]
fn test_editor_move_top_and_bottom() {
    let mut editor = Editor::new("line 1\nline 2\nline 3");
    editor.move_bottom();
    assert_eq!(editor.cursor(), (2, 6));
    editor.move_top();
    assert_eq!(editor.cursor(), (0, 0));
}

#[test]
fn test_editor_vertical_motion_sticky_goal_column() {
    let mut editor = Editor::new("long line 123456\nshort\nlong line again 123456");
    editor.set_width(80); // single visual segment per line
    editor.set_cursor(0, 12);

    // Moving down to "short" (len 5): clamped to 5
    editor.move_down();
    assert_eq!(editor.cursor(), (1, 5));

    // Moving down to "long line again 123456": restores goal column 12
    editor.move_down();
    assert_eq!(editor.cursor(), (2, 12));

    // Moving up back to "short": clamped to 5
    editor.move_up();
    assert_eq!(editor.cursor(), (1, 5));

    // Moving up back to line 0: restores goal column 12
    editor.move_up();
    assert_eq!(editor.cursor(), (0, 12));
}

#[test]
fn test_editor_soft_wrap_visual_motion() {
    // Width 10: "0123456789abcdef" wraps into [0..10] ("0123456789") and
    // [10..16] ("abcdef")
    let mut editor = Editor::new("0123456789abcdef");
    editor.set_width(10);
    editor.set_cursor(0, 3); // on first visual row, col 3

    // Move down should step to second visual row of the SAME logical line
    editor.move_down();
    let (row, col) = editor.cursor();
    assert_eq!(row, 0, "still on logical line 0");
    assert_eq!(col, 13, "visual col 3 on second segment = char 10 + 3 = 13");

    // Move up should return to first visual row at col 3
    editor.move_up();
    assert_eq!(editor.cursor(), (0, 3));
}

#[test]
fn test_editor_word_navigation() {
    let mut editor = Editor::new("hello   world, foo");
    editor.set_cursor(0, 0);

    editor.move_word_forward();
    assert_eq!(editor.cursor(), (0, 5)); // after "hello"

    editor.move_word_forward();
    assert_eq!(editor.cursor(), (0, 13)); // after "world"

    editor.move_word_backward();
    assert_eq!(editor.cursor(), (0, 8)); // start of "world"

    editor.move_word_backward();
    assert_eq!(editor.cursor(), (0, 0)); // start of "hello"
}

#[test]
fn test_editor_kill_to_end_of_line_and_yank() {
    let mut editor = Editor::new("hello world");
    editor.set_cursor(0, 5);
    editor.kill_to_end_of_line();
    assert_eq!(editor.text(), "hello");
    assert_eq!(editor.cursor(), (0, 5));

    editor.yank();
    assert_eq!(editor.text(), "hello world");
    assert_eq!(editor.cursor(), (0, 11));
}

#[test]
fn test_editor_kill_to_start_of_line() {
    let mut editor = Editor::new("hello world");
    editor.set_cursor(0, 6);
    editor.kill_to_start_of_line();
    assert_eq!(editor.text(), "world");
    assert_eq!(editor.cursor(), (0, 0));

    editor.yank();
    assert_eq!(editor.text(), "hello world");
    assert_eq!(editor.cursor(), (0, 6));
}

#[test]
fn test_editor_kill_word_backward_and_forward() {
    let mut editor = Editor::new("one two three");
    editor.set_cursor(0, 7); // after "one two"
    editor.kill_word_backward();
    assert_eq!(editor.text(), "one  three");
    assert_eq!(editor.cursor(), (0, 4));

    editor.set_cursor(0, 5); // before "three"
    editor.kill_word_forward();
    assert_eq!(editor.text(), "one  ");
    assert_eq!(editor.cursor(), (0, 5));
}

#[test]
fn test_editor_chained_kills_accumulate_in_kill_ring() {
    let mut editor = Editor::new("one two three");
    editor.set_cursor(0, 0);
    editor.kill_word_forward(); // kills "one"
    editor.kill_word_forward(); // kills " two"
    assert_eq!(editor.text(), " three");

    editor.set_cursor(0, 0);
    editor.yank(); // yanks "one two" (or combined kill text)
    assert_eq!(editor.text(), "one two three");
}

#[test]
fn test_editor_undo() {
    let mut editor = Editor::new("initial");
    editor.move_end();
    editor.insert_char('!');
    assert_eq!(editor.text(), "initial!");

    editor.undo();
    assert_eq!(editor.text(), "initial");
}

#[test]
fn test_editor_handle_key_shortcuts() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut editor = Editor::new("hello world");

    // Ctrl-a: home
    let ev = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::CursorMoved);
    assert_eq!(editor.cursor(), (0, 0));

    // Ctrl-e: end
    let ev = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::CursorMoved);
    assert_eq!(editor.cursor(), (0, 11));

    // Ctrl-w: kill word backward
    let ev = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::Modified);
    assert_eq!(editor.text(), "hello ");

    // Ctrl-y: yank
    let ev = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::Modified);
    assert_eq!(editor.text(), "hello world");

    // Ctrl-d: delete forward (at col 11 = nothing to delete)
    editor.set_cursor(0, 0);
    let ev = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::Modified);
    assert_eq!(editor.text(), "ello world");

    // Ctrl-_: undo
    let ev = KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::Modified);
    assert_eq!(editor.text(), "hello world");

    // Ctrl-s: submit
    let ev = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(editor.handle_key(ev), EditorAction::Submit);

    // Esc: cancel
    let ev = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(editor.handle_key(ev), EditorAction::Cancel);
}

#[test]
fn test_markdown_highlighter_styles_markdown_spans() {
    let theme = crate::render::DiffuseTheme::from_env();
    let highlighter = markdown_highlighter(theme);
    let lines = vec!["# Header".to_string()];
    let all_spans = highlighter(&lines);
    assert_eq!(all_spans.len(), 1);
    let spans = &all_spans[0];
    assert!(!spans.is_empty());
    let full: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(full, "# Header");
    // At least one span should have modifier bold
    assert!(spans.iter().any(|s| {
        s.style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    }));
}

#[test]
fn test_markdown_highlighter_table_headers_and_borders() {
    let theme = crate::render::DiffuseTheme::from_env();
    let highlighter = markdown_highlighter(theme);
    let lines = vec![
        "| Col A | Col B |".to_string(),
        "|---|---|".to_string(),
        "| 1 | 2 |".to_string(),
    ];
    let all_spans = highlighter(&lines);
    assert_eq!(all_spans.len(), 3);

    // Line 0: Header should have bold and underline modifier with wheat yellow
    // italic
    let h_spans = &all_spans[0];
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
                && s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content.contains("Col A")
        }),
        "table header cell must be bold and underlined"
    );
    assert!(
        h_spans.iter().any(|s| s.content.contains('|')),
        "table header must have pipes"
    );

    // Line 1: Delimiter row
    let d_spans = &all_spans[1];
    assert!(
        d_spans.iter().any(|s| s.content.contains("---")),
        "delimiter row must have dashes"
    );

    // Line 2: Body row
    let b_spans = &all_spans[2];
    assert!(
        b_spans.iter().any(|s| s.content.contains("1")),
        "body row must have cell content"
    );
}

#[test]
fn test_markdown_highlighter_table_header_style_stacking_and_spaces() {
    let theme = crate::render::DiffuseTheme::from_env();
    let highlighter = markdown_highlighter(theme);
    let lines = vec![
        "| **Name** | `Type` | Description |".to_string(),
        "|---|---|---|".to_string(),
        "| foo | u32 | bar |".to_string(),
    ];
    let all_spans = highlighter(&lines);
    assert_eq!(all_spans.len(), 3);

    let h_spans = &all_spans[0];

    // Name must be BOLD and UNDERLINED
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
                && s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content.contains("Name")
        }),
        "**Name** must be bold and underlined"
    );

    // Type must be BOLD and UNDERLINED
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
                && s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content.contains("Type")
        }),
        "`Type` must be bold and underlined"
    );

    // Description must be BOLD and UNDERLINED
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
                && s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content.contains("Description")
        }),
        "Description must be bold and underlined"
    );

    // Whitespace spans must NOT be underlined
    for s in h_spans {
        if s.content.trim().is_empty() {
            assert!(
                !s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::UNDERLINED),
                "whitespace span {:?} must not be underlined",
                s.content
            );
        }
    }

    // Accents `**` and `` ` `` must also be underlined in table header
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content == "**"
        }),
        "** accents must be underlined in table header"
    );
    assert!(
        h_spans.iter().any(|s| {
            s.style
                .add_modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
                && s.content == "`"
        }),
        "` accents must be underlined in table header"
    );
}

#[test]
fn test_editor_render_sets_cursor_position_and_scrolls() {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::{Position, Rect},
    };

    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut editor = Editor::new("line 0\nline 1\nline 2\nline 3\nline 4\nline 5");
    editor.set_cursor(1, 4); // on "line 1" at col 4

    let area = Rect::new(5, 2, 30, 4); // 4 rows tall, starting at (5, 2)
    terminal
        .draw(|f| {
            editor.render(f, area);
        })
        .unwrap();

    // Cursor position should be area.x + 4, area.y + 1
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        Position { x: 5 + 4, y: 2 + 1 }
    );

    // Now move cursor down to line 5 (past the 4-row viewport)
    editor.set_cursor(5, 2);
    terminal
        .draw(|f| {
            editor.render(f, area);
        })
        .unwrap();

    // The editor should have scrolled so line 5 is at the bottom of the 4-row
    // viewport (screen y = 2 + 3)
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        Position { x: 5 + 2, y: 2 + 3 }
    );
}
