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
        branch: "main",
        commit_range: None,
        common_prefix: Some("crates/diffuse-core/src/"),
        width: 100,
        is_streaming: false,
        streaming_dot: "●",
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
        branch: "main",
        commit_range: None,
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
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
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
    };

    let header_line = header_bar_line("diffuse", Some(&file), Some("src/"));
    assert!(header_line.to_string().contains("diffuse"));
    assert!(header_line.to_string().contains("src/"));
    assert!(header_line.to_string().contains("main.rs"));
    assert!(header_line.to_string().contains("−2 +5 ~1"));

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
    };

    let rename_header = header_bar_line("diffuse", Some(&rename_file), Some("crates/"));
    assert!(rename_header.to_string().contains("crates/"));
    assert!(rename_header.to_string().contains("algo/mod.rs -> algo.rs"));

    let rename_pfx_span = rename_header
        .spans
        .iter()
        .find(|s| s.content == "diffuse-core/src/")
        .expect("must have diffuse-core/src/ span in header line");
    assert_eq!(rename_pfx_span.style.fg, Some(Color::Indexed(139)));
    assert!(
        rename_pfx_span
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );

    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 2,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 4,
        num_buffer: "12",
        branch: "main",
        commit_range: None,
        common_prefix: Some("src/"),
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
    };
    let status_line = status_bar_line(ctx);
    assert!(status_line.to_string().contains("file 1/2"));
    assert!(status_line.to_string().contains("streaming..."));
    assert!(status_line.to_string().contains("col 4+"));
    assert!(status_line.to_string().contains("go 12"));
    assert!(!status_line.to_string().contains("prefix:"));
    assert!(status_line.to_string().contains("\u{2387} main"));

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
    };
    // ANSI header: must not contain "0 note".
    let ansi = render_header_bar("repo", Some(&file), 80, None).join("\n");
    assert!(
        !ansi.contains("0 note"),
        "ANSI header must not show '0 notes' when there are no notes"
    );
    // Ratatui header: must not contain "0 note".
    let line = header_bar_line_themed(
        "repo",
        Some(&file),
        &crate::render::DiffuseTheme::from_env(),
        None,
    );
    assert!(
        !line.to_string().contains("0 note"),
        "TUI header line must not show '0 notes' when there are no notes"
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
        counts: FileCounts::default(),
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
    };
    // ANSI header: must contain "2 notes".
    let ansi = render_header_bar("repo", Some(&file), 80, None).join("\n");
    assert!(
        ansi.contains("2 notes"),
        "ANSI header must show '2 notes' when there are 2 notes"
    );
    // Ratatui header: must contain "2 notes".
    let line = header_bar_line_themed(
        "repo",
        Some(&file),
        &crate::render::DiffuseTheme::from_env(),
        None,
    );
    assert!(
        line.to_string().contains("2 notes"),
        "TUI header line must show '2 notes' when there are 2 notes"
    );
}

#[test]
fn test_header_bar_none_file_no_diffs() {
    let ansi = render_header_bar("my_repo", None, 80, Some("crates/")).join("\n");
    assert!(ansi.contains("my_repo"), "must contain repo name");
    assert!(ansi.contains("crates/"), "must contain prefix");

    let line = header_bar_line_themed(
        "my_repo",
        None,
        &crate::render::DiffuseTheme::from_env(),
        Some("crates/"),
    );
    assert!(line.to_string().contains("my_repo"));
    assert!(line.to_string().contains("crates/"));

    let plain_line = header_bar_line("my_repo", None, None);
    assert!(plain_line.to_string().contains("my_repo"));
}

#[test]
fn test_status_bar_commit_range_removes_branch_indicator() {
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 3,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 0,
        num_buffer: "",
        branch: "work",
        commit_range: Some("3f05a71..7e0caab"),
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
    };

    // ANSI rendered status bar
    let ansi = render_status_bar(ctx).join("\n");
    assert!(
        ansi.contains("3f05a71..7e0caab"),
        "must show short hashes of commit range"
    );
    assert!(
        !ansi.contains("\u{2387}"),
        "must NOT show branch glyph when viewing commit range"
    );
    assert!(
        !ansi.contains("work"),
        "must NOT show branch name when viewing commit range"
    );

    // Ratatui rendered status line
    let line = status_bar_line(ctx);
    let line_str = line.to_string();
    assert!(
        line_str.contains("3f05a71..7e0caab"),
        "Ratatui status line must show commit range"
    );
    assert!(
        !line_str.contains("\u{2387}"),
        "Ratatui status line must NOT show branch glyph"
    );
    assert!(
        !line_str.contains("work"),
        "Ratatui status line must NOT show branch name"
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
        branch: "main",
        commit_range: None,
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
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
        branch: "main",
        commit_range: None,
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: " ",
    };
    let ansi_off = render_status_bar(ctx_dot_off).join("\n");
    assert!(ansi_off.contains("  streaming..."));
    let line_off = status_bar_line(ctx_dot_off);
    assert!(line_off.to_string().contains("  streaming..."));
}

#[test]
fn test_header_bar_omits_modified_status_and_shows_others() {
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
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
    };

    // ANSI renderer with modified status
    let ansi_mod = render_header_bar("diffuse", Some(&file), 80, None).join("\n");
    assert!(
        !ansi_mod.contains("(modified)"),
        "ANSI header bar must omit '(modified)', got: {ansi_mod}"
    );

    // Ratatui line renderer with modified status
    let line_mod = header_bar_line("diffuse", Some(&file), None).to_string();
    assert!(
        !line_mod.contains("(modified)"),
        "TUI header line must omit '(modified)', got: {line_mod}"
    );

    // Now test with added status
    file.status = "added".to_string();
    let ansi_add = render_header_bar("diffuse", Some(&file), 80, None).join("\n");
    assert!(
        ansi_add.contains("(added)"),
        "ANSI header bar must show '(added)', got: {ansi_add}"
    );
    let line_add = header_bar_line("diffuse", Some(&file), None).to_string();
    assert!(
        line_add.contains("(added)"),
        "TUI header line must show '(added)', got: {line_add}"
    );

    // Test with deleted status
    file.status = "deleted".to_string();
    let ansi_del = render_header_bar("diffuse", Some(&file), 80, None).join("\n");
    assert!(
        ansi_del.contains("(deleted)"),
        "ANSI header bar must show '(deleted)', got: {ansi_del}"
    );
    let line_del = header_bar_line("diffuse", Some(&file), None).to_string();
    assert!(
        line_del.contains("(deleted)"),
        "TUI header line must show '(deleted)', got: {line_del}"
    );
}
