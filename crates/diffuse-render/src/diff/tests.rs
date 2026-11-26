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
    // Traditional raw headers carry no counts: the header is just the
    // `---`/`+++` pair in whole-line side colors.
    let single_stripped = strip_ansi(&single_raw);
    assert!(single_stripped.starts_with("--- a/a.txt\n+++ b/a.txt\n"));
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
    assert_eq!(
        crate::ansi::strip_ansi(&raw),
        "--- a/src/old.rs\n+++ b/src/new.rs\n",
        "raw renames read the old path on the minus side: {raw:?}"
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
    // Raw mode is plain unified diff: no note counts, no callouts.
    assert!(
        !strip_ansi(&raw).contains("a note"),
        "raw render must not render note callouts: {raw:?}"
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
    assert!(buf.starts_with("\x1b[48;5;233m\x1b[1m\x1b[38;5;214m"));
    assert!(buf.contains("+5"));
    assert!(buf.contains("−2"));
    assert!(buf.contains("~3"));
    assert!(buf.contains("≈1"));
    assert!(buf.ends_with("\x1b[0m\n"));
    // The canonical banner ends with a gap before the next row.
    assert!(buf.ends_with(" \x1b[0m\n"));
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
        buf_badged.contains("1/2"),
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
    assert!(rendered_light.contains("240;243;248"));
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

    // Raw mode is plain unified diff: no banner or inline callouts.
    let raw = render_raw_file_diff(&file, None, &DiffuseTheme::dark());
    let raw_plain = strip_ansi(&raw);
    assert!(!raw_plain.contains("Unanchored banner note"));
    assert!(!raw_plain.contains("Anchored line note"));
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

    // Raw mode renders no notes, so there is nothing to wrap: the width
    // argument is accepted for API symmetry but ignored.
    let raw_narrow = render_raw_file_diff_with_width(&file, None, &DiffuseTheme::dark(), 40);
    let raw_wide = render_raw_file_diff_with_width(&file, None, &DiffuseTheme::dark(), 160);
    assert_eq!(raw_narrow, raw_wide);
    assert!(!strip_ansi(&raw_narrow).contains('▌'));
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

    // File header must separate filename from badges. Neither replacement is
    // a modification: `old1` and `new1` share only the digit `1`, which is not
    // a salient match, so both hunks keep their deletion and addition.
    assert!(plain.contains("a.txt −2 +2"));

    // Each hunk has a hunk divider
    assert!(plain.contains("──── 1/2"));
    assert!(plain.contains("──── 2/2"));

    // -p hunk headers do not contain the hidden gap / expansion arrows (the
    // file banner above them carries the collapse caret).
    assert!(!plain.contains("more lines"));
    assert!(
        !rendered
            .lines()
            .filter(|line| line.contains("──── 1/2") || line.contains("──── 2/2"))
            .any(|line| line.contains('▼')),
        "hunk headers must not carry expansion arrows"
    );

    // The divider paints the theme canvas instead of inheriting the terminal
    // background: a light terminal running the dark theme would otherwise
    // show a white band across every hunk header.
    let dark_divider = rendered
        .lines()
        .find(|line| line.contains("──── 1/2"))
        .expect("hunk divider line");
    assert!(
        dark_divider.starts_with("\x1b[48;5;233m"),
        "the dark divider must set the theme canvas background: {dark_divider:?}"
    );

    let light = render_file_diff(&out.files[0], None, &DiffuseTheme::light(), true);
    let light_divider = light
        .lines()
        .find(|line| line.contains("──── 1/2"))
        .expect("hunk divider line");
    assert!(
        light_divider.starts_with("\x1b[48;2;250;250;250m"),
        "the light divider must set the theme canvas background: {light_divider:?}"
    );
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
    let header_bg = theme.palette[crate::theme::Cursor::Rest]
        .file_header
        .bg
        .bg_code();

    // Line 0: Header bar
    assert!(lines[0].starts_with(&header_bg));
    // The banner runs out its ruler to the terminal width.
    assert!(lines[0].ends_with("═\x1b[0m"));

    // Lines 3 and 4: the replacement stays a deletion and an addition sharing
    // one line number, because `old1` and `new1` have no salient match.
    assert!(lines[3].contains(&del_bg));
    assert!(lines[3].ends_with(&format!("{del_bg}  \x1b[0m")) || lines[3].ends_with("  \x1b[0m"));
    assert!(strip_ansi(lines[3]).contains("old1"));

    assert!(lines[4].contains(&add_bg));
    assert!(strip_ansi(lines[4]).contains("new1"));

    // Line 5: Context row (ctx2)
    assert!(strip_ansi(lines[5]).contains("ctx2"));

    // Direct check on write_file_header_bar_with_width
    let mut header_buf = String::new();
    write_file_header_bar_with_width(&mut header_buf, &out.files[0], None, &theme, Some(120))
        .unwrap();
    let header_plain = strip_ansi(&header_buf);
    assert_eq!(
        crate::width::display_width(header_plain.trim_end_matches('\n')),
        120
    );
    // The banner extends across the requested width with its ruler fill.
    assert!(header_buf.ends_with("═\x1b[0m\n"));
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
fn test_write_file_header_bar_without_width_ends_with_a_gap() {
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
    assert!(buf.ends_with(" \x1b[0m\n"));
}

#[test]
fn test_render_file_diff_continuation_lines_indented_gutter_width() {
    // NOTE: the pair must be a genuine modification — a fully identical pair
    // normalizes to Ctx and has no `~` gutter. Only the tail word differs, so
    // the asserted slices (shared prefix on both sides) hold however the Mod
    // row renders.
    let long_line_old = format!("{} {}", "abcdefghij".repeat(9), "bcdefghij");
    let long_line_new = format!("{} {}", "abcdefghij".repeat(9), "BCDEFGHIJ");
    let diff = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-{long_line_old}\n+{long_line_new}\n"
    );
    let new_content = Some(
        [("a.txt".to_string(), format!("{long_line_new}\n"))]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), format!("{long_line_old}\n"))]
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
        .filter(|l| !l.contains("a.txt") && !l.contains("──── 1/1"))
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
        &plain0[0..7],
        "    1 ~",
        "segment 0 starts with line number gutter"
    );
    assert_eq!(
        &plain0[7..17],
        "abcdefghij",
        "segment 0 content starts at col 7"
    );

    let plain1 = strip_ansi(diff_lines[1]);
    assert_eq!(
        &plain1[0..7],
        "       ",
        "continuation segment must start with 7 spaces"
    );
    assert_eq!(
        &plain1[7..17],
        "defghijabc",
        "continuation content starts at col 7 with offset 73"
    );
    assert!(
        diff_lines[1].starts_with("\x1b[48;"),
        "continuation line must preserve row background tint"
    );
}

