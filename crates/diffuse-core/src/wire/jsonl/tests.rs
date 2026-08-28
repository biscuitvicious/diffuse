use super::*;
use crate::{
    counts::FileCounts,
    merge::RunKind,
    parse::{RowKind, Spans},
    wire::{OutputFile, OutputHunk, OutputRow, OutputRun},
};

fn sample_output_file() -> OutputFile {
    OutputFile {
        path: "src/sample.rs".to_string(),
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
                    moved: false,
                },
                OutputRow {
                    kind: RowKind::Mod,
                    new_line: Some(2),
                    old_line: Some(2),
                    text: "    println!(\"hello world!\");".to_string(),
                    del_text: Some("    println!(\"hello\");".to_string()),
                    spans: Spans::default(),
                    runs: vec![
                        OutputRun {
                            kind: RunKind::Common,
                            color: None,
                            sgr: None,
                            text: "    println!(\"hello".into(),
                        },
                        OutputRun {
                            kind: RunKind::Add,
                            color: None,
                            sgr: None,
                            text: " world!".into(),
                        },
                        OutputRun {
                            kind: RunKind::Common,
                            color: None,
                            sgr: None,
                            text: "\");".into(),
                        },
                    ],
                    wholesale: false,
                    moved: false,
                },
                OutputRow {
                    kind: RowKind::Ctx,
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
                    moved: false,
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
