use super::*;
use crate::{
    Badge, DiffuseTheme,
    ansi::{ColorExt, strip_ansi},
    core::{
        PipelineOptions,
        merge::RunKind,
        parse::RowKind,
        wire::{OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun, PipelineOutput},
    },
};

#[test]
fn test_output_ends_with_terminal_reset() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_pipeline_output(&out, &DiffuseTheme::dark(), true);
    assert!(
        rendered.ends_with("\x1b[0m"),
        "terminal must be left in a clean SGR state"
    );
}

#[test]
fn test_render_raw_pipeline_output() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_raw_pipeline_output(&out, &DiffuseTheme::dark());
    assert!(rendered.ends_with("\x1b[0m"));
    assert!(rendered.contains("a.txt"));
    // Must contain @@ hunk header
    assert!(rendered.contains("@@ -1,2 +1,2 @@"));
    // Must contain colorized number gutter
    assert!(rendered.contains("\x1b[38;5;120m") || rendered.contains("\x1b[38;5;203m"));
    // Must contain colorized change symbols + / - : gutter symbol renders
    // on a colorized line (byte adjacency is not asserted — the optimizer
    // legitimately drops redundant repeats, carrying the color forward).
    let stripped = strip_ansi(&rendered);
    assert!(
        stripped
            .lines()
            .any(|l| l.contains(" 1 +") || l.contains(" 1 -")),
        "raw output must gutter-mark the change, got: {stripped:.300}"
    );
    // No mod ~ symbols in diff rows
    let diff_body: Vec<_> = rendered.lines().skip(1).collect();
    for line in &diff_body {
        assert!(!line.contains("\x1b[38;5;214m~"));
    }

    // Also test with empty raw_hunks fallback
    let mut file_fallback = out.files[0].clone();
    file_fallback.raw_hunks.clear();
    file_fallback.counts.mods = 1;
    file_fallback.counts.movs = 1;
    let single_raw = render_raw_file_diff(&file_fallback, None, &DiffuseTheme::dark());
    assert!(single_raw.contains("~1"));
    assert!(single_raw.contains("moved"));
    assert!(single_raw.contains("@@ -1,2 +1,2 @@"));
}

#[test]
fn test_render_renamed_file_diff() {
    let file = OutputFile {
        path: "src/new.rs".to_string(),
        old_path: Some("src/old.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Renamed,
        new_line_count: 0,
        old_line_count: 0,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: Some(0),
        new_bytes: Some(0),
        raw_source: None,
    };
    let visual = render_file_diff(&file, None, &DiffuseTheme::dark(), true);
    assert!(
        crate::ansi::strip_ansi(&visual).contains("src/old.rs -> new.rs (renamed, 0B)"),
        "zero counts omitted, status parens follow the path: {visual:?}"
    );

    let raw = render_raw_file_diff(&file, None, &DiffuseTheme::dark());
    assert!(
        crate::ansi::strip_ansi(&raw).contains("src/old.rs -> new.rs (renamed, 0B)"),
        "zero counts omitted, status parens follow the path: {raw:?}"
    );
}

#[test]
fn test_zero_notes_not_rendered() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    // Styled render: zero notes must be invisible.
    let rendered = render_file_diff(&out.files[0], None, &DiffuseTheme::dark(), true);
    assert!(
        !rendered.contains("0 note"),
        "styled render must not show '0 notes' when there are no notes"
    );
    // Raw render: same requirement.
    let raw = render_raw_file_diff(&out.files[0], None, &DiffuseTheme::dark());
    assert!(
        !raw.contains("0 note"),
        "raw render must not show '0 notes' when there are no notes"
    );
}

#[test]
fn test_nonzero_notes_still_rendered() {
    let file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![OutputNote {
            id: "n1".to_string(),
            file: "src/lib.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(1),
            ),
            text: "a note".to_string(),
            created_at: 0,
            runs: vec![],
            content_hash: None,
            status: diffuse_core::notes::NoteStatus::Open,
            original_source_rows: Vec::new(),
        }],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let rendered = render_file_diff(&file, None, &DiffuseTheme::dark(), true);
    assert!(
        rendered.contains("1 note"),
        "styled render must show '1 note' when there is 1 note"
    );
    let raw = render_raw_file_diff(&file, None, &DiffuseTheme::dark());
    assert!(
        raw.contains("1 note"),
        "raw render must show '1 note' when there is 1 note"
    );
}

