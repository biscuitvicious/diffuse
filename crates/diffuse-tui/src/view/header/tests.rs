use super::*;
use crate::core::{counts::FileCounts, notes::Side as NoteSide, wire::OutputNote};

#[test]
fn test_render_status_bar_styling() {
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 11,
        cur_hunk: 0,
        total_hunks: 3,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: Some("crates/diffuse-core/src/"),
        width: 100,
        is_streaming: false,
        streaming_dot: "●",
        file: None,
        hunk_counts: None,
    };
    let rendered = render_status_bar(ctx).join("\n");
    assert!(
        rendered.contains("file 1/11"),
        "status bar must show file 1/11"
    );
    assert!(
        rendered.contains("38;5;220m"),
        "file stats in status bar must match in-file file header gold tint 220"
    );
    assert!(
        rendered.contains("38;5;213m"),
        "hunk count in status bar must match in-file hunk divider purple tint 213"
    );
    assert!(
        !rendered.contains("prefix:"),
        "status bar must not contain prefix:"
    );

    // Root changes also do not display "prefix:" in status bar
    let root_ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 5,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: None,
        hunk_counts: None,
    };
    let rendered_root = render_status_bar(root_ctx).join("\n");
    assert!(
        !rendered_root.contains("prefix:"),
        "status bar must not show prefix: when files are in root"
    );
}

#[test]
fn test_ratatui_header_and_status_lines() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 5,
            dels: 2,
            mods: 1,
            movs: 0,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let header_line = header_bar_line("diffuse", Some("src/"), None, None, None);
    assert!(header_line.to_string().contains("diffuse"));
    assert!(header_line.to_string().contains("src/"));
    assert!(
        !header_line.to_string().contains("main.rs"),
        "top header must not show the current file"
    );

    // Check that common prefix span in header line is styled with mild accent
    // 152
    let prefix_span = header_line
        .spans
        .iter()
        .find(|s| s.content == "src/")
        .expect("must have src/ span in header line");
    assert_eq!(prefix_span.style.fg, Some(Color::Indexed(152)));

    // Rename file test
    let rename_file = OutputFile {
        path: "crates/diffuse-core/src/algo.rs".to_string(),
        old_path: Some("crates/diffuse-core/src/algo/mod.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: "renamed".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let rename_header = header_bar_line("diffuse", Some("crates/"), None, None, None);
    assert!(rename_header.to_string().contains("crates/"));
    assert!(
        !rename_header.to_string().contains("algo.rs"),
        "top header must not show the current file"
    );

    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 2,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 4,
        num_buffer: "12",
        common_prefix: Some("src/"),
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    };
    let status_line = status_bar_line(ctx);
    assert!(status_line.to_string().contains("file 1/2"));
    assert!(status_line.to_string().contains("streaming..."));
    assert!(status_line.to_string().contains("col 4+"));
    assert!(status_line.to_string().contains("go 12"));
    assert!(!status_line.to_string().contains("prefix:"));
    assert!(status_line.to_string().contains("main.rs"));
    assert!(status_line.to_string().contains("−2 +5 ~1"));

    // Streaming indicator sits between the file badge and the filename.
    let text = status_line.to_string();
    let badge = text.find("file 1/2").expect("must show file badge");
    let stream = text.find("streaming...").expect("must show streaming");
    let path = text.find("main.rs").expect("must show file path");
    assert!(
        badge < stream && stream < path,
        "streaming must come after the file badge and before the filename: {text}"
    );
    let ansi = render_status_bar(ctx).join("\n");
    let (a_badge, a_stream, a_path) = (
        ansi.find("file 1/2").expect("ansi file badge"),
        ansi.find("streaming...").expect("ansi streaming"),
        ansi.find("main.rs").expect("ansi file path"),
    );
    assert!(
        a_badge < a_stream && a_stream < a_path,
        "ANSI streaming must come after the file badge and before the filename"
    );

    // Renamed files render the stripped old -> new path in the status bar.
    let rename_ctx = StatusBarContext {
        common_prefix: Some("crates/"),
        file: Some(&rename_file),
        ..ctx
    };
    assert!(
        status_bar_line(rename_ctx)
            .to_string()
            .contains("diffuse-core/src/algo/mod.rs -> diffuse-core/src/algo.rs")
    );

    let file_span = status_line
        .spans
        .iter()
        .find(|s| s.content.contains("file 1/2"))
        .expect("must have file span");
    assert_eq!(
        file_span.style.fg,
        Some(Color::Indexed(220)),
        "file span must use color 220"
    );

    let hunk_span = status_line
        .spans
        .iter()
        .find(|s| s.content.contains("hunk 1/1"))
        .expect("must have hunk span");
    assert_eq!(
        hunk_span.style.fg,
        Some(Color::Indexed(213)),
        "hunk span must use color 213"
    );

    let (div, rib) = bottom_ribbon_lines(80);
    assert!(div.to_string().contains("──"));
    assert!(rib.to_string().starts_with(" [?]help [q]uit"));
    assert!(rib.to_string().contains("[j/k]move"));

    let (ansi_div, ansi_rib) = render_bottom_ribbon(80);
    assert!(ansi_div.contains("──"));
    assert!(ansi_rib.contains("[?]help [q]uit"));
}

