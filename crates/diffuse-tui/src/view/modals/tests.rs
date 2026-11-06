use super::*;
use crate::{
    core::{
        counts::FileCounts,
        merge::RunKind,
        parse::RowKind,
        wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput},
    },
    finder::FileFinderState,
    render::DiffuseTheme,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

#[test]
fn test_ratatui_help_modal_widget() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_help_modal_widget(f, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("KEY BINDINGS"));
    assert!(text.contains("next / prev hunk"));
    assert!(text.contains("? / Esc close"));

    // Verify centering and scaling (50x16 centered in 80x24 at x=15, y=4)
    let modal_area = help_modal_area(Rect::new(0, 0, 80, 24));
    assert_eq!(modal_area, Rect::new(15, 4, 50, 16));

    // Outside cells must not be part of the modal
    assert_eq!(buffer[(0, 0)].symbol(), " ");
    assert_eq!(buffer[(79, 23)].symbol(), " ");

    // Border corners
    assert_eq!(buffer[(15, 4)].symbol(), "╭");
    assert_eq!(buffer[(64, 4)].symbol(), "╮");
    assert_eq!(buffer[(15, 19)].symbol(), "╰");
    assert_eq!(buffer[(64, 19)].symbol(), "╯");
}

#[test]
fn test_ratatui_help_modal_large_screen_centering() {
    let backend = TestBackend::new(200, 60);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_help_modal_widget(f, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let modal_area = help_modal_area(Rect::new(0, 0, 200, 60));
    // 50% width = 100, 60% height = 36, centered at x=50, y=12
    assert_eq!(modal_area, Rect::new(50, 12, 100, 36));

    // Outside cells must remain empty
    assert_eq!(buffer[(0, 0)].symbol(), " ");
    assert_eq!(buffer[(49, 12)].symbol(), " ");
    assert_eq!(buffer[(150, 12)].symbol(), " ");
    assert_eq!(buffer[(199, 59)].symbol(), " ");

    // Border corners at (50, 12) and (149, 47)
    assert_eq!(buffer[(50, 12)].symbol(), "╭");
    assert_eq!(buffer[(149, 12)].symbol(), "╮");
    assert_eq!(buffer[(50, 47)].symbol(), "╰");
    assert_eq!(buffer[(149, 47)].symbol(), "╯");
}

#[test]
fn test_ratatui_help_modal_visual_layout() {
    let backend = TestBackend::new(60, 80);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_help_modal_widget(f, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..80 {
        let mut line = String::new();
        for x in 0..60 {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    let full_text = lines.join("\n");
    assert!(full_text.contains("KEY BINDINGS"));
    assert!(full_text.contains("j · ↓ / k · ↑"));
    assert!(full_text.contains("next / prev line"));
    assert!(full_text.contains("J · } / K · {"));
    assert!(full_text.contains("next / prev hunk"));
    assert!(full_text.contains("Space · PgDn ·"));
    assert!(full_text.contains("page down"));
    assert!(full_text.contains("Ctrl+d · Ctrl+f"));
    assert!(full_text.contains("PgUp · Ctrl+u ·"));
    assert!(full_text.contains("page up"));
    assert!(full_text.contains("Ctrl+b"));
    assert!(full_text.contains("G · End / g · Home"));
    assert!(full_text.contains("bottom / top"));
    assert!(full_text.contains("n · ] · Tab"));
    assert!(full_text.contains("S-Tab"));
    assert!(full_text.contains("next / prev file"));
    assert!(full_text.contains("Enter"));
    assert!(full_text.contains("toggle file collapse"));
    assert!(full_text.contains("s / u"));
    assert!(full_text.contains("stage / unstage hunk"));
    assert!(full_text.contains("o · O"));
    assert!(full_text.contains("open in editor"));
    assert!(full_text.contains("l · → / h · ←"));
    assert!(full_text.contains("right / left scroll"));
    assert!(full_text.contains("a · +"));
    assert!(full_text.contains("expand all context"));
    assert!(full_text.contains("lines"));
    assert!(full_text.contains("c / C"));
    assert!(full_text.contains("reset hunk / all"));
    assert!(full_text.contains("context"));
    assert!(full_text.contains("m"));
    assert!(full_text.contains("new note"));
    assert!(full_text.contains("d"));
    assert!(full_text.contains("delete note"));
    assert!(full_text.contains("q · Ctrl+c"));
    assert!(full_text.contains("quit"));
    assert!(full_text.contains("? / Esc close"));
}

#[test]
fn test_ratatui_finder_modal_widget() {
    let output = PipelineOutput {
        files: vec![],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let mut finder = FileFinderState::new(&output.files, 0, 0);
    finder.query = "main".to_string();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("FIND FILE"));
    assert!(text.contains("@ main█"));
    assert!(text.contains("No matching files in diff"));
}

fn dummy_row(kind: RowKind, line: u32, text: &str) -> OutputRow {
    let run_kind = match kind {
        RowKind::Del => RunKind::Del,
        RowKind::Add => RunKind::Add,
        _ => RunKind::Ctx,
    };
    OutputRow {
        kind,
        mov_side: None,
        new_line: Some(line),
        old_line: Some(line),
        runs: vec![OutputRun::new(run_kind, text.to_string())],
        wholesale: false,
        group: None,
    }
}

fn sample_finder_output() -> PipelineOutput {
    use crate::core::{counts::FileCounts, wire::OutputFile};
    let file1 = OutputFile {
        path: "TODO.md".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 2,
        old_line_count: 0,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 2,
            dels: 0,
            mods: 0,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(RowKind::Add, 1, "+ First todo item"),
            dummy_row(RowKind::Add, 2, "+ Second todo item"),
        ],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let file2 = OutputFile {
        path: "crates/diffuse-tui/src/app/render.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 127,
        old_line_count: 295,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 127,
            dels: 295,
            mods: 2,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(RowKind::Del, 10, "- old render line"),
            dummy_row(RowKind::Add, 10, "+ new render line"),
        ],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    PipelineOutput {
        files: vec![file1, file2],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    }
}

#[test]
fn test_ratatui_finder_modal_visual_layout() {
    let output = sample_finder_output();
    let finder = FileFinderState::new(&output.files, 0, 0);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    let full_text = lines.join("\n");

    // Top Finder Box
    assert!(full_text.contains("FIND FILE (1/2)"));
    assert!(full_text.contains("@ █"));
    assert!(full_text.contains("▶ TODO.md"));
    assert!(full_text.contains("+2"));
    assert!(!full_text.contains("+2 -0 ~0"));
    assert!(
        full_text.contains("crates/diffuse-tui/src/app/render.rs")
            || full_text.contains("…/diffuse-tui/src/app/render.rs")
    );
    assert!(full_text.contains("−295 +127 ~2"));

    // Bottom Preview Box
    assert!(full_text.contains("PREVIEW"));
    assert!(full_text.contains("+ First todo item"));
    assert!(full_text.contains("+ Second todo item"));

    let modal_area = finder_modal_area(Rect::new(0, 0, 80, 24));
    let add_tint_bg = crate::view::to_color(
        DiffuseTheme::dark().palette[crate::render::Cursor::Rest][crate::render::DiffKind::Add]
            .surface
            .bg,
    );
    let mut found_preview_row = false;
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        if line.contains("First todo item") {
            found_preview_row = true;
            for x in (modal_area.x + 1)..(modal_area.x + modal_area.width - 1) {
                assert_eq!(
                    buffer[(x, y)].bg,
                    add_tint_bg,
                    "cell at ({x}, {y}) must have add_tint_bg across full width of preview modal"
                );
            }
            assert_ne!(buffer[(0, y)].bg, add_tint_bg);
            assert_ne!(buffer[(79, y)].bg, add_tint_bg);
        }
    }
    assert!(
        found_preview_row,
        "preview row containing 'First todo item' must exist"
    );
}

#[test]
fn test_finder_modal_navigation_updates_preview_visual_layout() {
    let output = sample_finder_output();
    let mut finder = FileFinderState::new(&output.files, 0, 0);
    // Move down to select the second file
    finder.move_down();
    assert_eq!(finder.selected_idx, 1);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    let full_text = lines.join("\n");

    // Top Finder Box reflects selection on second file
    assert!(full_text.contains("FIND FILE (2/2)"));
    assert!(full_text.contains("TODO.md"));

    // Bottom Preview Box reflects preview of the second file
    assert!(full_text.contains("PREVIEW"));
    assert!(full_text.contains("- old render line"));
    assert!(full_text.contains("+ new render line"));

    let modal_area = finder_modal_area(Rect::new(0, 0, 80, 24));
    let del_tint_bg = crate::view::to_color(
        DiffuseTheme::dark().palette[crate::render::Cursor::Rest][crate::render::DiffKind::Del]
            .surface
            .bg,
    );
    let add_tint_bg = crate::view::to_color(
        DiffuseTheme::dark().palette[crate::render::Cursor::Rest][crate::render::DiffKind::Add]
            .surface
            .bg,
    );
    let mut found_del = false;
    let mut found_add = false;
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        if line.contains("old render line") {
            found_del = true;
            for x in (modal_area.x + 1)..(modal_area.x + modal_area.width - 1) {
                assert_eq!(
                    buffer[(x, y)].bg,
                    del_tint_bg,
                    "del preview row at ({x}, {y}) must have del_tint_bg to full width"
                );
            }
            assert_ne!(buffer[(0, y)].bg, del_tint_bg);
            assert_ne!(buffer[(79, y)].bg, del_tint_bg);
        }
        if line.contains("new render line") {
            found_add = true;
            for x in (modal_area.x + 1)..(modal_area.x + modal_area.width - 1) {
                assert_eq!(
                    buffer[(x, y)].bg,
                    add_tint_bg,
                    "add preview row at ({x}, {y}) must have add_tint_bg to full width"
                );
            }
            assert_ne!(buffer[(0, y)].bg, add_tint_bg);
            assert_ne!(buffer[(79, y)].bg, add_tint_bg);
        }
    }
    assert!(found_del, "must find deleted preview row");
    assert!(found_add, "must find added preview row");
}

#[test]
fn test_finder_modal_visual_layout() {
    let output = sample_finder_output();
    let finder = FileFinderState::new(&output.files, 0, 0);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut joined = String::new();
    for y in 0..24 {
        for x in 0..80 {
            joined.push_str(buffer[(x, y)].symbol());
        }
        joined.push('\n');
    }
    assert!(joined.contains("FIND FILE (1/2)"));
    assert!(joined.contains('@'));
    assert!(joined.contains("TODO.md"));
    assert!(joined.contains("+2"));
    assert!(!joined.contains("+2 -0 ~0"));
    assert!(joined.contains("First todo item"));
    assert!(joined.contains("Second todo item"));
}

#[test]
fn test_finder_modal_preview_renders_standard_hunk_dividers() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Modified,
        new_line_count: 50,
        old_line_count: 50,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![
            OutputHunk {
                old_start: 5,
                old_count: 1,
                new_start: 5,
                new_count: 2,
                rows: vec![dummy_row(RowKind::Add, 5, "first hunk add")],
            },
            OutputHunk {
                old_start: 30,
                old_count: 2,
                new_start: 31,
                new_count: 1,
                rows: vec![
                    dummy_row(RowKind::Del, 30, "second hunk del"),
                    dummy_row(RowKind::Add, 31, "second hunk add"),
                ],
            },
        ],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 2,
            dels: 1,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(RowKind::Add, 5, "first hunk add"),
            dummy_row(RowKind::Del, 30, "second hunk del"),
            dummy_row(RowKind::Add, 31, "second hunk add"),
        ],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let output = PipelineOutput {
        files: vec![file],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let finder = FileFinderState::new(&output.files, 0, 0);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut full_text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            full_text.push_str(buffer[(x, y)].symbol());
        }
        full_text.push('\n');
    }

    // Must contain both hunk divider badges
    assert!(
        full_text.contains("Hunk 1/2"),
        "preview must show Hunk 1/2 divider: {full_text}"
    );
    assert!(
        full_text.contains("Hunk 2/2"),
        "preview must show Hunk 2/2 divider: {full_text}"
    );
    assert!(full_text.contains("first hunk add"));
    assert!(full_text.contains("second hunk del"));
    assert!(full_text.contains("second hunk add"));

    // Must NOT contain expansion icons
    assert!(
        !full_text.contains('⬍'),
        "divider must not contain expansion icons"
    );
    assert!(
        !full_text.contains('▲'),
        "divider must not contain expansion icons"
    );
    assert!(
        !full_text.contains('▼'),
        "divider must not contain expansion icons"
    );
}