#[test]
fn test_write_file_header_bar_direct_buffer() {
    let mut file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts {
            adds: 5,
            dels: 2,
            mods: 3,
            movs: 1,
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
    let mut buf = String::new();
    write_file_header_bar(&mut buf, &file, None, &DiffuseTheme::dark()).unwrap();
    assert!(buf.starts_with("\x1b[48;5;234m\x1b[1m\x1b[38;5;231m"));
    assert!(buf.contains("+5"));
    assert!(buf.contains("−2"));
    assert!(buf.contains("~3"));
    assert!(buf.contains("≈1"));
    assert!(buf.ends_with("\x1b[0m\n"));
    assert!(!buf.ends_with(" \x1b[0m\n"));
    assert!(!buf.contains("(modified)"));
    let plain = strip_ansi(&buf);
    assert!(plain.contains("src/lib.rs −2 +5 ~3 ≈1"));

    // And with added status:
    file.status = diffuse_core::FileStatus::Added;
    let mut buf_added = String::new();
    write_file_header_bar(&mut buf_added, &file, None, &DiffuseTheme::dark()).unwrap();
    assert!(buf_added.contains("(added)"));
    // Badged variant carries the file position badge.
    let mut buf_badged = String::new();
    write_file_header_bar(
        &mut buf_badged,
        &file,
        Some(Badge {
            idx: 0,
            total: Some(2),
        }),
        &DiffuseTheme::dark(),
    )
    .unwrap();
    assert!(
        buf_badged.contains("File 1/2"),
        "badged banner must show file position: {buf_badged:?}"
    );
}

#[test]
fn test_render_file_diff_resets_background_at_end_of_lines() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line\n+new line\n";
    let new_content = [("a.txt".to_string(), "new line\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_file_diff(&out.files[0], None, &DiffuseTheme::dark(), true);
    let rows: Vec<_> = rendered.lines().skip(1).collect();
    assert!(!rows.is_empty(), "must have rendered diff rows");
    for line in rows {
        assert!(
            line.ends_with("\x1b[0m") || line.ends_with("\x1b[49m"),
            "each diff line must reset background/SGR at end of line to prevent margin color bleeding, but got: {line:?}"
        );
    }
}

#[test]
fn test_render_file_diff_light_theme() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line\n+new line\n";
    let new_content = [("a.txt".to_string(), "new line\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered_dark = render_file_diff(&out.files[0], None, &DiffuseTheme::dark(), true);
    let rendered_light = render_file_diff(&out.files[0], None, &DiffuseTheme::light(), true);

    assert_ne!(rendered_dark, rendered_light);
    assert!(rendered_light.contains("48;2;"));
    assert!(rendered_light.contains("235;238;242"));
}

#[test]
fn test_status_suffix_unmodified_unadorned() {
    let file = diffuse_core::wire::OutputFile {
        path: "src/untouched.rs".to_string(),
        status: diffuse_core::FileStatus::Unmodified,
        ..Default::default()
    };

    let mut buf = String::new();
    write_file_header_bar(&mut buf, &file, None, &DiffuseTheme::dark()).unwrap();
    assert!(!buf.contains("(unmodified)"));
}

#[test]
fn test_render_file_diff_renders_inline_note_callouts() {
    // The anchored add row: the line note's hash derives from it.
    let anchored_add_row = OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(2),
        old_line: None,
        runs: vec![OutputRun::new(RunKind::Add, "line 2".to_string())],
        wholesale: false,
        group: None,
    };
    let line_note_hash =
        diffuse_core::notes::LineHash::of_rows(std::slice::from_ref(&anchored_add_row));
    let file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 2,
        old_line_count: 1,
        new_content: vec!["line 1".to_string(), "line 2".to_string()],
        old_content: vec!["line 1".to_string()],
        hunks: vec![OutputHunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            rows: vec![
                OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(1),
                    old_line: Some(1),
                    runs: vec![OutputRun::new(RunKind::Ctx, "line 1".to_string())],
                    wholesale: false,
                    group: None,
                },
                OutputRow {
                    kind: RowKind::Add,
                    mov_side: None,
                    new_line: Some(2),
                    old_line: None,
                    runs: vec![OutputRun::new(RunKind::Add, "line 2".to_string())],
                    wholesale: false,
                    group: None,
                },
            ],
        }],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![
            OutputNote {
                id: "banner-note".to_string(),
                file: "src/lib.rs".to_string(),
                location: diffuse_core::notes::NoteLocation::line(
                    diffuse_core::notes::DiffCoord::new_side(999),
                ),
                text: "Unanchored banner note".to_string(),
                created_at: 0,
                runs: vec![],
                content_hash: None,
                status: diffuse_core::notes::NoteStatus::Open,
                original_source_rows: Vec::new(),
            },
            OutputNote {
                id: "line-note".to_string(),
                file: "src/lib.rs".to_string(),
                location: diffuse_core::notes::NoteLocation::line(
                    diffuse_core::notes::DiffCoord::new_side(2),
                ),
                text: "Anchored line note".to_string(),
                created_at: 0,
                runs: vec![],
                // Complete structural hash over the anchored add row.
                content_hash: line_note_hash,
                status: diffuse_core::notes::NoteStatus::InProgress,
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

    let rendered = render_file_diff(&file, None, &DiffuseTheme::dark(), true);
    let plain = strip_ansi(&rendered);

    // Banner note must appear right after the header bar, before the first code
    // line
    assert!(plain.contains("▌███ 💬 Unanchored banner note"));
    let banner_pos = plain.find("▌███ 💬 Unanchored banner note").unwrap();
    let code_line_1_pos = plain.find("line 1").unwrap();
    assert!(
        banner_pos < code_line_1_pos,
        "banner note must precede code rows"
    );

    // Line note must appear after line 2
    assert!(plain.contains("▌███ 🔄 Anchored line note"));
    let code_line_2_pos = plain.find("line 2").unwrap();
    let line_note_pos = plain.find("▌███ 🔄 Anchored line note").unwrap();
    assert!(
        code_line_2_pos < line_note_pos,
        "line note must follow target code row"
    );

    // Raw mode diff must also render the inline callouts
    let raw = render_raw_file_diff(&file, None, &DiffuseTheme::dark());
    let raw_plain = strip_ansi(&raw);
    assert!(raw_plain.contains("▌███ 💬 Unanchored banner note"));
    assert!(raw_plain.contains("▌███ 🔄 Anchored line note"));
}

#[test]
fn test_render_file_diff_wraps_notes_at_specified_width() {
    let file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec!["hello world".to_string()],
        old_content: vec!["hello world".to_string()],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![OutputNote {
            id: "wrap-note".to_string(),
            file: "src/lib.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(1),
            ),
            text: "This is a long review note with enough content that it will definitely wrap differently at narrow versus wide widths.".to_string(),
            created_at: 0,
            runs: vec![],
            content_hash: None,
            status: diffuse_core::notes::NoteStatus::Open,
            original_source_rows: Vec::new(),
        }],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let narrow = render_file_diff_with_width(&file, None, &DiffuseTheme::dark(), true, 40);
    let wide = render_file_diff_with_width(&file, None, &DiffuseTheme::dark(), true, 160);

    let narrow_lines: Vec<&str> = narrow.lines().filter(|l| l.contains('▌')).collect();
    let wide_lines: Vec<&str> = wide.lines().filter(|l| l.contains('▌')).collect();

    assert!(narrow_lines.len() > wide_lines.len());
    assert_eq!(wide_lines.len(), 1);

    for line in narrow_lines {
        let plain = strip_ansi(line);
        assert!(crate::width::display_width(&plain) <= 40);
    }

    let pipeline_out = PipelineOutput {
        files: vec![file.clone()],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let p_narrow =
        render_pipeline_output_with_width(&pipeline_out, &DiffuseTheme::dark(), true, 40);
    let p_wide = render_pipeline_output_with_width(&pipeline_out, &DiffuseTheme::dark(), true, 160);
    let p_narrow_lines: Vec<&str> = p_narrow.lines().filter(|l| l.contains('▌')).collect();
    let p_wide_lines: Vec<&str> = p_wide.lines().filter(|l| l.contains('▌')).collect();
    assert!(p_narrow_lines.len() > p_wide_lines.len());

    let raw_narrow = render_raw_file_diff_with_width(&file, None, &DiffuseTheme::dark(), 40);
    let raw_wide = render_raw_file_diff_with_width(&file, None, &DiffuseTheme::dark(), 160);
    let raw_narrow_lines: Vec<&str> = raw_narrow.lines().filter(|l| l.contains('▌')).collect();
    let raw_wide_lines: Vec<&str> = raw_wide.lines().filter(|l| l.contains('▌')).collect();
    assert!(raw_narrow_lines.len() > raw_wide_lines.len());

    let raw_p_narrow =
        render_raw_pipeline_output_with_width(&pipeline_out, &DiffuseTheme::dark(), 40);
    let raw_p_wide =
        render_raw_pipeline_output_with_width(&pipeline_out, &DiffuseTheme::dark(), 160);
    let raw_p_narrow_lines: Vec<&str> = raw_p_narrow.lines().filter(|l| l.contains('▌')).collect();
    let raw_p_wide_lines: Vec<&str> = raw_p_wide.lines().filter(|l| l.contains('▌')).collect();
    assert!(raw_p_narrow_lines.len() > raw_p_wide_lines.len());
}

#[test]
fn test_render_file_diff_outputs_hunk_divider_between_hunks() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n ctx1\n-old1\n+new1\n ctx2\n@@ -20,3 +20,3 @@\n ctx3\n-old2\n+new2\n ctx4\n";
    let new_content = Some(
        [(
            "a.txt".to_string(),
            "ctx1\nnew1\nctx2\nctx3\nnew2\nctx4\n".to_string(),
        )]
        .into_iter()
        .collect(),
    );
    let old_content = Some(
        [(
            "a.txt".to_string(),
            "ctx1\nold1\nctx2\nctx3\nold2\nctx4\n".to_string(),
        )]
        .into_iter()
        .collect(),
    );
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        new_content,
        old_content,
        crate::core::pipeline::PipelineOptions::default(),
    );
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].hunks.len(), 2);

    let rendered = render_file_diff(&out.files[0], None, &DiffuseTheme::dark(), true);
    let plain = strip_ansi(&rendered);

    // File header must separate filename from badges
    assert!(plain.contains("a.txt −2 +2"));

    // Each hunk has a hunk divider
    assert!(plain.contains("──── Hunk 1/2"));
    assert!(plain.contains("──── Hunk 2/2"));

    // -p hunk headers do not contain the hidden gap / expansion arrows
    assert!(!plain.contains("more lines"));
    assert!(!plain.contains('▼'));
}

