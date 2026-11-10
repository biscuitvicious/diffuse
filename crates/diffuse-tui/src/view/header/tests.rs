use super::*;
use crate::{
    core::{
        counts::FileCounts,
        notes::NoteStatus,
        wire::{OutputFile, OutputNote},
    },
    render::DiffuseTheme,
};
use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

#[test]
fn test_render_status_bar_styling() {
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 11,
        cur_hunk: 0,
        total_hunks: 3,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: Some("crates/diffuse-core/src/"),
        width: 100,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: None,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };
    let line = status_bar_line(ctx);
    assert_eq!(
        line.to_string().trim(),
        "crates/diffuse-core/src/",
        "TUI status line must carry the directory prefix"
    );
    let text = line.to_string();
    assert!(
        !text.contains("hunk 1/3"),
        "status bar must not show hunk badge"
    );
    assert!(
        !text.contains("file 1/11"),
        "status bar must omit file count badge"
    );
    assert!(
        !text.contains("prefix:"),
        "status bar must not contain prefix:"
    );

    // Root changes also do not display "prefix:" in status bar
    let root_ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 5,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: None,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };
    let rendered_root = status_bar_line(root_ctx).to_string();
    assert!(
        rendered_root.is_empty(),
        "status bar must be empty when no transient segments, got: {rendered_root}"
    );
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
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
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

    let header_line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        None,
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert!(header_line.to_string().contains("diffuse"));
    assert!(
        !header_line.to_string().contains("src/"),
        "top header must not show the directory prefix (it lives in the secondary bar)"
    );
    assert!(
        !header_line.to_string().contains("main.rs"),
        "top header must not show the current file"
    );

    // Rename file test
    let rename_file = OutputFile {
        path: "crates/diffuse-core/src/algo.rs".to_string(),
        old_path: Some("crates/diffuse-core/src/algo/mod.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Renamed,
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

    let rename_header = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        None,
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert!(rename_header.to_string().contains("diffuse"));
    assert!(
        !rename_header.to_string().contains("crates/"),
        "top header must not show the directory prefix"
    );
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
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 4,
        num_buffer: "12",
        common_prefix: Some("src/"),
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };
    let status_line = status_bar_line(ctx);
    assert!(!status_line.to_string().contains("hunk 1/1"));
    assert!(!status_line.to_string().contains("file 1/2"));
    assert!(status_line.to_string().contains("streaming..."));
    assert!(status_line.to_string().contains("col 4+"));
    assert!(status_line.to_string().contains("go 12"));
    assert!(status_line.to_string().contains("src/"));
    assert!(!status_line.to_string().contains("prefix:"));
    assert!(
        !status_line.to_string().contains("main.rs"),
        "secondary status bar must omit current file path"
    );
    assert!(
        !status_line.to_string().contains("−2 +5 ~1"),
        "secondary status bar must omit file change counts"
    );

    // Directory prefix leads; streaming trails last: prefix, col, go,
    // streaming.
    let text = status_line.to_string();
    let pfx = text.find("src/").expect("must show directory prefix");
    let col = text.find("col 4+").expect("must show col offset");
    let go = text.find("go 12").expect("must show go buffer");
    let stream = text.find("streaming...").expect("must show streaming");
    assert!(
        pfx < col && col < go && go < stream,
        "order must be prefix, col, go, streaming: {text}"
    );
    assert!(
        !text.contains("hunk"),
        "status line must omit hunk data: {text}"
    );
    // Renamed files do not show path in the secondary status bar.
    let rename_ctx = StatusBarContext {
        common_prefix: Some("crates/"),
        file: Some(&rename_file),
        ..ctx
    };
    let rename_text = status_bar_line(rename_ctx).to_string();
    assert!(
        rename_text.contains("crates/"),
        "secondary status bar must show the directory prefix: {rename_text}"
    );
    assert!(
        !rename_text.contains("algo.rs"),
        "secondary status bar must not contain file path: {rename_text}"
    );

    let hunk_span = status_line
        .spans
        .iter()
        .find(|s| s.content.contains("streaming..."))
        .expect("must have streaming span");
    assert!(
        hunk_span.style.add_modifier.contains(Modifier::BOLD),
        "streaming span must be bold"
    );

    let (div, rib) = bottom_ribbon_lines(80, &DiffuseTheme::dark().palette.ui);
    assert!(div.to_string().contains("──"));
    assert!(rib.to_string().starts_with(" [?]help [q]uit"));
    assert!(rib.to_string().contains("[j/k]move"));
}

