use super::*;
use crate::core::counts::FileCounts;

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
        width: 80,
        is_streaming: false,
    };
    let rendered = render_status_bar(ctx).join("\n");
    assert!(
        rendered.contains("1/11 files"),
        "status bar must show 1/11 files"
    );
    assert!(
        rendered.contains("38;5;183m") || rendered.contains("38;5;225m"),
        "file stats in status bar must have purple tint"
    );
    assert!(
        rendered.contains("38;5;220m"),
        "hunk count in status bar must have golden-amber tint 220"
    );
}

#[test]
fn test_ratatui_header_and_status_lines() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
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
    };

    let header_line = header_bar_line("diffuse", &file);
    assert!(header_line.to_string().contains("Review: diffuse"));
    assert!(header_line.to_string().contains("src/main.rs"));
    assert!(header_line.to_string().contains("+5 −2 ~1"));

    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 2,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        col_offset: 4,
        num_buffer: "12",
        branch: "main",
        width: 80,
        is_streaming: true,
    };
    let status_line = status_bar_line(ctx);
    assert!(status_line.to_string().contains("1/2 files"));
    assert!(status_line.to_string().contains("streaming..."));
    assert!(status_line.to_string().contains("col 4+"));
    assert!(status_line.to_string().contains("go 12"));
    assert!(status_line.to_string().contains("branch: main"));

    let (div, rib) = bottom_ribbon_lines(80);
    assert!(div.to_string().contains("──"));
    assert!(rib.to_string().contains("[j/k]move"));
}
