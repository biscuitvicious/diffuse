use std::str;

use super::*;
use crate::{
    counts::FileCounts,
    merge::RunKind,
    parse::{FileStatus, GroupId, PeerGroup, RowKind},
    wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput, TokenColor},
};

fn sample_output_file() -> OutputFile {
    OutputFile {
        path: "src/sample.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: FileStatus::Modified,
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
                    runs: vec![OutputRun {
                        kind: RunKind::Ctx,
                        color: TokenColor::Default,
                        sgr: None,
                        text: "fn main() {".into(),
                    }],
                    wholesale: false,
                    group: None,
                },
                OutputRow {
                    kind: RowKind::Mod,
                    mov_side: None,
                    new_line: Some(2),
                    old_line: Some(2),
                    runs: vec![
                        OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "    println!(\"world\");".into(),
                        },
                        OutputRun {
                            kind: RunKind::Add,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "    println!(\"hello\");".into(),
                        },
                    ],
                    wholesale: false,
                    group: None,
                },
                OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(3),
                    old_line: Some(3),
                    runs: vec![OutputRun {
                        kind: RunKind::Ctx,
                        color: TokenColor::Default,
                        sgr: None,
                        text: "}".into(),
                    }],
                    wholesale: false,
                    group: None,
                },
            ],
        }],
        raw_hunks: vec![],
        counts: FileCounts {
            adds: 0,
            dels: 0,
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
    }
}

#[test]
fn test_jsonl_roundtrip_single_file() {
    let original = sample_output_file();
    let mut buffer = Vec::new();
    emit_file_jsonl(&original, &mut buffer).expect("emit jsonl");

    let text = str::from_utf8(&buffer).expect("valid utf8");
    let lines: Vec<&str> = text.lines().collect();

    // 1 FileHeader + 1 HunkHeader + 3 Lines + 1 FileFooter = 6 lines
    assert_eq!(lines.len(), 6);
    // Wire file_header must not contain deprecated "bytes" field
    assert!(!lines[0].contains("\"bytes\""));

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

    let text = str::from_utf8(&buffer).expect("valid utf8");
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
    file.status = FileStatus::Renamed;
    let mut buffer = Vec::new();
    emit_file_jsonl(&file, &mut buffer).expect("emit jsonl");

    let text = str::from_utf8(&buffer).expect("valid utf8");
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
        status: FileStatus::Renamed,
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
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let file2 = OutputFile {
        path: "crates/diffuse-core/src/apply.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: FileStatus::Modified,
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
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
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
        status: FileStatus::Modified,
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
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
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
        status: FileStatus::ModeChanged,
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
        old_bytes: Some(1024),
        new_bytes: Some(1024),
        raw_source: None,
    };
    assert_eq!(file.display_status(), "mode 100644 ➞ 100755, 1kB");

    // Renamed + mode change
    file.old_path = Some("old_script.sh".to_string());
    file.status = FileStatus::Renamed;
    assert_eq!(file.display_status(), "renamed, mode 100644 ➞ 100755, 1kB");

    // Renamed only
    file.old_mode = None;
    file.new_mode = None;
    assert_eq!(file.display_status(), "renamed, 1kB");

    // Standard modified file
    file.old_path = None;
    file.status = FileStatus::Modified;
    assert_eq!(file.display_status(), "1kB");

    // Size changes between old and new: post-image base + delta
    file.old_bytes = Some(1024);
    file.new_bytes = Some(2048);
    assert_eq!(file.display_status(), "2kB +1kB");

    // Negative delta (shrinkage)
    file.old_bytes = Some(2048);
    file.new_bytes = Some(1024);
    assert_eq!(file.display_status(), "1kB −1kB");

    // Small delta on large file
    file.old_bytes = Some(15110);
    file.new_bytes = Some(15115);
    assert_eq!(file.display_status(), "15kB +5B");

    // Added file with sign
    file.status = FileStatus::Added;
    file.old_bytes = None;
    file.new_bytes = Some(1024);
    assert_eq!(file.display_status(), "added, +1kB");

    // Untracked file with sign
    file.status = FileStatus::Untracked;
    assert_eq!(file.display_status(), "untracked, +1kB");

    // Deleted file with sign
    file.status = FileStatus::Deleted;
    file.old_bytes = Some(1024);
    file.new_bytes = None;
    assert_eq!(file.display_status(), "deleted, −1kB");

    // When both old_bytes and new_bytes are None, size is omitted completely
    file.old_bytes = None;
    file.new_bytes = None;

    file.status = FileStatus::Added;
    assert_eq!(file.display_size(), "");
    assert_eq!(file.display_status(), "added");

    file.status = FileStatus::Deleted;
    assert_eq!(file.display_size(), "");
    assert_eq!(file.display_status(), "deleted");

    file.status = FileStatus::Untracked;
    assert_eq!(file.display_size(), "");
    assert_eq!(file.display_status(), "untracked");

    file.status = FileStatus::Modified;
    assert_eq!(file.display_size(), "");
    assert_eq!(file.display_status(), "modified");

    file.is_binary = true;
    assert_eq!(file.display_size(), "");
    assert_eq!(file.display_status(), "binary");
}