#[test]
fn test_render_file_diff_extends_background_to_terminal_width() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n ctx1\n-old1\n+new1\n ctx2\n";
    let new_content = Some(
        [("a.txt".to_string(), "ctx1\nnew1\nctx2\n".to_string())]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), "ctx1\nold1\nctx2\n".to_string())]
            .into_iter()
            .collect(),
    );
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        new_content,
        old_content,
        crate::core::pipeline::PipelineOptions::default(),
    );
    assert_eq!(out.files.len(), 1);

    let width = 80;
    let rendered =
        render_file_diff_with_width(&out.files[0], None, &DiffuseTheme::dark(), true, width);

    let lines: Vec<&str> = rendered.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let plain = strip_ansi(line);
        let vis_w = crate::width::display_width(&plain);
        assert_eq!(
            vis_w, width,
            "line {i} visual width {vis_w} does not match expected terminal width {width}: {line:?}"
        );
        assert!(
            line.ends_with("\x1b[0m"),
            "line {i} must reset ANSI styling at end of terminal width: {line:?}"
        );
    }

    // Verify background tints are active on trailing spaces before reset
    let theme = DiffuseTheme::dark();
    let del_bg = theme.row_style(crate::core::parse::RowKind::Del).tint_bg();
    let add_bg = theme.row_style(crate::core::parse::RowKind::Add).tint_bg();
    let header_bg = theme.palette.ui.header_bar_bg.bg_code();

    // Line 0: Header bar
    assert!(lines[0].starts_with(&header_bg));
    assert!(lines[0].ends_with(&"  \x1b[0m".to_string()));

    // Line 3: Deletion row (-old1)
    assert!(lines[3].contains(&del_bg));
    assert!(lines[3].ends_with(&format!("{del_bg}  \x1b[0m")) || lines[3].ends_with("  \x1b[0m"));

    // Line 4: Addition row (+new1)
    assert!(lines[4].contains(&add_bg));
    assert!(lines[4].ends_with(&format!("{add_bg}  \x1b[0m")) || lines[4].ends_with("  \x1b[0m"));

    // Direct check on write_file_header_bar_with_width
    let mut header_buf = String::new();
    write_file_header_bar_with_width(&mut header_buf, &out.files[0], None, &theme, Some(120))
        .unwrap();
    let header_plain = strip_ansi(&header_buf);
    assert_eq!(
        crate::width::display_width(header_plain.trim_end_matches('\n')),
        120
    );
    assert!(header_buf.ends_with(" \x1b[0m\n"));
}