#[test]
fn test_header_bar_zero_notes_suppressed() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
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
    // Ratatui status bar: must not contain "💬" when there are no notes.
    let line = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    });
    assert!(
        !line.to_string().contains("💬"),
        "TUI status line must not show note icon when there are no notes"
    );
    // Top header with empty global counts: no note icon either.
    let header_line = header_bar_line(
        "repo",
        &DiffuseTheme::dark(),
        None,
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert!(
        !header_line.to_string().contains("💬"),
        "header must not show note icon when global counts are empty"
    );
}

#[test]
fn test_header_bar_nonzero_notes_shown() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
        },
        fences: vec![],
        notes: vec![
            OutputNote {
                id: "n1".to_string(),
                file: "src/main.rs".to_string(),
                location: crate::core::notes::NoteLocation::line(
                    crate::core::notes::DiffCoord::new_side(1),
                ),
                text: "note one".to_string(),
                created_at: 0,
                runs: vec![],
                content_hash: None,
                status: NoteStatus::Open,
                original_source_rows: Vec::new(),
            },
            OutputNote {
                id: "n2".to_string(),
                file: "src/main.rs".to_string(),
                location: crate::core::notes::NoteLocation::line(
                    crate::core::notes::DiffCoord::new_side(2),
                ),
                text: "note two".to_string(),
                created_at: 0,
                runs: vec![],
                content_hash: None,
                status: NoteStatus::Open,
                original_source_rows: Vec::new(),
            },
        ],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    // Transient-only status bar: notes no longer surface here.
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: Some(file.counts),
        theme: crate::render::DiffuseTheme::dark(),
    };
    // Ratatui status bar: must be empty when no transient segments.
    let line = status_bar_line(ctx);
    assert!(
        line.to_string().is_empty(),
        "TUI status line must be empty when no transient segments"
    );
    // Global counts surface notes in the top header too.
    let global = FileCounts {
        adds: 0,
        dels: 0,
        mods: 0,
        movs: 0,
        notes: 2,
        ..Default::default()
    };
    let header_line = header_bar_line(
        "repo",
        &DiffuseTheme::dark(),
        Some(global),
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert!(
        header_line.to_string().contains("💬2"),
        "header must show global '💬2'"
    );
}

#[test]
fn test_header_bar_none_file_no_diffs() {
    let line = header_bar_line(
        "my_repo",
        &DiffuseTheme::dark(),
        None,
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert!(line.to_string().contains("my_repo"));
    assert!(
        !line.to_string().contains("crates/"),
        "header line must not contain the prefix"
    );

    let plain_line = header_bar_line(
        "my_repo",
        &DiffuseTheme::dark(),
        None,
        None,
        None,
        false,
        None,
        usize::MAX,
    );
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
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: None,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };

    // Branch indicator (glyph and name) should not appear in the status bar
    let text = status_bar_line(ctx).to_string();
    assert!(
        !text.contains("\u{2387}"),
        "status bar must NOT show branch glyph"
    );
    assert!(
        !text.contains("work"),
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
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: "●",
        status_message: None,
        file: None,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };
    let line_on = status_bar_line(ctx_dot_on);
    assert!(line_on.to_string().contains("● streaming..."));

    let ctx_dot_off = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: true,
        streaming_dot: " ",
        status_message: None,
        file: None,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    };
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
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
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

    // Ratatui line renderer with modified status
    let line_mod = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    })
    .to_string();
    assert!(
        !line_mod.contains("(modified)"),
        "TUI status line must omit '(modified)', got: {line_mod}"
    );

    // Now test with added status
    file.status = diffuse_core::FileStatus::Added;
    let line_add = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    })
    .to_string();
    assert!(
        !line_add.contains("(added)"),
        "TUI status line must omit '(added)', got: {line_add}"
    );

    // Test with deleted status
    file.status = diffuse_core::FileStatus::Deleted;
    let line_del = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 80,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    })
    .to_string();
    assert!(
        !line_del.contains("(deleted)"),
        "TUI status line must omit '(deleted)', got: {line_del}"
    );
}