#[test]
fn test_display_status_unmodified() {
    let mut file = OutputFile {
        status: FileStatus::Unmodified,
        ..Default::default()
    };
    assert_eq!(file.display_status(), "unmodified");

    file.new_bytes = Some(1024);
    file.old_bytes = Some(1024);
    assert_eq!(file.display_status(), "1kB");

    let stub = OutputFile::collapsed_stub(
        &crate::parse::DiffFile {
            path: "src/untouched.rs".into(),
            status: crate::parse::FileStatus::Unmodified,
            old_path: None,
            old_mode: None,
            new_mode: None,
            is_binary: false,
            has_file_headers: false,
            old_bytes: None,
            new_bytes: None,
            new_line_count: 0,
            old_line_count: 0,
            new_content: Vec::new(),
            old_content: Vec::new(),
            hunks: Vec::new(),
            raw_hunks: Vec::new(),
            no_content: false,
        },
        FileCounts::default(),
        None,
    );
    assert_eq!(stub.status, "unmodified");
    assert_eq!(
        stub.reconstruct().status,
        crate::parse::FileStatus::Unmodified
    );
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
    assert_eq!(format_byte_size(5120), "5kB");
    assert_eq!(format_byte_size(10240), "10kB");
    assert_eq!(format_byte_size(138883072), "132MB");

    // At and above 5.0 the value rounds to an integer, never truncates:
    // 14849 bytes is 14.5009kB, which rounds up, not down to 14kB.
    assert_eq!(format_byte_size(14849), "15kB");
    assert_eq!(format_byte_size(15360), "15kB");

    // Large units
    assert_eq!(format_byte_size(1024 * 1024 * 1024), "1GB");
    assert_eq!(format_byte_size(1024usize.pow(4)), "1TB");
    assert_eq!(format_byte_size(1024usize.pow(5)), "1PB");
}

#[test]
fn test_output_file_release_rendered_memory() {
    let mut file = sample_output_file();
    file.counts.adds = 5;
    file.counts.dels = 2;
    file.rows.push(OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(1),
        old_line: None,
        runs: vec![OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "line".into(),
        }],
        wholesale: false,
        group: None,
    });
    file.new_content = vec!["line".to_string()];
    file.hunks = vec![OutputHunk {
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 1,
        rows: file.rows.clone(),
    }];

    assert!(file.is_loaded());
    assert!(!file.rows.is_empty());
    assert!(!file.hunks.is_empty());
    assert!(!file.new_content.is_empty());

    file.release_rendered_memory();

    assert!(!file.is_loaded());
    assert!(file.rows.is_empty());
    assert!(file.hunks.is_empty());
    assert!(file.raw_hunks.is_empty());
    assert!(file.new_content.is_empty());
    assert!(file.old_content.is_empty());
    assert!(file.content_runs.is_empty());
    assert!(file.fences.is_empty());

    // Necessary metadata preserved
    assert_eq!(file.counts.adds, 5);
    assert_eq!(file.counts.dels, 2);
    assert_eq!(file.path, "src/sample.rs");
    assert_eq!(file.status, "modified");
}

