use super::*;
use crate::core::parse::RowKind;
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn test_ratatui_help_modal_widget() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_help_modal_widget(f, f.area());
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
}

#[test]
fn test_ratatui_help_modal_visual_layout() {
    let backend = TestBackend::new(50, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_help_modal_widget(f, f.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..30 {
        let mut line = String::new();
        for x in 0..50 {
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
    assert!(full_text.contains("n · ] / p · ["));
    assert!(full_text.contains("next / prev file"));
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
            render_finder_modal_widget(f, &output, &finder, f.area());
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

fn dummy_row(kind: RowKind, line: u32, text: &str) -> crate::core::wire::OutputRow {
    crate::core::wire::OutputRow {
        kind,
        new_line: Some(line),
        old_line: Some(line),
        text: text.to_string(),
        del_text: None,
        spans: crate::core::parse::Spans::default(),
        runs: vec![],
        wholesale: false,
        moved: false,
    }
}

fn sample_finder_output() -> PipelineOutput {
    use crate::core::{counts::FileCounts, wire::OutputFile};
    let file1 = OutputFile {
        path: "TODO.md".to_string(),
        status: "modified".to_string(),
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
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(RowKind::Add, 1, "+ First todo item"),
            dummy_row(RowKind::Add, 2, "+ Second todo item"),
        ],
    };
    let file2 = OutputFile {
        path: "crates/diffuse-tui/src/app/render.rs".to_string(),
        status: "modified".to_string(),
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
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![
            dummy_row(RowKind::Del, 10, "- old render line"),
            dummy_row(RowKind::Add, 10, "+ new render line"),
        ],
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
            render_finder_modal_widget(f, &output, &finder, f.area());
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
    assert!(full_text.contains("+2 -0 ~0"));
    assert!(
        full_text.contains("crates/diffuse-tui/src/app/render.rs")
            || full_text.contains("…/diffuse-tui/src/app/render.rs")
    );
    assert!(full_text.contains("+127 -295 ~2"));

    // Bottom Preview Box
    assert!(full_text.contains("PREVIEW"));
    assert!(full_text.contains("+ First todo item"));
    assert!(full_text.contains("+ Second todo item"));
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
            render_finder_modal_widget(f, &output, &finder, f.area());
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
}

#[test]
fn test_ansi_finder_modal_visual_layout() {
    let output = sample_finder_output();
    let finder = FileFinderState::new(&output.files, 0, 0);

    let lines = render_finder_modal(&output, &finder, 80, 24);
    assert_eq!(lines.len(), 24);

    let joined = lines.join("\n");
    assert!(joined.contains("FIND FILE (1/2)"));
    assert!(joined.contains("@ █"));
    assert!(joined.contains("TODO.md"));
    assert!(joined.contains("+2 -0 ~0"));
    assert!(joined.contains("First todo item"));
    assert!(joined.contains("Second todo item"));
}

#[test]
fn test_help_modal_narrow_screen_no_wrapping() {
    use crate::view::header::visible_width;

    // Narrow phone width (44 columns, 24 rows)
    let lines = render_help_modal(44, 24);
    assert!(lines.len() >= 24);
    for (i, line) in lines.iter().enumerate() {
        let vw = visible_width(line);
        assert!(
            vw <= 44,
            "line #{i} visual width {vw} exceeds target width 44 on narrow screens: {line}"
        );
    }

    // Test with Ratatui backend on 44x32 terminal
    let backend = TestBackend::new(44, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_help_modal_widget(f, f.area());
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
    use crate::{
        core::{counts::FileCounts, wire::OutputFile},
        view::header::visible_width,
    };

    let file = OutputFile {
        path: "crates/diffuse-tui/src/view/diff_grid.rs".to_string(),
        status: "modified".to_string(),
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
    };
    let output = PipelineOutput {
        files: vec![file],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let finder = FileFinderState::new(&output.files, 0, 0);

    // Narrow phone width (44 columns, 24 rows)
    let lines = render_finder_modal(&output, &finder, 44, 24);
    for (i, line) in lines.iter().enumerate() {
        let vw = visible_width(line);
        assert!(
            vw <= 44,
            "finder line #{i} visual width {vw} exceeds target width 44 on narrow screens: {line}"
        );
    }

    // Test with Ratatui backend on 44x24 terminal
    let backend = TestBackend::new(44, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, f.area());
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
    use crate::{
        core::{counts::FileCounts, wire::OutputFile},
        view::header::visible_width,
    };

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
            status: "modified".to_string(),
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
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![dummy_row(RowKind::Add, 1, "+ line content")],
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

            let lines = render_finder_modal(&output, &finder, w, 24);
            assert_eq!(
                lines.len(),
                24,
                "must produce exactly 24 lines for w={w}, sel={sel}"
            );
            for (i, line) in lines.iter().enumerate() {
                let vw = visible_width(line);
                assert!(
                    vw <= w,
                    "line #{i} visual width {vw} exceeds target width {w} (sel={sel}): {line}"
                );
                assert_eq!(
                    vw, w,
                    "line #{i} visual width {vw} must be exactly padded to width {w} (sel={sel})"
                );
            }
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
fn test_ansi_to_ratatui_line_strikethrough_reset() {
    let ansi = "prefix \x1b[9mstruck text\x1b[29m normal text";
    let line = ansi_to_ratatui_line(ansi);
    assert_eq!(line.spans.len(), 3);
    assert_eq!(line.spans[0].content, "prefix ");
    assert!(
        !line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert_eq!(line.spans[1].content, "struck text");
    assert!(
        line.spans[1]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert_eq!(line.spans[2].content, " normal text");
    assert!(
        !line.spans[2]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
}

#[test]
fn test_finder_modal_proportional_height_scaling() {
    let output = PipelineOutput {
        files: (0..30)
            .map(|i| crate::core::wire::OutputFile {
                path: format!("file_{i}.rs"),
                status: "modified".to_string(),
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
            render_finder_modal_widget(f, &output, &finder, f.area());
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
    assert!(text.contains("file_11.rs"));

    // Constrained terminal (height 14): exactly 3 visible files (file_0.rs
    // through file_2.rs)
    let backend_constrained = TestBackend::new(80, 14);
    let mut term_constrained = Terminal::new(backend_constrained).unwrap();
    term_constrained
        .draw(|f| {
            render_finder_modal_widget(f, &output, &finder, f.area());
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

    // Verify multiline content and cursor
    assert!(full_text.contains("Fix potential off-by-one error"));
    assert!(full_text.contains("Ensure bounds are checked█"));

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
            render_note_modal_widget(f, "Cargo.toml:5", "", f.area());
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
        full_text.contains('█'),
        "Must render cursor for empty draft"
    );
}

#[test]
fn test_ratatui_note_modal_constrained_terminal() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(f, "main.rs:1", "short note", f.area());
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
    assert!(full_text.contains("short note█"));
}

#[test]
fn test_ratatui_note_modal_long_line_word_wrapping() {
    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let long_line = "This is a very long note text that exceeds the width of the modal and should wrap across multiple visual rows cleanly.";
    terminal
        .draw(|f| {
            render_note_modal_widget(f, "long.rs:10", long_line, f.area());
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
    assert!(full_text.contains("cleanly.█"));
}

#[test]
fn test_ratatui_note_modal_equal_margins_on_small_screens() {
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_note_modal_widget(f, "Cargo.lock:81", "hello", f.area());
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