#[test]
fn test_visible_width_emoji_zwj_and_ansi() {
    // Single character emoji
    assert_eq!(visible_width("🦀"), 2);
    // Emoji with ANSI styling
    assert_eq!(visible_width("\x1b[31m🦀\x1b[0m"), 2);
    // ZWJ sequence (woman technologist: 👩 + ZWJ + 💻) is a single grapheme
    // cluster of width 2
    assert_eq!(visible_width("👩\u{200d}💻"), 2);
    assert_eq!(visible_width("\x1b[32m👩\u{200d}💻\x1b[0m"), 2);
    // Flag sequence (regional indicators)
    assert_eq!(visible_width("🇩🇪"), 2);
    // Mixed text with ZWJ and styling
    assert_eq!(visible_width("\x1b[1mUnicode: 👩\u{200d}💻 \x1b[0m"), 12);
}

#[test]
fn test_header_bar_global_mod_counts_ratatui() {
    let file = OutputFile {
        path: "AGENTS.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
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
        ..Default::default()
    };

    let header_line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        Some(global_counts),
        None,
        None,
        false,
        None,
        usize::MAX,
    );
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
        ..Default::default()
    };
    let mov_header_line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        Some(global_with_movs),
        None,
        None,
        false,
        None,
        usize::MAX,
    );
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

    // Hunk counts no longer render in the transient-only status bar.
    let mut file_with_movs = file.clone();
    file_with_movs.counts.movs = 3;
    let status_ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file_with_movs),
        hunk_counts: Some(file_with_movs.counts),
        theme: crate::render::DiffuseTheme::dark(),
    };
    let local_mov_text = status_bar_line(status_ctx).to_string();
    assert!(
        local_mov_text.is_empty(),
        "status line must be empty when no transient segments, got: {local_mov_text}"
    );
}

#[test]
fn test_header_bar_global_mod_counts_with_common_prefix_and_none_file() {
    let global_counts = FileCounts {
        adds: 20,
        dels: 5,
        mods: 3,
        movs: 0,
        notes: 0,
        ..Default::default()
    };

    // No file in the top header: repo and global counts only; the prefix
    // lives in the secondary bar.
    let none_line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        Some(global_counts),
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert_eq!(none_line.to_string(), " diffuse −5 +20 ~3");

    // Common prefix + file
    let file = OutputFile {
        path: "crates/diffuse-tui/src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
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
        &DiffuseTheme::dark(),
        Some(global_counts),
        None,
        None,
        false,
        None,
        usize::MAX,
    );
    assert_eq!(
        pfx_line.to_string(),
        " diffuse −5 +20 ~3",
        "header must omit the directory prefix"
    );

    // The prefix trails in the secondary bar.
    let pfx_status = status_bar_line(StatusBarContext {
        cur_fi: 0,
        total_files: 1,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: Some("crates/diffuse-tui/src/"),
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    })
    .to_string();
    assert_eq!(
        pfx_status.trim(),
        "crates/diffuse-tui/src/",
        "secondary must carry the directory prefix, got: {pfx_status}"
    );
}

#[test]
fn test_format_counts_plain_and_spans_identical_underlying_struct() {
    use crate::view::header::{format_counts_plain, format_counts_spans};
    let p = &crate::render::DiffuseTheme::dark().palette.ui;

    // Test with all change types populated
    let counts = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 2,
        notes: 0,
        ..Default::default()
    };

    assert_eq!(format_counts_plain(&counts), "−7 +15 ~6 ≈2");

    let spans = format_counts_spans(&counts, p);
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
        ..Default::default()
    };
    assert_eq!(format_counts_plain(&del_only), "−4");
    let del_spans = format_counts_spans(&del_only, p);
    assert_eq!(del_spans.len(), 1);
    assert_eq!(del_spans[0].content, "−4");
}