#[test]
fn test_pipeline_output_total_counts() {
    let mut file1 = sample_output_file();
    file1.counts = crate::counts::FileCounts {
        adds: 10,
        dels: 3,
        mods: 2,
        movs: 1,
        notes: 0,
        ..Default::default()
    };

    let mut file2 = sample_output_file();
    file2.counts = crate::counts::FileCounts {
        adds: 5,
        dels: 4,
        mods: 4,
        movs: 1,
        notes: 0,
        ..Default::default()
    };

    let output = PipelineOutput {
        files: vec![file1, file2],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let totals = output.total_counts();
    assert_eq!(totals.adds, 15);
    assert_eq!(totals.dels, 7);
    assert_eq!(totals.mods, 6);
    assert_eq!(totals.movs, 2);
}

#[test]
fn test_output_hunk_counts() {
    use crate::wire::MovSide;
    let mk = |kind: RowKind, mov_side: Option<MovSide>| OutputRow {
        kind,
        mov_side,
        new_line: None,
        old_line: None,
        runs: vec![],
        wholesale: false,
        group: None,
    };
    let hunk = OutputHunk {
        old_start: 1,
        old_count: 4,
        new_start: 1,
        new_count: 4,
        rows: vec![
            mk(RowKind::Del, None),
            mk(RowKind::Add, None),
            mk(RowKind::Mod, None),
            mk(RowKind::Mov, Some(MovSide::Tgt)),
            mk(RowKind::Mov, Some(MovSide::Src)),
            mk(RowKind::Ctx, None),
            mk(RowKind::Sep, None),
        ],
    };
    let counts = hunk.counts();
    assert_eq!(counts.dels, 1);
    assert_eq!(counts.adds, 1);
    assert_eq!(counts.mods, 1);
    assert_eq!(counts.movs, 1);
    assert_eq!(counts.notes, 0);
}

#[test]
fn test_row_group_wire_serialization() {
    // OutputRow with a grouped peer addition
    let row_peer_add = OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(142),
        old_line: None,
        runs: vec![OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "add text".into(),
        }],
        wholesale: false,
        group: Some(PeerGroup {
            id: GroupId(7),
            slot: 2,
            n: 2,
            m: 3,
        }),
    };
    let json_peer_add = serde_json::to_string(&row_peer_add).unwrap();
    assert!(
        json_peer_add.contains(r#""g":{"id":7,"slot":2,"n":2,"m":3}"#),
        "serialized OutputRow must contain peer group info: {json_peer_add}"
    );
    let de_row_peer_add: OutputRow = serde_json::from_str(&json_peer_add).unwrap();
    assert_eq!(de_row_peer_add.group, row_peer_add.group);

    // OutputRow without group: group field is omitted from JSON
    let row_none = OutputRow {
        kind: RowKind::Add,
        mov_side: None,
        new_line: Some(1),
        old_line: None,
        runs: vec![OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "add text".into(),
        }],
        wholesale: false,
        group: None,
    };
    let json_none = serde_json::to_string(&row_none).unwrap();
    assert!(
        !json_none.contains("\"g\""),
        "OutputRow with None group must omit g key entirely: {json_none}"
    );
    let de_row_none: OutputRow = serde_json::from_str(&json_none).unwrap();
    assert_eq!(de_row_none.group, None);

    // OutputRow with Peer group
    let peer_group = PeerGroup {
        id: GroupId(42),
        slot: 1,
        n: 2,
        m: 3,
    };
    let peer_json = serde_json::to_string(&peer_group).unwrap();
    assert_eq!(peer_json, r#"{"id":42,"slot":1,"n":2,"m":3}"#);
    let de_peer: PeerGroup = serde_json::from_str(&peer_json).unwrap();
    assert_eq!(de_peer, peer_group);

    let row_peer = OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        new_line: Some(289),
        old_line: Some(294),
        runs: vec![OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "fn foo()".into(),
        }],
        wholesale: false,
        group: Some(peer_group),
    };
    let json_peer = serde_json::to_string(&row_peer).unwrap();
    assert!(
        json_peer.contains(r#""g":{"id":42,"slot":1,"n":2,"m":3}"#),
        "serialized OutputRow must contain peer group info: {json_peer}"
    );
    let de_row_peer: OutputRow = serde_json::from_str(&json_peer).unwrap();
    assert_eq!(de_row_peer.group, Some(peer_group));
}