#[test]
fn test_render_file_diff_expanded_mod_continuation_lines_extend_box_glyph() {
    let long_line_old = "old_value_".repeat(10); // 100 characters
    let long_line_new = "new_value_".repeat(10); // 100 characters
    let diff = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-{long_line_old}\n+{long_line_new}\n"
    );
    let new_content = Some(
        [("a.txt".to_string(), format!("{long_line_new}\n"))]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), format!("{long_line_old}\n"))]
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
    let theme = DiffuseTheme::dark();
    let rendered =
        render_file_diff_with_options(&out.files[0], None, &theme, true, width, Some(true), None);

    let lines: Vec<&str> = rendered.lines().collect();
    let diff_lines: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.contains("a.txt") && !l.contains("──── 1/1"))
        .collect();

    assert_eq!(
        diff_lines.len(),
        4,
        "expected 4 segments for expanded wrapped mod row: {diff_lines:#?}"
    );

    // Del half segment 0: starts with "    1┌-"
    let plain_del0 = strip_ansi(diff_lines[0]);
    let prefix_del0: String = plain_del0.chars().take(7).collect();
    assert_eq!(prefix_del0, "    1┌-");

    // Del half segment 1 (continuation): must extend box glyph with "     │ "
    let plain_del1 = strip_ansi(diff_lines[1]);
    let prefix_del1: String = plain_del1.chars().take(7).collect();
    assert_eq!(
        prefix_del1, "     │ ",
        "wrapped del continuation line must extend the box glyph with '│ '"
    );
    // Vertical line │ is colored in mod orange
    let mod_icon_color = theme.palette[crate::theme::Cursor::Rest][crate::theme::DiffKind::Mod]
        .gutter
        .icon
        .color;
    assert!(
        diff_lines[1].contains(&mod_icon_color.fg_code()),
        "continuation vertical bar │ must use mod icon color"
    );

    // Add half segment 0: starts with "    1│+" because the row wraps and does
    // not terminate here
    let plain_add0 = strip_ansi(diff_lines[2]);
    let prefix_add0: String = plain_add0.chars().take(7).collect();
    assert_eq!(prefix_add0, "    1│+");

    // Add half segment 1 (continuation): terminates box, starts with "     └ "
    let plain_add1 = strip_ansi(diff_lines[3]);
    let prefix_add1: String = plain_add1.chars().take(7).collect();
    assert_eq!(
        prefix_add1, "     └ ",
        "wrapped add continuation line closes box and uses '└ '"
    );
    assert!(
        diff_lines[3].contains(&mod_icon_color.fg_code()),
        "continuation bottom corner └ must use mod icon color"
    );
}

