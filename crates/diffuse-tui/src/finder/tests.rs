use super::*;
use crate::core::counts::FileCounts;

fn sample_file(path: &str) -> OutputFile {
    OutputFile {
        path: path.to_string(),
        status: "modified".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks: Vec::new(),
        raw_hunks: Vec::new(),
        counts: FileCounts {
            adds: 1,
            dels: 0,
            mods: 0,
            movs: 0,
        },
        fences: Vec::new(),
        notes: Vec::new(),
        content_runs: Vec::new(),
        rows: Vec::new(),
    }
}

#[test]
fn test_file_finder_empty_query_lists_all_files() {
    let files = vec![
        sample_file("src/main.rs"),
        sample_file("src/tui/app.rs"),
        sample_file("Cargo.toml"),
    ];
    let finder = FileFinderState::new(&files, 0, 0);
    assert_eq!(finder.matches.len(), 3);
    assert_eq!(finder.selected_file_idx(), Some(0));
}

#[test]
fn test_file_finder_basename_prefix_ranks_highest() {
    let files = vec![
        sample_file("src/tui/app.rs"),
        sample_file("src/application.rs"),
        sample_file("tests/app_test.rs"),
    ];
    let mut finder = FileFinderState::new(&files, 0, 0);
    finder.query = "app".to_string();
    finder.update_matches(&files);

    assert_eq!(finder.matches.len(), 3);
    // "src/tui/app.rs" and "src/application.rs" have basename prefix "app"
    assert_eq!(finder.matches[0].path, "src/application.rs");
    assert_eq!(finder.matches[1].path, "src/tui/app.rs");
}

#[test]
fn test_file_finder_navigation() {
    let files = vec![
        sample_file("a.rs"),
        sample_file("b.rs"),
        sample_file("c.rs"),
    ];
    let mut finder = FileFinderState::new(&files, 0, 0);
    assert_eq!(finder.selected_file_idx(), Some(0));

    finder.move_down();
    assert_eq!(finder.selected_file_idx(), Some(1));

    finder.move_down();
    assert_eq!(finder.selected_file_idx(), Some(2));

    finder.move_down(); // wrap
    assert_eq!(finder.selected_file_idx(), Some(0));

    finder.move_up(); // wrap back
    assert_eq!(finder.selected_file_idx(), Some(2));
}