#[test]
fn test_header_bar_zero_notes_suppressed() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 0,
        old_line_count: 0,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    // ANSI status bar: must not contain "💬" when there are no notes.
    let ansi = render_status_bar(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .join("\n");
    assert!(
        !ansi.contains("💬"),
        "ANSI status bar must not show note icon when there are no notes"
    );
    // Ratatui status bar: must not contain "💬" when there are no notes.
    let line = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    });
    assert!(
        !line.to_string().contains("💬"),
        "TUI status line must not show note icon when there are no notes"
    );
    // Top header with empty global counts: no note icon either.
    let header_ansi = render_header_bar("repo", 80, None, None, None, None).join("\n");
    assert!(
        !header_ansi.contains("💬"),
        "ANSI header must not show note icon when global counts are empty"
    );
}

#[test]
fn test_header_bar_nonzero_notes_shown() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 0,
        old_line_count: 0,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 0,
            dels: 0,
            mods: 0,
            movs: 0,
            notes: 2,
        },
        fences: vec![],
        notes: vec![
            OutputNote {
                id: "n1".to_string(),
                file: "src/main.rs".to_string(),
                side: NoteSide::New,
                start: 1,
                end: 1,
                text: "note one".to_string(),
                created_at: 0,
                runs: vec![],
            },
            OutputNote {
                id: "n2".to_string(),
                file: "src/main.rs".to_string(),
                side: NoteSide::New,
                start: 2,
                end: 2,
                text: "note two".to_string(),
                created_at: 0,
                runs: vec![],
            },
        ],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    // ANSI status bar: must contain "💬 2".
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    };
    let ansi = render_status_bar(ctx).join("\n");
    assert!(
        ansi.contains("💬 2"),
        "ANSI status bar must show '💬 2' when there are 2 notes"
    );
    // Ratatui status bar: must contain "💬 2".
    let line = status_bar_line(ctx);
    assert!(
        line.to_string().contains("💬 2"),
        "TUI status line must show '💬 2' when there are 2 notes"
    );
    // Global counts surface notes in the top header too.
    let global = FileCounts {
        adds: 0,
        dels: 0,
        mods: 0,
        movs: 0,
        notes: 2,
    };
    let header_ansi = render_header_bar("repo", 80, None, Some(global), None, None).join("\n");
    assert!(
        header_ansi.contains("💬 2"),
        "ANSI header must show global '💬 2'"
    );
}

#[test]
fn test_header_bar_none_file_no_diffs() {
    let ansi = render_header_bar("my_repo", 80, Some("crates/"), None, None, None).join("\n");
    assert!(ansi.contains("my_repo"), "must contain repo name");
    assert!(ansi.contains("crates/"), "must contain prefix");

    let line = header_bar_line_themed(
        "my_repo",
        &crate::render::DiffuseTheme::from_env(),
        Some("crates/"),
        None,
        None,
        None,
    );
    assert!(line.to_string().contains("my_repo"));
    assert!(line.to_string().contains("crates/"));

    let plain_line = header_bar_line("my_repo", None, None, None, None);
    assert!(plain_line.to_string().contains("my_repo"));
}

#[test]
fn test_status_bar_no_branch_indicator() {
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 3,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: None,
        hunk_counts: None,
    };

    // Branch indicator (glyph and name) should not appear in the status bar
    let ansi = render_status_bar(ctx).join("\n");
    assert!(
        !ansi.contains("\u{2387}"),
        "status bar must NOT show branch glyph"
    );
    assert!(
        !ansi.contains("work"),
        "status bar must NOT show branch name"
    );
}