#[test]
fn test_compact_wire_run_and_row_serialization() {
    // 1. Bare string scalar for run with no color or SGR
    let run_ctx = OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Default,
        sgr: None,
        text: "hello world".into(),
    };
    let json_ctx = serde_json::to_string(&run_ctx).unwrap();
    assert_eq!(json_ctx, r#"" hello world""#);
    let de_ctx: OutputRun = serde_json::from_str(&json_ctx).unwrap();
    assert_eq!(de_ctx.kind, RunKind::Ctx);
    assert_eq!(de_ctx.text, "hello world");
    assert_eq!(de_ctx.color, TokenColor::Default);
    assert_eq!(de_ctx.sgr, None);

    // 2. 2-element array for run with color
    let run_add = OutputRun {
        kind: RunKind::Add,
        color: TokenColor::Keyword,
        sgr: None,
        text: "fn".into(),
    };
    let json_add = serde_json::to_string(&run_add).unwrap();
    assert_eq!(json_add, r#"["+fn","kw"]"#);
    let de_add: OutputRun = serde_json::from_str(&json_add).unwrap();
    assert_eq!(de_add.kind, RunKind::Add);
    assert_eq!(de_add.text, "fn");
    assert_eq!(de_add.color, TokenColor::Keyword);
    assert_eq!(de_add.sgr, None);

    // 3. 3-element array for run with color and SGR
    let run_del = OutputRun {
        kind: RunKind::Del,
        color: TokenColor::Italic,
        sgr: Some(14),
        text: "header".into(),
    };
    let json_del = serde_json::to_string(&run_del).unwrap();
    assert_eq!(json_del, r#"["-header","ita",14]"#);
    let de_del: OutputRun = serde_json::from_str(&json_del).unwrap();
    assert_eq!(de_del.kind, RunKind::Del);
    assert_eq!(de_del.text, "header");
    assert_eq!(de_del.color, TokenColor::Italic);
    assert_eq!(de_del.sgr, Some(14));

    // 4. Run with SGR but default color serializes color position as null
    let run_sgr_no_color = OutputRun {
        kind: RunKind::Add,
        color: TokenColor::Default,
        sgr: Some(4),
        text: "link".into(),
    };
    let json_sgr = serde_json::to_string(&run_sgr_no_color).unwrap();
    assert_eq!(json_sgr, r#"["+link",null,4]"#);
    let de_sgr: OutputRun = serde_json::from_str(&json_sgr).unwrap();
    assert_eq!(de_sgr.color, TokenColor::Default);
    assert_eq!(de_sgr.sgr, Some(4));

    // 5. null normalizes to TokenColor::Default on deserialize
    let de_null: OutputRun = serde_json::from_str(r#"[" foo",null]"#).unwrap();
    assert_eq!(de_null.color, TokenColor::Default);

    // 6. OutputRow serialization has no text, del_text, spans fields
    let row = OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        old_line: Some(10),
        new_line: Some(12),
        runs: vec![
            OutputRun {
                kind: RunKind::Del,
                color: TokenColor::Default,
                sgr: None,
                text: "old_call();".into(),
            },
            OutputRun {
                kind: RunKind::Add,
                color: TokenColor::Keyword,
                sgr: None,
                text: "new_call();".into(),
            },
        ],
        wholesale: false,
        group: None,
    };
    let row_json = serde_json::to_string(&row).unwrap();
    assert_eq!(
        row_json,
        r#"{"t":"row","k":"mod","ol":10,"nl":12,"r":[["-","old_call();"],["+",["new_call();","kw"]]]}"#
    );

    // Dynamic reconstruction
    let de_row: OutputRow = serde_json::from_str(&row_json).unwrap();
    assert_eq!(de_row.old_text(), "old_call();");
    assert_eq!(de_row.new_text(), "new_call();");
    assert_eq!(de_row.text(), "new_call();");

    // Blank row omits "r"
    let blank_row = OutputRow {
        kind: RowKind::Ctx,
        mov_side: None,
        old_line: Some(10),
        new_line: Some(10),
        runs: vec![],
        wholesale: false,
        group: None,
    };
    let blank_json = serde_json::to_string(&blank_row).unwrap();
    assert_eq!(blank_json, r#"{"t":"row","k":"ctx","ol":10,"nl":10}"#);
    let de_blank: OutputRow = serde_json::from_str(&blank_json).unwrap();
    assert_eq!(de_blank.runs.len(), 0);
    assert_eq!(de_blank.text(), "");

    // Multi-batch row with whitespace absorption and coalescing
    let multi_run_row = OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        old_line: Some(10),
        new_line: Some(10),
        runs: vec![
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Default,
                sgr: None,
                text: "    ".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Keyword,
                sgr: None,
                text: "let".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Default,
                sgr: None,
                text: " ".into(),
            },
            OutputRun {
                kind: RunKind::Del,
                color: TokenColor::Default,
                sgr: None,
                text: "x".into(),
            },
            OutputRun {
                kind: RunKind::Add,
                color: TokenColor::Default,
                sgr: None,
                text: "y".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Default,
                sgr: None,
                text: " ".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Operator,
                sgr: None,
                text: "=".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Default,
                sgr: None,
                text: " ".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Number,
                sgr: None,
                text: "17".into(),
            },
            OutputRun {
                kind: RunKind::Ctx,
                color: TokenColor::Operator,
                sgr: None,
                text: ";".into(),
            },
        ],
        wholesale: false,
        group: None,
    };
    let multi_json = serde_json::to_string(&multi_run_row).unwrap();
    assert_eq!(
        multi_json,
        r#"{"t":"row","k":"mod","ol":10,"nl":10,"r":[[" ","    ",["let ","kw"]],["-","x"],["+","y"],[" ",[" = ","op"],["17","num"],[";","op"]]]}"#
    );
    let de_multi: OutputRow = serde_json::from_str(&multi_json).unwrap();
    assert_eq!(de_multi.old_text(), "    let x = 17;");
    assert_eq!(de_multi.new_text(), "    let y = 17;");

    // WireRecord::Line serializes identically to OutputRow (as a tuple)
    let line_rec = WireRecord::Line(row);
    let line_json = serde_json::to_string(&line_rec).unwrap();
    assert_eq!(line_json, row_json);
    let de_line: WireRecord = serde_json::from_str(&line_json).unwrap();
    assert_eq!(de_line, line_rec);
}