#[test]
fn test_format_counts_omit_zero_dels() {
    use crate::view::header::{format_counts_plain, format_counts_spans};
    let p = &crate::render::DiffuseTheme::dark().palette.ui;

    // No del lines: the −0 segment must be omitted like any other zero.
    let adds_only = FileCounts {
        adds: 3,
        dels: 0,
        mods: 1,
        movs: 0,
        notes: 0,
        ..Default::default()
    };
    assert_eq!(format_counts_plain(&adds_only), "+3 ~1");
    let spans = format_counts_spans(&adds_only, p);
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
        ..Default::default()
    };
    assert_eq!(format_counts_plain(&notes_only), "💬2");
}

#[test]
fn test_header_bar_shows_branch_indicator() {
    // Ratatui header with branch — verify spans contain branch
    let line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        None,
        Some("main"),
        None,
        false,
        None,
        usize::MAX,
    );
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
    assert_eq!(branch_span.style.fg, Some(Color::Indexed(231)));
    assert!(branch_span.style.add_modifier.contains(Modifier::BOLD));

    // The branch glyph span must directly precede the branch name with no
    // dot separator span in between.
    let glyph_idx = line
        .spans
        .iter()
        .position(|s| s.content == " \u{2387}")
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
    let line_range = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        None,
        None,
        Some("v1.0..v2.0"),
        false,
        None,
        usize::MAX,
    );
    assert!(line_range.to_string().contains("v1.0..v2.0"));
    assert!(line_range.to_string().contains("\u{2387}"));
}

#[test]
fn test_header_bar_shows_staged_indicator() {
    // When is_staged is true, the header must show `(staged)` after the branch.
    // Ratatui line with is_staged
    let line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        None,
        Some("work"),
        None,
        true,
        None,
        usize::MAX,
    );
    let text = line.to_string();
    assert!(text.contains("work"), "Ratatui line must show branch name");
    assert!(
        text.contains("(staged)"),
        "Ratatui line must show (staged) indicator"
    );

    // Verify staged span styling: "staged" span has Color 120 (green) + bold
    let staged_span = line
        .spans
        .iter()
        .find(|s| s.content == "staged")
        .expect("must have staged span");
    assert_eq!(staged_span.style.fg, Some(Color::Indexed(120)));
    assert!(staged_span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn test_top_header_drops_file_section_and_counts_dot() {
    let global = FileCounts {
        adds: 15,
        dels: 7,
        mods: 6,
        movs: 0,
        notes: 2,
        ..Default::default()
    };
    // Ratatui header renders the exact compact form.
    let line = header_bar_line(
        "diffuse",
        &DiffuseTheme::dark(),
        Some(global),
        Some("main"),
        None,
        false,
        None,
        usize::MAX,
    );
    assert_eq!(
        line.to_string(),
        " diffuse \u{2387}main −7 +15 ~6 💬2",
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
        runs: vec![],
        wholesale: false,
        group: None,
    };
    let _ = MovSide::Tgt;
    let file = OutputFile {
        path: "AGENTS.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
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
            ..Default::default()
        },
        fences: vec![],
        notes: vec![OutputNote {
            id: "n1".to_string(),
            file: "AGENTS.md".to_string(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(1),
            ),
            text: "hunk note".to_string(),
            created_at: 0,
            runs: vec![],
            content_hash: None,
            status: NoteStatus::Open,
            original_source_rows: Vec::new(),
        }],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let hunk_counts = file.hunks[0].counts();
    let ctx = StatusBarContext {
        cur_fi: 0,
        total_files: 41,
        cur_hunk: 0,
        total_hunks: 3,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width: 120,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file: Some(&file),
        hunk_counts: Some(hunk_counts),
        theme: crate::render::DiffuseTheme::dark(),
    };

    // Transient-only status bar omits hunk/file/counts data entirely.
    let line = status_bar_line(ctx);
    assert_eq!(
        line,
        Line::default(),
        "TUI status line must be empty when no transient segments"
    );
}

fn status_test_file(path: &str) -> OutputFile {
    OutputFile {
        path: path.to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 3,
            dels: 0,
            mods: 0,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    }
}

fn status_ctx<'a>(file: Option<&'a OutputFile>, width: usize) -> StatusBarContext<'a> {
    StatusBarContext {
        cur_fi: 1,
        total_files: 25,
        cur_hunk: 0,
        total_hunks: 1,
        raw_diff: false,
        side_by_side: false,
        expanded_mods_count: 0,
        total_mods: 0,
        col_offset: 0,
        num_buffer: "",
        common_prefix: None,
        width,
        is_streaming: false,
        streaming_dot: "●",
        status_message: None,
        file,
        hunk_counts: None,
        theme: crate::render::DiffuseTheme::dark(),
    }
}