#[test]
fn test_render_file_diff_multiline_wrapping_extends_last_line_to_width() {
    let long_line = "x".repeat(130);
    let diff = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-{long_line}\n+{long_line}\n"
    );
    let new_content = Some(
        [("a.txt".to_string(), format!("{long_line}\n"))]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), format!("{long_line}\n"))]
            .into_iter()
            .collect(),
    );
    let out = crate::core::pipeline::run_pipeline_full(
        &diff,
        new_content,
        old_content,
        crate::core::pipeline::PipelineOptions::default(),
    );
    assert_eq!(out.files.len(), 1);

    let width = 80;
    let rendered =
        render_file_diff_with_width(&out.files[0], None, &DiffuseTheme::dark(), true, width);

    for (i, line) in rendered.lines().enumerate() {
        let plain = strip_ansi(line);
        let vis_w = crate::width::display_width(&plain);
        assert_eq!(
            vis_w % width,
            0,
            "line {i} visual width {vis_w} must be a multiple of terminal width {width} so the last line is fully painted: {line:?}"
        );
        assert!(
            line.ends_with("\x1b[0m"),
            "line {i} must reset ANSI styling at the end of the line: {line:?}"
        );
    }
}

#[test]
fn test_write_file_header_bar_without_width_has_no_trailing_space() {
    let file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let mut buf = String::new();
    write_file_header_bar(&mut buf, &file, None, &DiffuseTheme::dark()).unwrap();
    assert!(buf.ends_with("\x1b[0m\n"));
    assert!(!buf.ends_with(" \x1b[0m\n"));
}