#[test]
fn test_render_file_diff_expanded_mod_multi_segment_continuation_box_glyphs() {
    // Both old and new lines are ~180 chars, wrapping into 3 segments each at width
    // 80
    let long_line_old = "old_val_prefix_".repeat(12);
    let long_line_new = "new_val_prefix_".repeat(12);
    let diff = format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-{long_line_old}\n+{long_line_new}\n"
    );
    let new_content = Some(
        [("a.txt".to_string(), format!("{long_line_new}\n"))]
            .into_iter()
            .collect(),
    );
    let old_content = Some(
        [("a.txt".to_string(), format!("{long_line_old}\n"))]
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
    let theme = DiffuseTheme::dark();
    let rendered =
        render_file_diff_with_options(&out.files[0], None, &theme, true, width, Some(true), None);

    let lines: Vec<&str> = rendered.lines().collect();
    let diff_lines: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.contains("a.txt") && !l.contains("──── 1/1"))
        .collect();

    // 3 del segments + 3 add segments = 6 total diff lines
    assert_eq!(
        diff_lines.len(),
        6,
        "expected 6 segments for 3-segment del + 3-segment add: {diff_lines:#?}"
    );

    // Del segment 0: starts with "    1┌-"
    let pfx = |s: &str| -> String { strip_ansi(s).chars().take(7).collect() };
    assert_eq!(pfx(diff_lines[0]), "    1┌-");

    // Del segment 1 (continuation): starts with "     │ "
    assert_eq!(pfx(diff_lines[1]), "     │ ");

    // Del segment 2 (continuation of del row, still mid cluster): starts with "
    // │ "
    assert_eq!(pfx(diff_lines[2]), "     │ ");

    // Add segment 0: starts with "    1│+"
    assert_eq!(pfx(diff_lines[3]), "    1│+");

    // Add segment 1 (intermediate continuation): starts with "     │ "
    assert_eq!(pfx(diff_lines[4]), "     │ ");

    // Add segment 2 (final continuation): starts with "     └ "
    assert_eq!(pfx(diff_lines[5]), "     └ ");

    // Unstyled (plain text) leg verification
    let rendered_plain =
        render_file_diff_with_options(&out.files[0], None, &theme, false, width, Some(true), None);
    let plain_lines: Vec<&str> = rendered_plain
        .lines()
        .filter(|l| !l.contains("a.txt") && !l.contains("──── 1/1"))
        .collect();
    let plain_pfx = |s: &str| -> String { strip_ansi(s).chars().take(7).collect() };
    assert_eq!(plain_pfx(plain_lines[0]), "    1┌-");
    assert_eq!(plain_pfx(plain_lines[1]), "     │ ");
    assert_eq!(plain_pfx(plain_lines[2]), "     │ ");
    assert_eq!(plain_pfx(plain_lines[3]), "    1│+");
    assert_eq!(plain_pfx(plain_lines[4]), "     │ ");
    assert_eq!(plain_pfx(plain_lines[5]), "     └ ");
}