#[test]
fn test_status_bar_full_badges() {
    let file = status_test_file("crates/diffuse-core/src/lib.rs");
    let mut ctx = status_ctx(Some(&file), 80);
    ctx.raw_diff = true;
    let line = status_bar_line(ctx);
    assert!(line.to_string().contains("raw diff"));
}

#[test]
fn test_status_bar_abbreviates_badges_on_overflow() {
    let file = status_test_file("crates/diffuse-core/src/lib.rs");
    let mut ctx = status_ctx(Some(&file), 80);
    ctx.raw_diff = true;
    ctx.col_offset = 12;
    ctx.num_buffer = "42";
    let text = status_bar_line(ctx).to_string();
    assert!(
        text.contains("raw diff") && text.contains("col 12+") && text.contains("go 42"),
        "multiple transients must be joined: {text:?}"
    );
}

#[test]
fn test_status_bar_line_abbreviates() {
    let file = status_test_file("crates/diffuse-core/src/lib.rs");
    let mut ctx = status_ctx(Some(&file), 80);
    ctx.raw_diff = true;
    ctx.col_offset = 12;
    ctx.num_buffer = "42";
    let line = status_bar_line(ctx);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains("raw diff") && text.contains("col 12+") && text.contains("go 42"),
        "line must join multiple transients: {text:?}"
    );
}

#[test]
fn test_truncate_status_path_unit_cases() {
    assert_eq!(truncate_status_path("short.rs", 40), None);
    assert_eq!(
        truncate_status_path("123456789012345", 10),
        Some("…789012345".to_string())
    );
    assert_eq!(
        truncate_status_path("crates/diffuse-core/src/lib.rs", 20),
        Some("…use-core/src/lib.rs".to_string())
    );
    assert_eq!(
        truncate_status_path("crates/diffuse-core/src/lib.rs", 10),
        Some("…rc/lib.rs".to_string())
    );
    assert_eq!(
        truncate_status_path("crates/diffuse-core/src/lib.rs", 0),
        None
    );
}

#[test]
fn test_stripped_file_path_rename_and_prefix() {
    let file = OutputFile {
        path: "crates/diffuse-core/src/algo.rs".to_string(),
        old_path: Some("crates/diffuse-core/src/algo/mod.rs".to_string()),
        ..Default::default()
    };
    assert_eq!(
        stripped_file_path(&file, Some("crates/")),
        "diffuse-core/src/algo/mod.rs -> diffuse-core/src/algo.rs"
    );
    assert_eq!(
        stripped_file_path(&file, None),
        "crates/diffuse-core/src/algo/mod.rs -> crates/diffuse-core/src/algo.rs"
    );
}

#[test]
fn test_status_bar_background_styled() {
    let file = status_test_file("src/main.rs");
    let mut ctx = status_ctx(Some(&file), 80);
    ctx.raw_diff = true;

    // Dark theme
    ctx.theme = crate::render::DiffuseTheme::dark();
    let dark_line = status_bar_line(ctx);
    assert_eq!(
        dark_line.style.bg,
        Some(crate::view::to_color(
            crate::render::DiffuseTheme::dark().palette.ui.status_bar_bg
        ))
    );

    // Light theme
    ctx.theme = crate::render::DiffuseTheme::light();
    let light_line = status_bar_line(ctx);
    assert_eq!(
        light_line.style.bg,
        Some(crate::view::to_color(
            crate::render::DiffuseTheme::light()
                .palette
                .ui
                .status_bar_bg
        ))
    );
}

