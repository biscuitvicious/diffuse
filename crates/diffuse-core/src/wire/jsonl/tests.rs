use super::*;
use crate::{
    counts::FileCounts,
    merge::RunKind,
    parse::{RowKind, Spans},
    wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput},
};

fn sample_output_file() -> OutputFile {
    OutputFile {
        path: "src/sample.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 3,
        old_line_count: 3,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![OutputHunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            rows: vec![
                OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(1),
                    old_line: Some(1),
                    text: "fn main() {".to_string(),
                    del_text: None,
                    spans: Spans::default(),
                    runs: vec![OutputRun {
                        kind: RunKind::Ctx,
                        color: None,
                        sgr: None,
                        text: "fn main() {".into(),
                    }],
                    wholesale: false,
                },
                OutputRow {
                    kind: RowKind::Mod,
                    mov_side: None,
                    new_line: Some(2),
                    old_line: Some(2),
                    text: "    println!(\"hello\");".to_string(),
                    del_text: Some("    println!(\"world\");".to_string()),
                    spans: Spans::default(),
                    runs: vec![OutputRun {
                        kind: RunKind::Common,
                        color: None,
                        sgr: None,
                        text: "    println!(\"hello\");".into(),
                    }],
                    wholesale: false,
                },
                OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(3),
                    old_line: Some(3),
                    text: "}".to_string(),
                    del_text: None,
                    spans: Spans::default(),
                    runs: vec![OutputRun {
                        kind: RunKind::Ctx,
                        color: None,
                        sgr: None,
                        text: "}".into(),
                    }],
                    wholesale: false,
                },
            ],
        }],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 0,
            dels: 0,
            mods: 1,
            movs: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    }
}

#[test]
fn test_jsonl_roundtrip_single_file() {
    let original = sample_output_file();
    let mut buffer = Vec::new();
    emit_file_jsonl(&original, &mut buffer).expect("emit jsonl");

    let text = std::str::from_utf8(&buffer).expect("valid utf8");
    let lines: Vec<&str> = text.lines().collect();

    // 1 FileHeader + 1 HunkHeader + 3 Lines + 1 FileFooter = 6 lines
    assert_eq!(lines.len(), 6);

    let decoded = decode_jsonl_str(text).expect("decode jsonl");
    assert_eq!(decoded.len(), 1);
    let decoded_file = &decoded[0];

    assert_eq!(decoded_file.path, original.path);
    assert_eq!(decoded_file.old_path, original.old_path);
    assert_eq!(decoded_file.status, original.status);
    assert_eq!(decoded_file.new_line_count, original.new_line_count);
    assert_eq!(decoded_file.old_line_count, original.old_line_count);
    assert_eq!(decoded_file.hunks.len(), original.hunks.len());
    assert_eq!(decoded_file.hunks[0].rows, original.hunks[0].rows);
    assert_eq!(decoded_file.counts, original.counts);
}

#[test]
fn test_jsonl_roundtrip_empty_hunks() {
    let mut file = sample_output_file();
    file.hunks.clear();
    let mut buffer = Vec::new();
    emit_file_jsonl(&file, &mut buffer).expect("emit jsonl");

    let text = std::str::from_utf8(&buffer).expect("valid utf8");
    let lines: Vec<&str> = text.lines().collect();
    // 1 FileHeader + 1 FileFooter = 2 lines
    assert_eq!(lines.len(), 2);

    let decoded = decode_jsonl_str(text).expect("decode jsonl");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].path, file.path);
    assert!(decoded[0].hunks.is_empty());
}

#[test]
fn test_jsonl_roundtrip_renamed_file() {
    let mut file = sample_output_file();
    file.path = "src/new.rs".to_string();
    file.old_path = Some("src/old.rs".to_string());
    file.status = "renamed".to_string();
    let mut buffer = Vec::new();
    emit_file_jsonl(&file, &mut buffer).expect("emit jsonl");

    let text = std::str::from_utf8(&buffer).expect("valid utf8");
    let decoded = decode_jsonl_str(text).expect("decode jsonl");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].path, "src/new.rs");
    assert_eq!(decoded[0].old_path.as_deref(), Some("src/old.rs"));
    assert_eq!(decoded[0].status, "renamed");
}

#[test]
fn test_format_rename_path_and_styling() {
    use crate::wire::{format_rename_path, format_rename_path_markdown, format_rename_path_styled};
    assert_eq!(
        format_rename_path(
            "crates/diffuse-core/src/algo/mod.rs",
            "crates/diffuse-core/src/algo.rs"
        ),
        "crates/diffuse-core/src/algo/mod.rs -> algo.rs"
    );
    assert_eq!(
        format_rename_path("src/old.rs", "src/new.rs"),
        "src/old.rs -> new.rs"
    );
    assert_eq!(format_rename_path("same.rs", "same.rs"), "same.rs");

    let styled = format_rename_path_styled(
        "crates/diffuse-core/src/algo/mod.rs",
        "crates/diffuse-core/src/algo.rs",
        "\x1b[39m",
    );
    assert_eq!(
        styled,
        "\x1b[4m\x1b[38;5;139mcrates/diffuse-core/src/\x1b[24m\x1b[39malgo/mod.rs -> algo.rs"
    );

    let md = format_rename_path_markdown(
        "crates/diffuse-core/src/algo/mod.rs",
        "crates/diffuse-core/src/algo.rs",
    );
    assert_eq!(md, "<u>crates/diffuse-core/src/</u>algo/mod.rs -> algo.rs");

    // Root file rename has no common directory prefix and no
    // strikeout/underline
    let root_styled = format_rename_path_styled("old.rs", "new.rs", "\x1b[39m");
    assert_eq!(root_styled, "old.rs -> new.rs");
    assert!(!root_styled.contains("\x1b[9m"));
    assert!(!root_styled.contains("\x1b[4m"));

    let root_md = format_rename_path_markdown("old.rs", "new.rs");
    assert_eq!(root_md, "old.rs -> new.rs");
    assert!(!root_md.contains("~~"));
    assert!(!root_md.contains("<u>"));
}