#[test]
fn test_status_bar_streaming_dot_blinking() {
    let ctx_dot_on = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
        file: None,
        hunk_counts: None,
    };
    let ansi_on = render_status_bar(ctx_dot_on).join("\n");
    assert!(ansi_on.contains("● streaming..."));
    let line_on = status_bar_line(ctx_dot_on);
    assert!(line_on.to_string().contains("● streaming..."));

    let ctx_dot_off = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: " ",
        file: None,
        hunk_counts: None,
    };
    let ansi_off = render_status_bar(ctx_dot_off).join("\n");
    assert!(ansi_off.contains("  streaming..."));
    let line_off = status_bar_line(ctx_dot_off);
    assert!(line_off.to_string().contains("  streaming..."));
}

#[test]
fn test_status_bar_omits_modified_status_and_shows_others() {
    let mut file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 5,
            dels: 2,
            mods: 1,
            movs: 0,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    // ANSI renderer with modified status
    let ansi_mod = render_status_bar(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .join("\n");
    assert!(
        !ansi_mod.contains("(modified)"),
        "ANSI status bar must omit '(modified)', got: {ansi_mod}"
    );

    // Ratatui line renderer with modified status
    let line_mod = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .to_string();
    assert!(
        !line_mod.contains("(modified)"),
        "TUI status line must omit '(modified)', got: {line_mod}"
    );

    // Now test with added status
    file.status = "added".to_string();
    let ansi_add = render_status_bar(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .join("\n");
    assert!(
        ansi_add.contains("(added)"),
        "ANSI status bar must show '(added)', got: {ansi_add}"
    );
    let line_add = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .to_string();
    assert!(
        line_add.contains("(added)"),
        "TUI status line must show '(added)', got: {line_add}"
    );

    // Test with deleted status
    file.status = "deleted".to_string();
    let ansi_del = render_status_bar(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .join("\n");
    assert!(
        ansi_del.contains("(deleted)"),
        "ANSI status bar must show '(deleted)', got: {ansi_del}"
    );
    let line_del = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .to_string();
    assert!(
        line_del.contains("(deleted)"),
        "TUI status line must show '(deleted)', got: {line_del}"
    );
}

#[test]
fn test_clamp_or_pad_ansi_exact_width_guarantee() {
    // 1. Shorter plain string is padded to exact width
    let s1 = clamp_or_pad_ansi("hello", 10);
    assert_eq!(visible_width(&s1), 10);
    assert!(s1.starts_with("hello     \x1b[0m"));

    // 2. Longer plain string is truncated to exact width
    let s2 = clamp_or_pad_ansi("hello world wide universe", 10);
    assert_eq!(visible_width(&s2), 10);
    assert!(s2.starts_with("hello worl\x1b[0m"));

    // 3. String with ANSI escapes keeps escapes, truncates printable characters
    let colored = "\x1b[38;5;120mhello \x1b[1mworld\x1b[0m from diffuse";
    let s3 = clamp_or_pad_ansi(colored, 10);
    assert_eq!(visible_width(&s3), 10);
    assert!(s3.contains("\x1b[38;5;120m"));
    assert!(s3.ends_with("\x1b[0m"));

    // 4. String with tabs expands tabs and fits width exactly
    let tabbed = "a\tb";
    let s4 = clamp_or_pad_ansi(tabbed, 10);
    assert_eq!(visible_width(&s4), 10);
    assert!(!s4.contains('\t'));
    assert!(s4.starts_with("a   b     \x1b[0m"));

    // 5. Overflow with tabs clamps at target_width
    let long_tabbed = "a\t\t\t\t\t\t\t\t\t\t\t\tb";
    let s5 = clamp_or_pad_ansi(long_tabbed, 15);
    assert_eq!(visible_width(&s5), 15);
    assert!(!s5.contains('\t'));
}

#[test]
fn test_header_bar_global_mod_counts_ratatui() {
    let file = OutputFile {
        path: "AGENTS.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 15,
            dels: 7,
            mods: 6,
            movs: 0,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let global_counts = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 0,
        notes: 0,
    };

    let header_line = header_bar_line("diffuse", None, Some(global_counts), None, None);
    let text = header_line.to_string();
    assert_eq!(
        text, " diffuse −7 +15 ~6",
        "header text must format: diffuse <global mod counts> with no dot separator"
    );

    // Verify highlighting colors of global mod counts
    let del_span = header_line
        .spans
        .iter()
        .find(|s| s.content == "−7")
        .expect("must have −7 span");
    assert_eq!(del_span.style.fg, Some(Color::Indexed(203)));
    assert!(del_span.style.add_modifier.contains(Modifier::BOLD));

    let add_span = header_line
        .spans
        .iter()
        .find(|s| s.content == " +15")
        .expect("must have +15 span");
    assert_eq!(add_span.style.fg, Some(Color::Indexed(120)));
    assert!(add_span.style.add_modifier.contains(Modifier::BOLD));

    let mod_span = header_line
        .spans
        .iter()
        .find(|s| s.content == " ~6")
        .expect("must have ~6 span");
    assert_eq!(mod_span.style.fg, Some(Color::Indexed(214)));
    assert!(mod_span.style.add_modifier.contains(Modifier::BOLD));

    // With moved lines
    let global_with_movs = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 2,
        notes: 0,
    };
    let mov_header_line = header_bar_line("diffuse", None, Some(global_with_movs), None, None);
    let mov_text = mov_header_line.to_string();
    assert_eq!(
        mov_text, " diffuse −7 +15 ~6 ≈2",
        "header text must format: diffuse −7 +15 ~6 ≈2 with no dot separator"
    );

    let mov_span = mov_header_line
        .spans
        .iter()
        .find(|s| s.content == " ≈2")
        .expect("must have ≈2 span");
    assert_eq!(mov_span.style.fg, Some(Color::Indexed(147)));
    assert!(mov_span.style.add_modifier.contains(Modifier::BOLD));

    // Per-file counts with moved lines render in the status bar.
    let mut file_with_movs = file.clone();
    file_with_movs.counts.movs = 3;
    let status_ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file_with_movs),
        hunk_counts: None,
    };
    let local_mov_text = status_bar_line(status_ctx).to_string();
    assert!(
        local_mov_text.contains("AGENTS.md"),
        "status line must show the file path"
    );
    assert!(
        local_mov_text.contains("−7 +15 ~6 ≈3"),
        "status line must format file counts with ≈3 (without 'moved')"
    );
}

