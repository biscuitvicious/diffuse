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
        common_prefix: Some("crates/diffuse-core/src/"),
        width: 100,
        is_streaming: false,
    };
    let rendered = render_status_bar(ctx).join("\n");
    assert!(
        rendered.contains("file 1/11"),
        "status bar must show file 1/11"
    );
    assert!(
        rendered.contains("38;5;183m") || rendered.contains("38;5;225m"),
        "file stats in status bar must have purple tint"
    );
    assert!(
        rendered.contains("38;5;208m"),
        "hunk count in status bar must have orange tint 208"
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
        common_prefix: None,
        width: 80,
        is_streaming: false,
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

    let header_line = header_bar_line("diffuse", &file, Some("src/"));
    assert!(header_line.to_string().contains("Review: diffuse"));
    assert!(header_line.to_string().contains("src/"));
    assert!(header_line.to_string().contains("main.rs"));
    assert!(header_line.to_string().contains("+5 −2 ~1"));

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

    let rename_header = header_bar_line("diffuse", &rename_file, Some("crates/"));
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
        common_prefix: Some("src/"),
        width: 80,
        is_streaming: true,
    };
    let status_line = status_bar_line(ctx);
    assert!(status_line.to_string().contains("file 1/2"));
    assert!(status_line.to_string().contains("streaming..."));
    assert!(status_line.to_string().contains("col 4+"));
    assert!(status_line.to_string().contains("go 12"));
    assert!(!status_line.to_string().contains("prefix:"));
    assert!(status_line.to_string().contains("branch: main"));

    let (div, rib) = bottom_ribbon_lines(80);
    assert!(div.to_string().contains("──"));
    assert!(rib.to_string().contains("[j/k]move"));
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
    let ansi = render_header_bar("repo", &file, 80, None).join("\n");
    assert!(
        !ansi.contains("0 note"),
        "ANSI header must not show '0 notes' when there are no notes"
    );
    // Ratatui header: must not contain "0 note".
    let line = header_bar_line_themed(
        "repo",
        &file,
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
    let ansi = render_header_bar("repo", &file, 80, None).join("\n");
    assert!(
        ansi.contains("2 notes"),
        "ANSI header must show '2 notes' when there are 2 notes"
    );
    // Ratatui header: must contain "2 notes".
    let line = header_bar_line_themed(
        "repo",
        &file,
        &crate::render::DiffuseTheme::from_env(),
        None,
    );
    assert!(
        line.to_string().contains("2 notes"),
        "TUI header line must show '2 notes' when there are 2 notes"
    );
}