#[test]
fn test_standard_modal_area_scaling_and_centering() {
    use ratatui::layout::Rect;

    let cases = [
        // (input, expected)
        (Rect::new(0, 0, 200, 60), Rect::new(50, 12, 100, 36)),
        (Rect::new(0, 0, 160, 50), Rect::new(40, 10, 80, 30)),
        (Rect::new(0, 0, 100, 30), Rect::new(25, 6, 50, 18)),
        (Rect::new(0, 0, 80, 24), Rect::new(15, 4, 50, 16)),
        (Rect::new(0, 0, 44, 24), Rect::new(0, 4, 44, 16)),
        (Rect::new(0, 0, 80, 14), Rect::new(15, 0, 50, 14)),
        (Rect::new(0, 0, 50, 12), Rect::new(0, 0, 50, 12)),
    ];

    for (input, expected) in cases {
        assert_eq!(standard_modal_area(input), expected);
        assert_eq!(finder_modal_area(input), expected);
        assert_eq!(help_modal_area(input), expected);
    }
}

#[test]
fn test_help_modal_narrow_screen_no_wrapping() {
    // Ratatui backend on a 44x32 terminal
    let backend = TestBackend::new(44, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_help_modal_widget(f, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..32 {
        for x in 0..44 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("KEY BINDINGS"));
    assert!(text.contains("? / Esc"));
}

#[test]
fn test_finder_modal_narrow_screen_no_wrapping() {
    use crate::core::{counts::FileCounts, wire::OutputFile};

    let file = OutputFile {
        path: "crates/diffuse-tui/src/view/diff_grid.rs".to_string(),
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
            adds: 116,
            dels: 0,
            mods: 0,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(
                RowKind::Add,
                10,
                "+ very long addition line exceeding narrow screen width",
            ),
            dummy_row(
                RowKind::Del,
                11,
                "- another long deletion line exceeding width",
            ),
        ],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    let output = PipelineOutput {
        files: vec![file],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let finder = FileFinderState::new(&output.files, 0, 0);

    // Ratatui backend on a 44x24 terminal
    let backend = TestBackend::new(44, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..44 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("FIND FILE"));
    assert!(text.contains("diff_grid.rs") || text.contains("…"));
}

#[test]
fn test_finder_modal_lines_exact_width_wide_and_narrow() {
    use crate::core::{counts::FileCounts, wire::OutputFile};

    let file_specs = [
        ("crates/diffuse-tui/src/app/tests.rs", 73, 0, 0),
        ("crates/diffuse-tui/src/event.rs", 3, 7, 3),
        ("crates/diffuse-tui/src/view/diff_grid.rs", 116, 0, 0),
        ("crates/diffuse-tui/src/view/mod.rs", 3, 0, 2),
        ("crates/diffuse-tui/src/view/modals.rs", 1010, 17, 35),
        ("plan/Ratatui Migration.md", 0, 0, 2),
    ];

    let files: Vec<OutputFile> = file_specs
        .iter()
        .map(|(path, adds, dels, mods)| OutputFile {
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
                adds: *adds,
                dels: *dels,
                mods: *mods,
                movs: 0,
                notes: 0,
                ..Default::default()
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![dummy_row(RowKind::Add, 1, "+ line content")],
            is_binary: false,
            old_bytes: None,
            new_bytes: None,
            raw_source: None,
        })
        .collect();

    let output = PipelineOutput {
        files,
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    // Test across widths: mobile (44), narrow (60), standard (80), laptop
    // (120), ultrawide (322)
    for &w in &[44, 60, 80, 120, 322] {
        for sel in 0..file_specs.len() {
            let mut finder = FileFinderState::new(&output.files, 0, 0);
            finder.selected_idx = sel;

            let backend = TestBackend::new(w, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| {
                    render_finder_modal_widget(
                        f,
                        &output,
                        &finder,
                        &DiffuseTheme::dark(),
                        f.area(),
                    );
                })
                .unwrap();

            let buffer = terminal.backend().buffer();
            let mut text = String::new();
            for y in 0..24 {
                for x in 0..w {
                    text.push_str(buffer[(x, y)].symbol());
                }
                text.push('\n');
            }
            assert!(
                text.contains("FIND FILE"),
                "w={w}, sel={sel}: modal must render its title"
            );
            assert!(
                text.contains("line content"),
                "w={w}, sel={sel}: preview must render the selected file's row"
            );
        }
    }
}

#[test]
fn test_wrap_words() {
    assert_eq!(wrap_words("", 10), vec![""]);
    assert_eq!(
        wrap_words("quick brown fox jumps", 11),
        vec!["quick brown", "fox jumps"]
    );
    assert_eq!(
        wrap_words("new note (ctrl+enter save, esc cancel)", 20),
        vec!["new note (ctrl+enter", "save, esc cancel)"]
    );
    assert_eq!(
        wrap_words("superlongwordexceedingmax", 8),
        vec!["superlon", "gwordexc", "eedingma", "x"]
    );
}

#[test]
fn test_finder_modal_proportional_height_scaling() {
    let output = PipelineOutput {
        files: (0..30)
            .map(|i| crate::core::wire::OutputFile {
                path: format!("file_{i}.rs"),
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
                counts: Default::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: None,
                new_bytes: None,
                raw_source: None,
            })
            .collect(),
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let finder = FileFinderState::new(&output.files, 0, 0);
    let backend = TestBackend::new(100, 60);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..60 {
        for x in 0..100 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("file_0.rs"));
    assert!(text.contains("file_6.rs"));
    assert!(!text.contains("file_7.rs"));

    // Constrained terminal (height 14): exactly 3 visible files (file_0.rs
    // through file_2.rs)
    let backend_constrained = TestBackend::new(80, 14);
    let mut term_constrained = Terminal::new(backend_constrained).unwrap();
    term_constrained
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();
    let buf_constrained = term_constrained.backend().buffer();
    let mut text_constrained = String::new();
    for y in 0..14 {
        for x in 0..80 {
            text_constrained.push_str(buf_constrained[(x, y)].symbol());
        }
        text_constrained.push('\n');
    }
    assert!(text_constrained.contains("file_0.rs"));
    assert!(text_constrained.contains("file_2.rs"));
    assert!(!text_constrained.contains("file_3.rs"));
}

#[test]
fn test_ratatui_note_modal_visual_layout() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(
                f,
                "src/lib.rs:12-18",
                "Fix potential off-by-one error\nEnsure bounds are checked",
                None,
                &DiffuseTheme::dark(),
                f.area(),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    let full_text = lines.join("\n");

    // Verify title components
    assert!(full_text.contains("NOTE on src/lib.rs:12-18"));
    assert!(full_text.contains("Ctrl+S Save"));
    assert!(full_text.contains("Enter ↵"));
    assert!(full_text.contains("Esc Cancel"));

    // Verify multiline content and terminal cursor
    assert!(full_text.contains("Fix potential off-by-one error"));
    assert!(full_text.contains("Ensure bounds are checked"));
    assert!(terminal.get_cursor_position().is_ok());

    // Verify rounded borders rendered
    assert!(full_text.contains('╭') || full_text.contains('┌'));
    assert!(full_text.contains('╰') || full_text.contains('└'));
}

#[test]
fn test_ratatui_note_modal_empty_draft_visual() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(f, "Cargo.toml:5", "", None, &DiffuseTheme::dark(), f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut full_text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            full_text.push_str(buffer[(x, y)].symbol());
        }
        full_text.push('\n');
    }

    assert!(full_text.contains("NOTE on Cargo.toml:5"));
    assert!(
        terminal.get_cursor_position().is_ok(),
        "Must position terminal hardware cursor for empty draft"
    );
}

#[test]
fn test_ratatui_note_modal_constrained_terminal() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(
                f,
                "main.rs:1",
                "short note",
                None,
                &DiffuseTheme::dark(),
                f.area(),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut full_text = String::new();
    for y in 0..10 {
        for x in 0..40 {
            full_text.push_str(buffer[(x, y)].symbol());
        }
        full_text.push('\n');
    }

    assert!(full_text.contains("NOTE on main.rs:1"));
    assert!(full_text.contains("short note"));
}

#[test]
fn test_ratatui_note_modal_long_line_word_wrapping() {
    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let long_line = "This is a very long note text that exceeds the width of the modal and should wrap across multiple visual rows cleanly.";
    terminal
        .draw(|f| {
            render_note_modal_widget(
                f,
                "long.rs:10",
                long_line,
                None,
                &DiffuseTheme::dark(),
                f.area(),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..20 {
        let mut line = String::new();
        for x in 0..60 {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    let full_text = lines.join("\n");
    assert!(full_text.contains("NOTE on long.rs:10"));
    assert!(full_text.contains("This is a very long note text"));
    assert!(full_text.contains("eanly."));
}

#[test]
fn test_ratatui_note_modal_equal_margins_on_small_screens() {
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(
                f,
                "Cargo.lock:81",
                "hello",
                None,
                &DiffuseTheme::dark(),
                f.area(),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Check top border row (where the rounded corners ╭ and ╮ live)
    // Y coordinate of centered modal in 20-height area:
    // Modal height is approx 7..10, centered vertically
    let mut found_border_y = None;
    for y in 0..20 {
        if buffer[(1, y)].symbol() == "╭" {
            found_border_y = Some(y);
            break;
        }
    }

    let y = found_border_y.expect("must find top border ╭ at col 1");
    assert_eq!(
        buffer[(0, y)].symbol(),
        " ",
        "Col 0 must be 1 char left margin"
    );
    assert_eq!(buffer[(1, y)].symbol(), "╭", "Col 1 must be left border ╭");
    assert_eq!(
        buffer[(48, y)].symbol(),
        "╮",
        "Col 48 must be right border ╮"
    );
    assert_eq!(
        buffer[(49, y)].symbol(),
        " ",
        "Col 49 must be 1 char right margin (symmetric!)"
    );
}

#[test]
fn test_finder_modal_counts_format_omits_zero_and_matches_canonical_order() {
    use crate::core::{counts::FileCounts, wire::OutputFile};
    use ratatui::{Terminal, backend::TestBackend};

    fn make_test_file(path: &str, counts: FileCounts) -> OutputFile {
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
            counts,
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

    let files = vec![
        make_test_file(
            "src/full.rs",
            FileCounts {
                dels: 5,
                adds: 10,
                mods: 2,
                movs: 3,
                ..Default::default()
            },
        ),
        make_test_file(
            "src/adds_only.rs",
            FileCounts {
                dels: 0,
                adds: 4,
                mods: 0,
                movs: 0,
                ..Default::default()
            },
        ),
        make_test_file(
            "src/dels_mods.rs",
            FileCounts {
                dels: 2,
                adds: 0,
                mods: 1,
                movs: 0,
                ..Default::default()
            },
        ),
        make_test_file("src/clean.rs", FileCounts::default()),
    ];

    let output = PipelineOutput {
        branch: "main".to_string(),
        untracked_skipped: vec![],
        files,
    };

    let mut finder = FileFinderState::new(&output.files, 0, 0);
    finder.selected_idx = 0; // src/full.rs selected

    let theme = DiffuseTheme::dark();

    // Ratatui widget test
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, &theme, f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut rendered_lines = Vec::new();
    for y in 0..24 {
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buffer[(x, y)].symbol());
        }
        rendered_lines.push(line);
    }
    let full_rendered = rendered_lines.join("\n");
    assert!(full_rendered.contains("−5 +10 ~2 ≈3"));
    assert!(full_rendered.contains("+4"));
    assert!(full_rendered.contains("−2 ~1"));
    assert!(!full_rendered.contains("+4 -0 ~0"));
    assert!(!full_rendered.contains("+0 -2 ~1"));

    // Verify cell color attributes:
    // Selected row (src/full.rs) counts must use modal_selected_fg &
    // modal_selected_bg
    let modal_sel_fg = crate::view::to_color(theme.palette.ui.modal_selected_fg);
    let modal_sel_bg = crate::view::to_color(theme.palette.ui.modal_selected_bg);
    let modal_dim_fg = crate::view::to_color(theme.palette.ui.modal_dim);
    let modal_bg = crate::view::to_color(theme.palette.ui.bg_modal);

    // Find the row containing "−5 +10 ~2 ≈3"
    let mut found_full_row = false;
    let mut found_adds_row = false;
    for y in 0..24 {
        let line = &rendered_lines[y];
        if line.contains("−5 +10 ~2 ≈3") {
            found_full_row = true;
            let byte_idx = line.find('−').unwrap();
            let col = line[..byte_idx].chars().count();
            assert_eq!(buffer[(col as u16, y as u16)].fg, modal_sel_fg);
            assert_eq!(buffer[(col as u16, y as u16)].bg, modal_sel_bg);
        }
        if line.contains("+4") {
            found_adds_row = true;
            let byte_idx = line.find("+4").unwrap();
            let col = line[..byte_idx].chars().count();
            assert_eq!(buffer[(col as u16, y as u16)].fg, modal_dim_fg);
            assert_eq!(buffer[(col as u16, y as u16)].bg, modal_bg);
        }
    }
    assert!(found_full_row, "must find selected row with counts");
    assert!(found_adds_row, "must find unselected row with counts");
}