#[test]
fn test_header_bar_global_mod_counts_ansi() {
    let file = OutputFile {
        path: "AGENTS.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 15,
            dels: 7,
            mods: 6,
            movs: 0,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let global_counts = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 2,
        notes: 0,
    };

    let ansi_lines = render_header_bar("diffuse", 120, None, Some(global_counts), None, None);
    let joined = ansi_lines.join("\n");

    // Check highlighted global mod counts in ANSI escapes
    assert!(joined.contains("\x1b[1m\x1b[38;5;203m−7\x1b[0m"));
    assert!(joined.contains("\x1b[1m\x1b[38;5;120m+15\x1b[0m"));
    assert!(joined.contains("\x1b[1m\x1b[38;5;214m~6\x1b[0m"));
    assert!(joined.contains("\x1b[1m\x1b[38;5;147m≈2\x1b[0m"));
    assert!(
        !joined.contains("AGENTS.md"),
        "top header must not show the current file"
    );

    // The current file surfaces in the status bar instead.
    let status_ansi = render_status_bar(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .join("\n");
    assert!(status_ansi.contains("AGENTS.md"));
    assert!(status_ansi.contains("file 1/1"));
}

#[test]
fn test_header_bar_global_mod_counts_with_common_prefix_and_none_file() {
    let global_counts = FileCounts {
        adds: 20,
        dels: 5,
        mods: 3,
        movs: 0,
        notes: 0,
    };

    // No file in the top header: repo, global counts, prefix only.
    let none_line = header_bar_line("diffuse", None, Some(global_counts), None, None);
    assert_eq!(none_line.to_string(), " diffuse −5 +20 ~3");

    // Common prefix + file
    let file = OutputFile {
        path: "crates/diffuse-tui/src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 5,
            dels: 2,
            mods: 1,
            movs: 0,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let pfx_line = header_bar_line(
        "diffuse",
        Some("crates/diffuse-tui/src/"),
        Some(global_counts),
        None,
        None,
    );
    assert_eq!(
        pfx_line.to_string(),
        " diffuse −5 +20 ~3 · crates/diffuse-tui/src/"
    );

    // The prefixed file path renders stripped in the status bar.
    let pfx_status = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: Some("crates/diffuse-tui/src/"),
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: None,
    })
    .to_string();
    assert!(
        pfx_status.contains("main.rs"),
        "status line must show the stripped file path"
    );
    assert!(
        !pfx_status.contains("crates/diffuse-tui/src/main.rs"),
        "status line must strip the common prefix from the file path"
    );
}