#[test]
fn test_common_directory_prefix_and_display() {
    let file1 = OutputFile {
        path: "crates/diffuse-core/src/algo.rs".to_string(),
        old_path: Some("crates/diffuse-core/src/algo/mod.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: "renamed".to_string(),
        new_line_count: 0,
        old_line_count: 0,
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
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };
    let file2 = OutputFile {
        path: "crates/diffuse-core/src/apply.rs".to_string(),
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
        counts: Default::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };
    let output = PipelineOutput {
        files: vec![file1.clone(), file2.clone()],
        branch: String::new(),
        untracked_skipped: vec![],
    };

    let prefix = output.common_directory_prefix();
    assert_eq!(prefix.as_deref(), Some("crates/diffuse-core/src/"));

    assert_eq!(
        file1.display_path_with_prefix(prefix.as_deref()),
        "algo/mod.rs -> algo.rs"
    );
    assert_eq!(
        file2.display_path_with_prefix(prefix.as_deref()),
        "apply.rs"
    );

    // If change edits files in the root, common prefix is None and paths are
    // not abbreviated
    let root_file = OutputFile {
        path: "Cargo.toml".to_string(),
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
        counts: Default::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };
    let root_output = PipelineOutput {
        files: vec![file1.clone(), root_file.clone()],
        branch: String::new(),
        untracked_skipped: vec![],
    };
    assert_eq!(root_output.common_directory_prefix(), None);
    assert_eq!(
        file1.display_path_with_prefix(root_output.common_directory_prefix().as_deref()),
        "crates/diffuse-core/src/algo/mod.rs -> algo.rs"
    );
    assert_eq!(
        root_file.display_path_with_prefix(root_output.common_directory_prefix().as_deref()),
        "Cargo.toml"
    );

    // Single file has no common prefix across files
    let single_output = PipelineOutput {
        files: vec![file1.clone()],
        branch: String::new(),
        untracked_skipped: vec![],
    };
    assert_eq!(single_output.common_directory_prefix(), None);
    assert_eq!(
        file1.display_path(),
        "crates/diffuse-core/src/algo/mod.rs -> algo.rs"
    );
}

#[test]
fn test_display_status_mode_change_and_renames() {
    let mut file = OutputFile {
        path: "script.sh".to_string(),
        old_path: None,
        old_mode: Some("100644".to_string()),
        new_mode: Some("100755".to_string()),
        status: "mode changed".to_string(),
        new_line_count: 5,
        old_line_count: 5,
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
        bytes: 1024,
        old_bytes: 1024,
        new_bytes: 1024,
    };
    assert_eq!(file.display_status(), "mode 100644 -> 100755, 1kB");

    // Renamed + mode change
    file.old_path = Some("old_script.sh".to_string());
    file.status = "renamed".to_string();
    assert_eq!(file.display_status(), "mode 100644 -> 100755, 1kB");

    // Renamed only
    file.old_mode = None;
    file.new_mode = None;
    assert_eq!(file.display_status(), "1kB");

    // Standard modified file
    file.old_path = None;
    file.status = "modified".to_string();
    assert_eq!(file.display_status(), "1kB");

    // Size changes between old and new
    file.old_bytes = 1024;
    file.new_bytes = 2048;
    assert_eq!(file.display_status(), "1kB -> 2kB");

    // Added file
    file.status = "added".to_string();
    file.old_bytes = 0;
    file.new_bytes = 1024;
    assert_eq!(file.display_status(), "added, 1kB");

    // Deleted file
    file.status = "deleted".to_string();
    file.old_bytes = 1024;
    file.new_bytes = 0;
    assert_eq!(file.display_status(), "deleted, 1kB");
}

#[test]
fn test_format_byte_size() {
    use crate::wire::format_byte_size;

    // Exact units
    assert_eq!(format_byte_size(0), "0B");
    assert_eq!(format_byte_size(1024), "1kB");
    assert_eq!(format_byte_size(1024 * 1024), "1MB");

    // Small values
    assert_eq!(format_byte_size(511), "511B");
    assert_eq!(format_byte_size(512), "0.5kB");
    assert_eq!(format_byte_size(512 * 1024), "0.5MB");

    // Decimals
    assert_eq!(format_byte_size(1536), "1.5kB");
    assert_eq!(format_byte_size(2427), "2.4kB");
    assert_eq!(format_byte_size(138883072), "132.4MB");

    // Large units
    assert_eq!(format_byte_size(1024 * 1024 * 1024), "1GB");
    assert_eq!(format_byte_size(1024usize.pow(4)), "1TB");
    assert_eq!(format_byte_size(1024usize.pow(5)), "1PB");
}