#[test]
fn test_render_file_diff_adjacent_expanded_mods_merged_into_unified_cluster() {
    let diff = r#"diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
-let val_one = 1;
-let val_two = 2;
-let val_three = 3;
+let val_one = 10;
+let val_two = 20;
+let val_three = 30;
"#;
    let new_content = Some(
        [(
            "a.txt".to_string(),
            "let val_one = 10;\nlet val_two = 20;\nlet val_three = 30;\n".to_string(),
        )]
        .into_iter()
        .collect(),
    );
    let old_content = Some(
        [(
            "a.txt".to_string(),
            "let val_one = 1;\nlet val_two = 2;\nlet val_three = 3;\n".to_string(),
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

    let width = 80;
    let theme = DiffuseTheme::dark();
    let rendered =
        render_file_diff_with_options(&out.files[0], None, &theme, true, width, Some(true), None);

    let lines: Vec<&str> = rendered.lines().collect();
    let diff_lines: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.contains("a.txt") && !l.contains("──── 1/1"))
        .collect();

    // 3 dels followed by 3 adds: 6 lines total
    assert_eq!(
        diff_lines.len(),
        6,
        "expected 6 lines for 3 merged adjacent mod rows: {diff_lines:#?}"
    );

    // 3 deletions first
    let p0: String = strip_ansi(diff_lines[0]).chars().take(7).collect();
    let p1: String = strip_ansi(diff_lines[1]).chars().take(7).collect();
    let p2: String = strip_ansi(diff_lines[2]).chars().take(7).collect();
    assert_eq!(p0, "    1┌-");
    assert_eq!(p1, "    2│-");
    assert_eq!(p2, "    3│-");

    // 3 additions second
    let p3: String = strip_ansi(diff_lines[3]).chars().take(7).collect();
    let p4: String = strip_ansi(diff_lines[4]).chars().take(7).collect();
    let p5: String = strip_ansi(diff_lines[5]).chars().take(7).collect();
    assert_eq!(p3, "    1│+");
    assert_eq!(p4, "    2│+");
    assert_eq!(p5, "    3└+");

    // Verify text correspondence
    assert!(diff_lines[0].contains("val_one"));
    assert!(diff_lines[0].contains("1"));
    assert!(diff_lines[1].contains("val_two"));
    assert!(diff_lines[1].contains("2"));
    assert!(diff_lines[2].contains("val_three"));
    assert!(diff_lines[2].contains("3"));

    assert!(diff_lines[3].contains("val_one"));
    assert!(diff_lines[3].contains("10"));
    assert!(diff_lines[4].contains("val_two"));
    assert!(diff_lines[4].contains("20"));
    assert!(diff_lines[5].contains("val_three"));
    assert!(diff_lines[5].contains("30"));
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
    assert!(
        plain_ctx.starts_with("   10  "),
        "ctx gutter: {plain_ctx:?}"
    );
    assert!(plain_ctx.contains("unchanged line"));
    let plain_add = strip_ansi(&styled[1]);
    assert!(
        plain_add.starts_with("   11 +"),
        "add row renders its number and sign: {plain_add:?}"
    );
    assert!(plain_add.contains("added line"));
    assert!(styled[1].starts_with("\x1b["), "styled prefixes carry ANSI");

    // Plain prefixes: bare sign gutters.
    let plain = render_source_rows(&rows, &theme, 0, false);
    let plain0 = strip_ansi(&plain[0]);
    assert!(plain0.starts_with("   10 "), "plain ctx gutter: {plain0:?}");
}

fn banner_test_file() -> OutputFile {
    OutputFile {
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
        counts: diffuse_core::counts::FileCounts {
            adds: 5,
            dels: 2,
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

#[test]
fn test_banner_spans_carry_banner_text_and_roles() {
    use crate::{FileHeaderSpec, span::spans_width, theme::Cursor};

    let theme = DiffuseTheme::dark();
    let file = banner_test_file();
    let banner = file_header_banner(
        &file,
        &FileHeaderSpec {
            badge: Some(Badge {
                idx: 0,
                total: Some(1),
            }),
            is_collapsed: false,
            common_prefix: None,
        },
        &theme,
        Cursor::Rest,
    );
    let spans = banner_spans(&banner, None);
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(
        text, banner.text,
        "spans must concatenate to the banner text"
    );
    // The path run is not bold; the base and count runs are.
    let path = spans
        .iter()
        .find(|span| span.text == "src/main.rs")
        .expect("path span");
    assert!(!path.bold, "path span must not be bold");
    assert_eq!(path.fg, Some(banner.surface.fg_or_path()));
    let base = spans.first().expect("indicator span");
    assert!(base.bold, "indicator span must be bold");
    let count = spans
        .iter()
        .find(|span| span.text == "+5")
        .expect("count span");
    assert!(count.bold, "count span must be bold");
    assert_eq!(count.fg, Some(theme.palette.ui.count_adds));
    // Every span rides on the banner surface background.
    assert!(spans.iter().all(|span| span.bg == Some(banner.surface.bg)));
    assert_eq!(
        spans_width(&spans),
        crate::width::display_width(&banner.text)
    );
}

#[test]
fn test_banner_spans_underline_rename_range() {
    use crate::{FileHeaderSpec, theme::Cursor};

    let theme = DiffuseTheme::dark();
    let mut file = banner_test_file();
    file.old_path = Some("src/old.rs".to_string());
    let banner = file_header_banner(
        &file,
        &FileHeaderSpec {
            badge: None,
            is_collapsed: false,
            common_prefix: None,
        },
        &theme,
        Cursor::Rest,
    );
    // Overlay ranges are visual columns, not byte indices: the banner
    // indicator holds multibyte rules.
    let byte_idx = banner.text.find("src/").expect("prefix in banner");
    let start = crate::width::display_width(&banner.text[..byte_idx]);
    let end = start + "src/".len();
    let spans = banner_spans(&banner, Some((start, end)));
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(text, banner.text, "overlay must not change banner text");
    let underlined: Vec<_> = spans.iter().filter(|span| span.underline).collect();
    assert_eq!(
        underlined.len(),
        1,
        "exactly the prefix range is underlined"
    );
    assert_eq!(underlined[0].text, "src/");
    assert_eq!(underlined[0].fg, Some(yansi::Color::Fixed(139)));
}

/// Raw-mode file headers use traditional unified-diff markers: `---` and `+++`
/// riding whole-line in the file header yellow foreground.
fn raw_header_color() -> String {
    use crate::theme::Cursor;

    let theme = DiffuseTheme::dark();
    theme.palette[Cursor::Rest].file_header.fg.fg_code()
}

fn raw_header_rendered(file: &OutputFile) -> String {
    let mut buf = String::new();
    write_raw_file_header(&mut buf, file, &DiffuseTheme::dark()).unwrap();
    buf
}

#[test]
fn test_raw_file_header_modified_paths() {
    let rendered = raw_header_rendered(&banner_test_file());
    assert_eq!(
        strip_ansi(&rendered),
        "--- a/src/main.rs\n+++ b/src/main.rs\n"
    );
    let header_fg = raw_header_color();
    assert!(
        rendered.contains(&format!("{header_fg}--- a/src/main.rs")),
        "minus line rides whole-line in the file header foreground"
    );
    assert!(
        rendered.contains(&format!("{header_fg}+++ b/src/main.rs")),
        "plus line rides whole-line in the file header foreground"
    );
}

#[test]
fn test_raw_file_header_added_uses_dev_null() {
    let mut file = banner_test_file();
    file.path = "new.txt".to_string();
    file.status = diffuse_core::FileStatus::Added;
    assert_eq!(
        strip_ansi(&raw_header_rendered(&file)),
        "--- /dev/null\n+++ b/new.txt\n"
    );
}

#[test]
fn test_raw_file_header_deleted_uses_dev_null() {
    let mut file = banner_test_file();
    file.path = "old.txt".to_string();
    file.status = diffuse_core::FileStatus::Deleted;
    assert_eq!(
        strip_ansi(&raw_header_rendered(&file)),
        "--- a/old.txt\n+++ /dev/null\n"
    );
}

#[test]
fn test_raw_file_header_rename_uses_old_path() {
    let mut file = banner_test_file();
    file.path = "new.txt".to_string();
    file.old_path = Some("old.txt".to_string());
    file.status = diffuse_core::FileStatus::Renamed;
    assert_eq!(
        strip_ansi(&raw_header_rendered(&file)),
        "--- a/old.txt\n+++ b/new.txt\n"
    );
}

#[test]
fn test_raw_file_header_untracked_uses_dev_null() {
    let mut file = banner_test_file();
    file.path = "fresh.txt".to_string();
    file.status = diffuse_core::FileStatus::Untracked;
    assert_eq!(
        strip_ansi(&raw_header_rendered(&file)),
        "--- /dev/null\n+++ b/fresh.txt\n"
    );
}

#[test]
fn test_raw_pipeline_output_opens_with_traditional_header() {
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
    let stripped = strip_ansi(&rendered);
    let mut lines = stripped.lines();
    assert_eq!(lines.next(), Some("--- a/a.txt"));
    assert_eq!(lines.next(), Some("+++ b/a.txt"));
    assert!(
        !stripped.contains('\u{2550}'),
        "raw output must not carry the banner ruler"
    );
    let hunk_fg = DiffuseTheme::dark().palette[crate::theme::Cursor::Rest]
        .hunk_divider
        .fg
        .fg_code();
    assert!(
        rendered.contains(&format!("{hunk_fg}@@ -1,2 +1,2 @@")),
        "hunk header must be painted in hunk header purple: {rendered:?}"
    );
}

#[test]
fn test_hunk_divider_text_omits_icons() {
    let hunk = OutputHunk {
        old_start: 10,
        old_count: 5,
        new_start: 10,
        new_count: 5,
        rows: vec![],
    };
    let badge = Badge {
        idx: 1,
        total: Some(3),
    };
    let text = hunk_divider_text(badge, &hunk, 8);
    assert_eq!(text, "──── 2/3 · 8 more lines ────");
}

#[test]
fn test_hunk_divider_text_change_counts_without_lines_prefix() {
    use crate::core::{parse::RowKind, wire::OutputRow};
    let mk = |kind: RowKind| OutputRow {
        kind,
        mov_side: None,
        new_line: None,
        old_line: None,
        runs: Vec::new(),
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        old_start: 10,
        old_count: 5,
        new_start: 10,
        new_count: 5,
        rows: vec![
            mk(RowKind::Ctx),
            mk(RowKind::Del),
            mk(RowKind::Add),
            mk(RowKind::Add),
            mk(RowKind::Mod),
        ],
    };
    let badge = Badge {
        idx: 1,
        total: Some(3),
    };
    let text_no_gap = hunk_divider_text(badge, &hunk, 0);
    assert_eq!(text_no_gap, "──── 2/3 −1 +2 ~1 ────");
    assert!(!text_no_gap.contains("lines:"));

    let text_with_gap = hunk_divider_text(badge, &hunk, 8);
    assert_eq!(text_with_gap, "──── 2/3 −1 +2 ~1 · 8 more lines ────");
    assert!(!text_with_gap.contains("lines:"));
}

#[test]
fn test_render_hunk_divider_colors_change_counts() {
    use crate::{
        ansi::ColorExt,
        core::{parse::RowKind, wire::OutputRow},
    };
    let mk = |kind: RowKind| OutputRow {
        kind,
        mov_side: None,
        new_line: None,
        old_line: None,
        runs: Vec::new(),
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        old_start: 10,
        old_count: 5,
        new_start: 10,
        new_count: 5,
        rows: vec![mk(RowKind::Del), mk(RowKind::Add), mk(RowKind::Mod)],
    };
    let badge = Badge {
        idx: 0,
        total: Some(1),
    };
    let theme = DiffuseTheme::dark();
    let rendered = render_hunk_divider(badge, &hunk, 0, 80, &theme);
    assert!(!rendered.contains("lines:"));
    assert!(!rendered.contains("line:"));
    // Change counts must be colored with their ui palette colors
    assert!(rendered.contains(&theme.palette.ui.count_dels.fg_code()));
    assert!(rendered.contains(&theme.palette.ui.count_adds.fg_code()));
    assert!(rendered.contains(&theme.palette.ui.count_mods.fg_code()));
}

#[test]
fn test_render_side_by_side_diff_output() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n ctx line\n-old line\n+new line\n ctx line 2\n";
    let new_content = [(
        "a.txt".to_string(),
        "ctx line\nnew line\nctx line 2\n".to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "a.txt".to_string(),
        "ctx line\nold line\nctx line 2\n".to_string(),
    )]
    .into_iter()
    .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let theme = DiffuseTheme::dark();
    let rendered =
        render_file_diff_with_all_options(&out.files[0], None, &theme, true, 80, None, None, true);
    let stripped = strip_ansi(&rendered);
    assert!(
        stripped.contains('│'),
        "side-by-side pager output must contain center divider"
    );
    assert!(stripped.contains("old line"));
    assert!(stripped.contains("new line"));

    let pipe_rendered =
        render_pipeline_output_with_all_options(&out, &theme, true, 80, None, None, true);
    let pipe_stripped = strip_ansi(&pipe_rendered);
    assert!(pipe_stripped.contains('│'));
}

#[test]
fn test_file_header_reordered_layout() {
    use crate::FileHeaderSpec;

    let mut file = OutputFile {
        path: "app.rs".to_string(),
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
        counts: diffuse_core::counts::FileCounts {
            adds: 2,
            dels: 1,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: Some(9216),
        new_bytes: Some(9216),
        raw_source: None,
    };
    let spec = FileHeaderSpec {
        badge: Some(Badge {
            idx: 0,
            total: Some(7),
        }),
        is_collapsed: false,
        common_prefix: None,
    };
    let text = file_header_text(&file, &spec);
    assert_eq!(text, "════ ▼ app.rs 1/7 −1 +2 (9kB) ");

    // Binary file: no caret
    file.is_binary = true;
    let text_bin = file_header_text(&file, &spec);
    assert_eq!(text_bin, "════ app.rs 1/7 −1 +2 (binary, 9kB) ");
}

#[test]
fn test_file_header_banner_size_delta_plain_coloring() {
    use crate::{FileHeaderSpec, HeaderRole, theme::Cursor};

    let theme = DiffuseTheme::dark();
    let mut file = banner_test_file();
    // Modified file with positive delta: 21,000 -> 24,300 (+3.2kB)
    file.old_bytes = Some(21000);
    file.new_bytes = Some(24300);

    let spec = FileHeaderSpec {
        badge: Some(Badge {
            idx: 0,
            total: Some(1),
        }),
        is_collapsed: false,
        common_prefix: None,
    };
    let banner = file_header_banner(&file, &spec, &theme, Cursor::Rest);
    assert_eq!(banner.text, "════ ▼ src/main.rs 1/1 −2 +5 (24kB +3.2kB) ");

    // Verify status tag uses plain HeaderRole::Path
    let status_span = banner
        .spans
        .iter()
        .find(|s| s.text == " (24kB +3.2kB)")
        .expect("must have status tag span");
    assert_eq!(status_span.role, HeaderRole::Path);

    // Negative delta (shrinkage): 24,300 -> 21,000 (-3.2kB)
    file.old_bytes = Some(24300);
    file.new_bytes = Some(21000);
    let banner_neg = file_header_banner(&file, &spec, &theme, Cursor::Rest);
    assert_eq!(
        banner_neg.text,
        "════ ▼ src/main.rs 1/1 −2 +5 (21kB −3.2kB) "
    );
    let neg_span = banner_neg
        .spans
        .iter()
        .find(|s| s.text == " (21kB −3.2kB)")
        .expect("must have status tag span");
    assert_eq!(neg_span.role, HeaderRole::Path);

    // Added file: +24kB
    file.status = diffuse_core::FileStatus::Added;
    file.old_bytes = None;
    file.new_bytes = Some(24300);
    let banner_add = file_header_banner(&file, &spec, &theme, Cursor::Rest);
    assert_eq!(
        banner_add.text,
        "════ ▼ src/main.rs 1/1 −2 +5 (added, +24kB) "
    );
    let add_span = banner_add
        .spans
        .iter()
        .find(|s| s.text == " (added, +24kB)")
        .expect("must have status tag span");
    assert_eq!(add_span.role, HeaderRole::Path);

    // Deleted file: −21kB
    file.status = diffuse_core::FileStatus::Deleted;
    file.old_bytes = Some(21000);
    file.new_bytes = None;
    let banner_del = file_header_banner(&file, &spec, &theme, Cursor::Rest);
    assert_eq!(
        banner_del.text,
        "════ ▼ src/main.rs 1/1 −2 +5 (deleted, −21kB) "
    );
    let del_span = banner_del
        .spans
        .iter()
        .find(|s| s.text == " (deleted, −21kB)")
        .expect("must have status tag span");
    assert_eq!(del_span.role, HeaderRole::Path);
}