#[test]
fn test_format_counts_plain_ansi_spans_identical_underlying_struct() {
    use crate::view::header::{format_counts_ansi, format_counts_plain, format_counts_spans};

    // Test with all change types populated
    let counts = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 2,
        notes: 0,
    };

    assert_eq!(format_counts_plain(&counts), "−7 +15 ~6 ≈2");
    assert_eq!(
        format_counts_ansi(&counts),
        "\x1b[1m\x1b[38;5;203m−7\x1b[0m \x1b[1m\x1b[38;5;120m+15\x1b[0m \x1b[1m\x1b[38;5;214m~6\x1b[0m \x1b[1m\x1b[38;5;147m≈2\x1b[0m"
    );

    let spans = format_counts_spans(&counts);
    assert_eq!(spans.len(), 4);
    assert_eq!(spans[0].content, "−7");
    assert_eq!(spans[0].style.fg, Some(Color::Indexed(203)));
    assert_eq!(spans[1].content, " +15");
    assert_eq!(spans[1].style.fg, Some(Color::Indexed(120)));
    assert_eq!(spans[2].content, " ~6");
    assert_eq!(spans[2].style.fg, Some(Color::Indexed(214)));
    assert_eq!(spans[3].content, " ≈2");
    assert_eq!(spans[3].style.fg, Some(Color::Indexed(147)));

    // Test with only dels (zeros for adds, mods, movs)
    let del_only = FileCounts {
        adds: 0,
        dels: 4,
        mods: 0,
        movs: 0,
        notes: 0,
    };
    assert_eq!(format_counts_plain(&del_only), "−4");
    assert_eq!(
        format_counts_ansi(&del_only),
        "\x1b[1m\x1b[38;5;203m−4\x1b[0m"
    );
    let del_spans = format_counts_spans(&del_only);
    assert_eq!(del_spans.len(), 1);
    assert_eq!(del_spans[0].content, "−4");
}

#[test]
fn test_format_counts_omit_zero_dels() {
    use crate::view::header::{format_counts_ansi, format_counts_plain, format_counts_spans};

    // No del lines: the −0 segment must be omitted like any other zero.
    let adds_only = FileCounts {
        adds: 3,
        dels: 0,
        mods: 1,
        movs: 0,
        notes: 0,
    };
    assert_eq!(format_counts_plain(&adds_only), "+3 ~1");
    assert_eq!(
        format_counts_ansi(&adds_only),
        "\x1b[1m\x1b[38;5;120m+3\x1b[0m \x1b[1m\x1b[38;5;214m~1\x1b[0m"
    );
    let spans = format_counts_spans(&adds_only);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].content, "+3");
    assert_eq!(spans[0].style.fg, Some(Color::Indexed(120)));
    assert_eq!(spans[1].content, " ~1");
    assert_eq!(spans[1].style.fg, Some(Color::Indexed(214)));

    // Notes-only counts render bare.
    let notes_only = FileCounts {
        adds: 0,
        dels: 0,
        mods: 0,
        movs: 0,
        notes: 2,
    };
    assert_eq!(format_counts_plain(&notes_only), "💬 2");
}