#[test]
fn test_file_status_tag_unmodified_is_empty() {
    let mut file = OutputFile {
        status: diffuse_core::FileStatus::Unmodified,
        ..Default::default()
    };
    assert_eq!(file_status_tag(&file), "");

    file.status = diffuse_core::FileStatus::Modified;
    assert_eq!(file_status_tag(&file), "");

    file.status = diffuse_core::FileStatus::Renamed;
    assert_eq!(file_status_tag(&file), "");

    file.status = diffuse_core::FileStatus::Deleted;
    assert_eq!(file_status_tag(&file), " (deleted)");
}

#[test]
fn test_header_bar_carries_the_file_count() {
    let theme = DiffuseTheme::dark();
    let counts = FileCounts {
        adds: 10,
        mods: 73,
        ..Default::default()
    };

    let line = header_bar_line(
        "diffuse",
        &theme,
        Some(counts),
        Some("work"),
        None,
        false,
        Some(11),
        usize::MAX,
    )
    .to_string();
    let branch_at = line.find("work").expect("header must carry the branch");
    let files_at = line
        .find("11 files")
        .expect("header must carry the file count");
    let counts_at = line
        .find("+10")
        .expect("header must carry the change counts");
    assert!(
        branch_at < files_at && files_at < counts_at,
        "the file count sits between the branch and the counts: {line}"
    );

    let singular = header_bar_line(
        "diffuse",
        &theme,
        None,
        Some("work"),
        None,
        false,
        Some(1),
        usize::MAX,
    )
    .to_string();
    assert!(
        singular.contains("1 file"),
        "one file is singular: {singular}"
    );
    assert!(
        !singular.contains("1 files"),
        "no plural for one file: {singular}"
    );

    let unknown = header_bar_line(
        "diffuse",
        &theme,
        None,
        Some("work"),
        None,
        false,
        None,
        usize::MAX,
    )
    .to_string();
    assert!(
        !unknown.contains("file"),
        "an unknown count is omitted: {unknown}"
    );

    let streaming = header_bar_line(
        "diffuse",
        &theme,
        None,
        Some("work"),
        None,
        false,
        HeaderFileCount::Streaming,
        usize::MAX,
    )
    .to_string();
    assert!(
        streaming.contains("? files"),
        "streaming renders '? files': {streaming}"
    );
}

#[test]
fn test_header_file_count_from_conversions() {
    assert_eq!(HeaderFileCount::from(5), HeaderFileCount::Exact(5));
    assert_eq!(HeaderFileCount::from(Some(3)), HeaderFileCount::Exact(3));
    assert_eq!(HeaderFileCount::from(None), HeaderFileCount::None);
    assert!(HeaderFileCount::Exact(5).is_some());
    assert!(HeaderFileCount::Streaming.is_some());
    assert!(!HeaderFileCount::None.is_some());
    assert_eq!(
        HeaderFileCount::Exact(1).badge_text(),
        Some(" 1 file".to_string())
    );
    assert_eq!(
        HeaderFileCount::Exact(4).badge_text(),
        Some(" 4 files".to_string())
    );
    assert_eq!(
        HeaderFileCount::Streaming.badge_text(),
        Some(" ? files".to_string())
    );
    assert_eq!(HeaderFileCount::None.badge_text(), None);
}

#[test]
fn test_header_bar_drops_the_file_count_before_the_change_counts() {
    let theme = DiffuseTheme::dark();
    let counts = FileCounts {
        adds: 10,
        mods: 73,
        ..Default::default()
    };

    let wide = header_bar_line(
        "diffuse",
        &theme,
        Some(counts),
        Some("work"),
        None,
        false,
        Some(11),
        80,
    )
    .to_string();
    assert!(
        wide.contains("11 files"),
        "a wide header keeps the file count: {wide}"
    );
    assert!(
        wide.contains("+10") && wide.contains("~73"),
        "a wide header keeps the change counts: {wide}"
    );

    let narrow = header_bar_line(
        "diffuse",
        &theme,
        Some(counts),
        Some("work"),
        None,
        false,
        Some(11),
        25,
    )
    .to_string();
    assert!(
        !narrow.contains("11 files"),
        "a narrow header drops the file count: {narrow}"
    );
    assert!(
        narrow.contains("+10") && narrow.contains("~73"),
        "a narrow header keeps the change counts: {narrow}"
    );
}