#[test]
fn test_render_file_diff_continuation_lines_indented_six_spaces() {
    let long_line = "abcdefghij".repeat(10); // 100 characters
    let diff = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-{long_line}\n+{long_line}\n"
    );
    let new_content = Some(
        [("a.txt".to_string(), format!("{long_line}\n"))]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), format!("{long_line}\n"))]
            .into_iter()
            .collect(),
    );
    let out = crate::core::pipeline::run_pipeline_full(
        &diff,
        new_content,
        old_content,
        crate::core::pipeline::PipelineOptions::default(),
    );

    let width = 80;
    let rendered =
        render_file_diff_with_width(&out.files[0], None, &DiffuseTheme::dark(), true, width);

    let lines: Vec<&str> = rendered.lines().collect();
    let diff_lines: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.contains("a.txt") && !l.contains("──── Hunk 1/1"))
        .collect();

    assert_eq!(
        diff_lines.len(),
        2,
        "expected 2 segments for 1 wrapped mod row: {diff_lines:#?}"
    );
    for line in &diff_lines {
        let plain = strip_ansi(line);
        assert_eq!(
            crate::width::display_width(&plain),
            width,
            "each segment must be exactly terminal width"
        );
    }

    let plain0 = strip_ansi(diff_lines[0]);
    assert_eq!(
        &plain0[0..6],
        "   1 ~",
        "segment 0 starts with line number gutter"
    );
    assert_eq!(
        &plain0[6..16],
        "abcdefghij",
        "segment 0 content starts at col 6"
    );

    let plain1 = strip_ansi(diff_lines[1]);
    assert_eq!(
        &plain1[0..6],
        "      ",
        "continuation segment must start with 6 spaces"
    );
    assert_eq!(
        &plain1[6..16],
        "efghijabcd",
        "continuation content starts at col 6 with offset 74"
    );
    assert!(
        diff_lines[1].starts_with("\x1b[48;"),
        "continuation line must preserve row background tint"
    );
}

#[test]
fn test_render_source_rows_mirrors_grid_rendering() {
    let theme = DiffuseTheme::dark();
    let rows = vec![
        crate::core::wire::OutputRow {
            kind: crate::core::parse::RowKind::Ctx,
            mov_side: None,
            old_line: Some(10),
            new_line: Some(10),
            runs: vec![OutputRun::new(RunKind::Ctx, "unchanged line".to_string())],
            wholesale: false,
            group: None,
        },
        crate::core::wire::OutputRow {
            kind: crate::core::parse::RowKind::Add,
            mov_side: None,
            old_line: None,
            new_line: Some(11),
            runs: vec![OutputRun::new(RunKind::Add, "added line".to_string())],
            wholesale: false,
            group: None,
        },
    ];

    // Styled prefixes: gutter line numbers and row tints match the diff grid.
    let styled = render_source_rows(&rows, &theme, 0, true);
    assert_eq!(styled.len(), 2, "one rendered line per row");
    let plain_ctx = strip_ansi(&styled[0]);
    assert!(plain_ctx.starts_with("  10  "), "ctx gutter: {plain_ctx:?}");
    assert!(plain_ctx.contains("unchanged line"));
    let plain_add = strip_ansi(&styled[1]);
    assert!(
        plain_add.starts_with("  11 +"),
        "add row renders its number and sign: {plain_add:?}"
    );
    assert!(plain_add.contains("added line"));
    assert!(styled[1].starts_with("\x1b["), "styled prefixes carry ANSI");

    // Plain prefixes: bare sign gutters.
    let plain = render_source_rows(&rows, &theme, 0, false);
    let plain0 = strip_ansi(&plain[0]);
    assert!(plain0.starts_with("  10 "), "plain ctx gutter: {plain0:?}");
}