#[test]
fn test_header_bar_shows_branch_indicator() {
    // ANSI header with branch
    let ansi = render_header_bar("diffuse", 80, None, None, Some("main"), None).join("\n");
    assert!(
        ansi.contains("\u{2387}"),
        "ANSI header must show branch glyph"
    );
    assert!(ansi.contains("main"), "ANSI header must show branch name");
    assert!(ansi.contains("diffuse"), "ANSI header must show repo");
    // The branch glyph itself is the separator — no `·` dot between the repo
    // name and the branch glyph.
    let repo_glyph_gap = ansi
        .find("diffuse")
        .and_then(|ri| ansi[ri..].find("\u{2387}").map(|gi| &ansi[ri..ri + gi]))
        .unwrap_or("");
    assert!(
        !repo_glyph_gap.contains('\u{00b7}'),
        "ANSI header must not place a dot between repo name and branch glyph"
    );

    // ANSI header without branch
    let ansi_no_branch = render_header_bar("diffuse", 80, None, None, None, None).join("\n");
    assert!(
        !ansi_no_branch.contains("\u{2387}"),
        "ANSI header must not show branch glyph when branch is None"
    );

    // Ratatui header with branch — verify spans contain branch
    let line = header_bar_line("diffuse", None, None, Some("main"), None);
    let text = line.to_string();
    assert!(
        text.contains("\u{2387}"),
        "header line must show branch glyph"
    );
    assert!(text.contains("main"), "header line must show branch name");

    // Verify the branch spans are styled correctly
    let branch_span = line
        .spans
        .iter()
        .find(|s| s.content == "main")
        .expect("must have branch span");
    assert_eq!(branch_span.style.fg, Some(Color::Indexed(252)));
    assert!(branch_span.style.add_modifier.contains(Modifier::BOLD));

    // The branch glyph span must directly precede the branch name with no
    // dot separator span in between.
    let glyph_idx = line
        .spans
        .iter()
        .position(|s| s.content == " \u{2387} ")
        .expect("must have branch glyph span");
    assert!(
        line.spans[..glyph_idx]
            .iter()
            .all(|s| !s.content.contains('\u{00b7}')),
        "TUI header must not place a dot between repo name and branch glyph"
    );
    assert_eq!(
        line.spans[glyph_idx + 1].content,
        "main",
        "branch glyph span must directly precede the branch name span"
    );

    // Commit range shows the same branch glyph in header
    let line_range = header_bar_line("diffuse", None, None, None, Some("v1.0..v2.0"));
    assert!(line_range.to_string().contains("v1.0..v2.0"));
    assert!(line_range.to_string().contains("\u{2387}"));
}

#[test]
fn test_top_header_drops_file_section_and_counts_dot() {
    let global = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 0,
        notes: 2,
    };
    // ANSI header: repo, glyph, branch, then global counts with no dot and
    // no file section.
    let ansi = render_header_bar("diffuse", 120, None, Some(global), Some("main"), None).join("\n");
    assert!(ansi.contains("diffuse"), "must show repo name");
    let visible = crate::render::ansi::strip_ansi(&ansi);
    assert_eq!(
        visible.trim_end(),
        " diffuse \u{2387} main −7 +15 ~6 💬 2",
        "unexpected top header visible text"
    );
    assert!(
        !visible.contains("main.rs"),
        "top header must not show the current file"
    );

    // Ratatui header renders the exact compact form.
    let line = header_bar_line("diffuse", None, Some(global), Some("main"), None);
    assert_eq!(
        line.to_string(),
        " diffuse \u{2387} main −7 +15 ~6 💬 2",
        "unexpected top header text"
    );
}

#[test]
fn test_status_bar_shows_file_and_hunk_stats() {
    use crate::core::{
        parse::RowKind,
        wire::{MovSide, OutputHunk, OutputRow},
    };
    let mk = |kind: RowKind| OutputRow {
        kind,
        mov_side: None,
        new_line: None,
        old_line: None,
        text: String::new(),
        del_text: None,
        spans: Default::default(),
        runs: vec![],
        wholesale: false,
    };
    let _ = MovSide::Tgt;
    let file = OutputFile {
        path: "AGENTS.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![OutputHunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 1,
            rows: vec![mk(RowKind::Del), mk(RowKind::Del), mk(RowKind::Add)],
        }],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 15,
            dels: 7,
            mods: 6,
            movs: 0,
            notes: 1,
        },
        fences: vec![],
        notes: vec![OutputNote {
            id: "n1".to_string(),
            file: "AGENTS.md".to_string(),
            side: NoteSide::New,
            start: 1,
            end: 1,
            text: "hunk note".to_string(),
            created_at: 0,
            runs: vec![],
        }],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let hunk_counts = file.hunks[0].counts_with_notes(&file.notes);
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 41,
        cur_hunk: 0,
        total_hunks: 3,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        file: Some(&file),
        hunk_counts: Some(hunk_counts),
    };
    let ansi = render_status_bar(ctx).join("\n");
    let visible = crate::render::ansi::strip_ansi(&ansi);
    assert_eq!(
        visible.trim_end(),
        " file 1/41 · AGENTS.md (−7 +15 ~6 💬 1) · hunk 1/3 (−2 +1 💬 1)",
        "unexpected status bar visible text"
    );

    let line = status_bar_line(ctx);
    assert_eq!(
        line.to_string().trim_end(),
        " file 1/41 · AGENTS.md (−7 +15 ~6 💬 1) · hunk 1/3 (−2 +1 💬 1)",
        "unexpected status bar text"
    );
}
