//! Unit tests for note persistence, markdown formatting, and querying.

use std::{ffi::OsString, mem, path::PathBuf};

use diffuse_syntax::color::TokenColor;

use super::*;

#[test]
fn test_state_dir_prefers_xdg_state_home() {
    assert_eq!(
        state_dir_from(
            Some(OsString::from("/xdg")),
            Some(OsString::from("/home/u"))
        ),
        PathBuf::from("/xdg/diffuse")
    );
}

#[test]
fn test_state_dir_falls_back_to_local_state() {
    assert_eq!(
        state_dir_from(None, Some(OsString::from("/home/u"))),
        PathBuf::from("/home/u/.local/state/diffuse")
    );
}

#[test]
fn test_state_dir_last_resort_is_cwd() {
    assert_eq!(state_dir_from(None, None), PathBuf::from(".diffuse"));
}

#[test]
fn test_default_notes_path_lives_in_state_dir() {
    let dir = default_state_dir();
    assert_eq!(default_notes_path(), dir.join("notes.json"));
    assert_eq!(default_state_path(), dir.join("state.json"));
}

#[test]
fn test_diff_coord_helpers_and_note_location_constructors() {
    let c_new = DiffCoord::new_side(42);
    assert_eq!(c_new.new_line_num.map(|n| n.get()), Some(42));
    assert_eq!(c_new.old_line_num, None);

    let c_old = DiffCoord::old_side(17);
    assert_eq!(c_old.old_line_num.map(|n| n.get()), Some(17));
    assert_eq!(c_old.new_line_num, None);

    let c_both = DiffCoord::both(17, 42);
    assert_eq!(c_both.old_line_num.map(|n| n.get()), Some(17));
    assert_eq!(c_both.new_line_num.map(|n| n.get()), Some(42));

    let loc_line = NoteLocation::line(c_both);
    assert_eq!(loc_line, NoteLocation::Line { coord: c_both });
    assert_eq!(loc_line.primary_coord(), Some(c_both));

    let loc_range = NoteLocation::range(c_old, c_new);
    assert_eq!(
        loc_range,
        NoteLocation::Range {
            start: c_old,
            end: c_new
        }
    );
    assert_eq!(loc_range.primary_coord(), Some(c_old));

    let loc_file = NoteLocation::File;
    assert_eq!(loc_file.primary_coord(), None);
}

#[test]
fn test_notes_add_and_query() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(10), DiffCoord::new_side(15)),
        text: "Check `this` out".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", "c1", note.clone());

    let repo_notes: Vec<_> = notes_for_file(&db, "myrepo", "c1", "src/main.rs").collect();
    assert_eq!(repo_notes.len(), 1);
    assert_eq!(repo_notes[0].id, "n1");

    let all: Vec<_> = all_notes(&db, "myrepo").collect();
    assert_eq!(all.len(), 1);
}

#[test]
fn test_notes_remove() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(10), DiffCoord::new_side(15)),
        text: "First".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(20), DiffCoord::new_side(25)),
        text: "Second".into(),
        created_at: 1001,
        content_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", "c1", note1);
    add_note(&mut db, "myrepo", "c1", note2);

    let removed = remove_notes_in_range(&mut db, "myrepo", "c1", "src/main.rs", 12, 18);
    assert_eq!(removed, 1);

    let remaining: Vec<_> = notes_for_file(&db, "myrepo", "c1", "src/main.rs").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
}

#[test]
fn test_remove_note_by_id_targeted() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(10), DiffCoord::new_side(15)),
        text: "First".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(10), DiffCoord::new_side(15)),
        text: "Second (same line)".into(),
        created_at: 1001,
        content_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", "c1", note1);
    add_note(&mut db, "myrepo", "c1", note2);

    // Target specifically note1
    assert!(remove_note_by_id(
        &mut db,
        "myrepo",
        "c1",
        "src/main.rs",
        "n1"
    ));
    let remaining: Vec<_> = notes_for_file(&db, "myrepo", "c1", "src/main.rs").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
    assert_eq!(remaining[0].text, "Second (same line)");

    // Non-existent ID, rev, or repo returns false
    assert!(!remove_note_by_id(
        &mut db,
        "myrepo",
        "c1",
        "src/main.rs",
        "nonexistent"
    ));
    assert!(!remove_note_by_id(
        &mut db,
        "unknown_repo",
        "c1",
        "src/main.rs",
        "n2"
    ));
    assert!(!remove_note_by_id(
        &mut db,
        "myrepo",
        "unknown_rev",
        "src/main.rs",
        "n2"
    ));
    assert!(!remove_note_by_id(
        &mut db,
        "myrepo",
        "c1",
        "unknown_file.rs",
        "n2"
    ));

    // Removing last note cleans up file entry in DB
    assert!(remove_note_by_id(
        &mut db,
        "myrepo",
        "c1",
        "src/main.rs",
        "n2"
    ));
    assert_eq!(
        notes_for_file(&db, "myrepo", "c1", "src/main.rs").count(),
        0
    );
    assert!(!db.get("myrepo").unwrap().contains_key("c1"));
}

#[test]
fn test_update_note_text_targeted() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(10), DiffCoord::new_side(15)),
        text: "Initial text".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo", "c1", note);

    assert!(update_note_text(
        &mut db,
        "myrepo",
        "c1",
        "src/main.rs",
        "n1",
        "Updated text".into()
    ));

    let updated: Vec<_> = notes_for_file(&db, "myrepo", "c1", "src/main.rs").collect();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "n1");
    assert_eq!(updated[0].text, "Updated text");
    assert_eq!(updated[0].start(), 10);
    assert_eq!(updated[0].end(), 15);
    assert_eq!(updated[0].created_at, 1000);

    // Non-existent ID, file, rev, or repo returns false
    assert!(!update_note_text(
        &mut db,
        "myrepo",
        "c1",
        "src/main.rs",
        "nonexistent",
        "New text".into()
    ));
    assert!(!update_note_text(
        &mut db,
        "unknown_repo",
        "c1",
        "src/main.rs",
        "n1",
        "New text".into()
    ));
    assert!(!update_note_text(
        &mut db,
        "myrepo",
        "unknown_rev",
        "src/main.rs",
        "n1",
        "New text".into()
    ));
    assert!(!update_note_text(
        &mut db,
        "myrepo",
        "c1",
        "other.rs",
        "n1",
        "New text".into()
    ));
}

#[test]
fn test_notes_isolated_across_heads() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(1)),
        text: "head-scoped".into(),
        created_at: 1,
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo", "h1", note);

    assert_eq!(notes_for_file(&db, "myrepo", "h1", "lib.rs").count(), 1);
    // Bare repo or a different head must not see the note.
    assert_eq!(notes_for_file(&db, "myrepo", "", "lib.rs").count(), 0);
    assert_eq!(notes_for_file(&db, "myrepo", "h2", "lib.rs").count(), 0);
}

#[test]
fn test_head_commit_resolves_temp_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // Not a repo yet: no HEAD.
    assert_eq!(head_commit(dir), None);

    let repo = diffuse_git::testing::init_repo(dir).unwrap();
    // No commits yet: no HEAD.
    assert_eq!(head_commit(dir), None);

    diffuse_git::testing::commit_files(&repo, "commit1", &[("a.rs", "fn a() {}\n")]).unwrap();

    let head = head_commit(dir).expect("HEAD must resolve after a commit");
    assert_eq!(head.len(), 40);
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_notes_commit_for_ranges() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = diffuse_git::testing::init_repo(dir).unwrap();

    diffuse_git::testing::commit_files(&repo, "commit1", &[("a.rs", "1\n")]).unwrap();
    let c1 = head_commit(dir).unwrap();

    diffuse_git::testing::commit_files(&repo, "commit2", &[("a.rs", "2\n")]).unwrap();
    let c2 = head_commit(dir).unwrap();

    diffuse_git::testing::commit_files(&repo, "commit3", &[("a.rs", "3\n")]).unwrap();
    let c3 = head_commit(dir).unwrap();

    diffuse_git::testing::commit_files(&repo, "commit4", &[("a.rs", "4\n")]).unwrap();
    let c4 = head_commit(dir).unwrap();

    // 1. Working tree / no range -> keys off HEAD (c4)
    assert_eq!(notes_commit(dir, &[]), Some(c4.clone()));

    // 2. Single-commit range c1..c2 -> next newest in range is c1 (commit before
    //    c2)
    let r_1_2 = vec![format!("{c1}..{c2}")];
    assert_eq!(notes_commit(dir, &r_1_2), Some(c1.clone()));

    // 3. Multi-commit range c1..c4 (commits: c2, c3, c4) -> base c1, parent of tip
    //    c4 is c3 -> c1..c3
    let r_1_4 = vec![format!("{c1}..{c4}")];
    assert_eq!(notes_commit(dir, &r_1_4), Some(format!("{c1}..{c3}")));

    // 4. Single-commit notation c3^! -> commit before c3 is c2
    let r_c3_head = vec![format!("{c3}^!")];
    assert_eq!(notes_commit(dir, &r_c3_head), Some(c2.clone()));

    // 5. Two ref arguments [c1, c3] (commits: c2, c3) -> base c1, parent of tip c3
    //    is c2 -> c1..c2
    let r_two_refs = vec![c1.clone(), c3.clone()];
    assert_eq!(notes_commit(dir, &r_two_refs), Some(format!("{c1}..{c2}")));
}

#[test]
fn test_commit_spec_matches() {
    use super::commit_spec_matches;

    // Both single commits: prefix matching in both directions
    assert!(commit_spec_matches("abc12345", "abc12"));
    assert!(commit_spec_matches("abc12", "abc12345"));
    assert!(commit_spec_matches("abc12345", "abc12345"));
    assert!(!commit_spec_matches("abc12345", "def678"));

    // Range vs single commit: never matches
    assert!(!commit_spec_matches("abc12..def34", "abc12"));
    assert!(!commit_spec_matches("abc12", "abc12..def34"));

    // Both ranges: both base and tip must match
    assert!(commit_spec_matches("abc12345..def67890", "abc12..def67"));
    assert!(commit_spec_matches("abc12..def67", "abc12345..def67890"));
    assert!(!commit_spec_matches("abc12345..def67890", "xyz99..def67"));
    assert!(!commit_spec_matches("abc12345..def67890", "abc12..xyz99"));
}

#[test]
fn test_notes_persistence() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let file = tmp.path();

    let mut db = NotesDB::new();
    add_note(
        &mut db,
        "repo1",
        "c1",
        ReviewNote {
            id: "n1".into(),
            file: "lib.rs".into(),
            location: NoteLocation::line(DiffCoord::old_side(1)),
            text: "**important**".into(),
            created_at: 500,
            content_hash: None,
            ..Default::default()
        },
    );

    save_notes(file, &db).unwrap();

    let loaded = load_notes(file);
    assert_eq!(loaded.len(), 1);
    let loaded_notes: Vec<_> = notes_for_file(&loaded, "repo1", "c1", "lib.rs").collect();
    assert_eq!(loaded_notes.len(), 1);
    assert_eq!(loaded_notes[0].text, "**important**");
}

#[test]
fn test_note_runs_inline() {
    let text = "This is **bold** and `code` span";
    let runs = note_runs(text);

    let texts: Vec<&str> = runs.iter().map(|t| t.text.as_ref()).collect();
    assert_eq!(
        texts,
        vec![
            "This is ", "**", "bold", "**", " and ", "`", "code", "`", " span"
        ]
    );

    let colors: Vec<TokenColor> = runs.iter().map(|t| t.color).collect();
    assert_eq!(colors[0], TokenColor::Default);
    assert_eq!(colors[2], TokenColor::Bold);
    assert_eq!(colors[6], TokenColor::Code);
}

#[test]
fn test_note_runs_underscores_and_asterisks() {
    let text = "**Markdown** __note!__ and *italic* _also italic_";
    let runs = note_runs(text);

    let texts: Vec<&str> = runs.iter().map(|t| t.text.as_ref()).collect();
    assert_eq!(
        texts,
        vec![
            "**",
            "Markdown",
            "**",
            " ",
            "__",
            "note!",
            "__",
            " and ",
            "*",
            "italic",
            "*",
            " ",
            "_",
            "also italic",
            "_"
        ]
    );

    let colors: Vec<TokenColor> = runs.iter().map(|t| t.color).collect();
    assert_eq!(
        colors,
        vec![
            TokenColor::Delimiter,
            TokenColor::Bold,
            TokenColor::Delimiter,
            TokenColor::Default,
            TokenColor::Delimiter,
            TokenColor::Bold,
            TokenColor::Delimiter,
            TokenColor::Default,
            TokenColor::Delimiter,
            TokenColor::Italic,
            TokenColor::Delimiter,
            TokenColor::Default,
            TokenColor::Delimiter,
            TokenColor::Italic,
            TokenColor::Delimiter,
        ]
    );
}

#[test]
fn test_normalize_note_text_multiline() {
    let single = "Single line without trailing whitespace";
    let norm_single = normalize_note_text(single);
    assert!(
        matches!(norm_single, Cow::Borrowed(_)),
        "Single line should return Cow::Borrowed"
    );
    assert_eq!(norm_single, single);

    let single_word = "  Word   ";
    let norm_word = normalize_note_text(single_word);
    assert!(
        matches!(norm_word, Cow::Borrowed(_)),
        "Single word should return Cow::Borrowed slice"
    );
    assert_eq!(norm_word, "Word");

    let single_trailing = "   Single line with trailing whitespace   ";
    let norm_trailing = normalize_note_text(single_trailing);
    assert!(
        matches!(norm_trailing, Cow::Borrowed(_)),
        "Trimmed multi-word line without internal whitespace runs should return Cow::Borrowed subslice"
    );
    assert_eq!(norm_trailing, "Single line with trailing whitespace");

    let multi_whitespace =
        "  Hello   world\t\twith   multiple   spaces  \n  and\ttabs\n\nand newlines   ";
    let norm_ws = normalize_note_text(multi_whitespace);
    assert!(
        matches!(norm_ws, Cow::Owned(_)),
        "Collapsed whitespace should return Cow::Owned"
    );
    assert_eq!(
        norm_ws,
        "Hello world with multiple spaces and tabs and newlines"
    );
}

#[test]
fn test_note_runs_multiline_html_behavior() {
    let input = "  Hello   `world`  \n\nand   **bold text**  ";
    let runs = note_runs(input);
    let full_rendered: String = runs.iter().map(|r| r.text.as_ref()).collect();
    assert_eq!(full_rendered, "Hello `world` and **bold text**");
}

#[test]
fn test_line_hash_zero_cost_option_representation() {
    // Zero-cost representation: Option<LineHash> occupies 4 bytes with no
    // discriminant overhead
    assert_eq!(mem::size_of::<LineHash>(), 4);
    assert_eq!(mem::size_of::<Option<LineHash>>(), 4);
    assert_eq!(mem::align_of::<Option<LineHash>>(), 4);
}

#[test]
fn test_line_hash_encapsulation_behaviour() {
    // Parsing from hex roundtrip
    let hash = LineHash::from_hex("deadbeef").unwrap();
    let hex_str = hash.to_hex();
    let parsed = LineHash::from_hex(&hex_str).unwrap();
    assert_eq!(hash, parsed);
    assert_eq!(hash.get(), parsed.get());

    // Non-zero guarantee: zero wraps to 1
    let zero_hash = LineHash::from_hex("00000000").unwrap();
    assert_eq!(zero_hash.get(), 1);
}

#[test]
fn test_row_aware_hash_and_anchoring() {
    let run = |kind: crate::merge::RunKind, text: &str| crate::wire::OutputRun {
        kind,
        color: TokenColor::Default,
        sgr: None,
        text: text.to_owned().into(),
    };
    let row = |kind: crate::parse::RowKind, old: Option<u32>, new: Option<u32>, text: &str| {
        crate::wire::OutputRow {
            kind,
            mov_side: None,
            old_line: old,
            new_line: new,
            runs: vec![run(crate::merge::RunKind::Ctx, text)],
            wholesale: false,
            group: None,
        }
    };

    // 1. Context-line gate (gap notes): the stored rows must reproduce the file
    //    content at the location's exact primary coordinates.
    let file_lines = vec![
        "// header".to_string(),
        "fn foo() {".to_string(),
        "    let x = 42;".to_string(),
        "}".to_string(),
        "// footer".to_string(),
    ];
    let ctx2 = row(crate::parse::RowKind::Ctx, Some(2), Some(2), "fn foo() {");
    let ctx3 = row(
        crate::parse::RowKind::Ctx,
        Some(3),
        Some(3),
        "    let x = 42;",
    );
    let ctx4 = row(crate::parse::RowKind::Ctx, Some(4), Some(4), "}");

    let note_multi = ReviewNote {
        id: "n_multi".into(),
        file: "foo.rs".into(),
        location: crate::notes::NoteLocation::Range {
            start: DiffCoord::from_row(&ctx2).unwrap(),
            end: DiffCoord::from_row(&ctx4).unwrap(),
        },
        text: "check function".into(),
        original_source_rows: vec![ctx2.clone(), ctx3.clone(), ctx4.clone()],
        content_hash: LineHash::of_rows(&[ctx2.clone(), ctx3.clone(), ctx4.clone()]),
        ..Default::default()
    };

    assert!(note_anchors_in_context_lines(&note_multi, &file_lines));

    // Modified one line in the middle -> gate fails
    let file_lines_modified = vec![
        "// header".to_string(),
        "fn foo() {".to_string(),
        "    let x = 43;".to_string(),
        "}".to_string(),
        "// footer".to_string(),
    ];
    assert!(!note_anchors_in_context_lines(
        &note_multi,
        &file_lines_modified
    ));

    // Out of bounds -> gate fails
    let short_file = vec!["// header".to_string(), "fn foo() {".to_string()];
    assert!(!note_anchors_in_context_lines(&note_multi, &short_file));

    // A stored row whose text contradicts its own coordinate -> gate fails
    let note_wrong_text = ReviewNote {
        original_source_rows: vec![row(
            crate::parse::RowKind::Ctx,
            Some(3),
            Some(3),
            "    let x = 43;",
        )],
        ..note_multi.clone()
    };
    assert!(!note_anchors_in_context_lines(
        &note_wrong_text,
        &file_lines
    ));

    // Out of bounds -> gate fails
    let short_file = vec!["// header".to_string(), "fn foo() {".to_string()];
    assert!(!note_anchors_in_context_lines(&note_multi, &short_file));

    // Rows without a primary-side number cannot be verified -> gate fails
    let note_unverifiable = ReviewNote {
        original_source_rows: vec![row(crate::parse::RowKind::Ctx, None, None, "fn foo() {")],
        ..note_multi.clone()
    };
    assert!(!note_anchors_in_context_lines(
        &note_unverifiable,
        &file_lines
    ));

    // Note without stored rows cannot anchor
    let note_without_rows = ReviewNote {
        id: "n_no_rows".into(),
        file: "foo.rs".into(),
        location: NoteLocation::range(DiffCoord::new_side(2), DiffCoord::new_side(4)),
        text: "note without rows".into(),
        content_hash: None,
        ..Default::default()
    };
    assert!(!note_anchors_in_context_lines(
        &note_without_rows,
        &file_lines
    ));

    // Coordinates still gate the lookup: duplicated chunks elsewhere in the
    // file do not re-anchor the note at a different line.
    let file_with_duplicate = vec![
        "// header".to_string(),
        "fn foo() {".to_string(),
        "    let x = 42;".to_string(),
        "}".to_string(),
        "// duplicate below".to_string(),
        "fn foo() {".to_string(),
        "    let x = 42;".to_string(),
        "}".to_string(),
    ];
    let shifted = ReviewNote {
        location: crate::notes::NoteLocation::Range {
            start: DiffCoord::from_row(&ctx2).unwrap(),
            end: DiffCoord {
                old_line_num: NonZeroU32::new(7),
                new_line_num: NonZeroU32::new(7),
            },
        },
        ..note_multi.clone()
    };
    assert!(
        !note_anchors_in_context_lines(&shifted, &file_with_duplicate),
        "the gate checks the exact recorded span, not a content search"
    );
    assert!(note_anchors_in_context_lines(
        &note_multi,
        &file_with_duplicate
    ));

    // 2. Output-row anchoring: exact both-side coordinate match first, then a
    //    complete structural hash comparison over the located window.

    // A mod row pairs old and new content.
    let mod_row = crate::wire::OutputRow {
        kind: crate::parse::RowKind::Mod,
        mov_side: None,
        old_line: Some(12),
        new_line: Some(15),
        runs: vec![
            run(crate::merge::RunKind::Ctx, "fn foo("),
            run(crate::merge::RunKind::Del, "old"),
            run(crate::merge::RunKind::Add, "new"),
            run(crate::merge::RunKind::Ctx, ") {}"),
        ],
        wholesale: false,
        group: None,
    };
    let old_only_row = row(crate::parse::RowKind::Del, Some(3), None, "old debt");
    let rows = vec![old_only_row.clone(), mod_row.clone()];

    // Note anchored to the mod row's full coordinate: both sides must match.
    let note_mod = ReviewNote {
        id: "n_mod".into(),
        file: "foo.rs".into(),
        location: crate::notes::NoteLocation::Line {
            coord: DiffCoord::from_row(&mod_row).unwrap(),
        },
        text: "mod note".into(),
        original_source_rows: vec![mod_row.clone()],
        content_hash: LineHash::of_rows(std::slice::from_ref(&mod_row)),
        ..Default::default()
    };
    assert!(
        note_anchors_in_rows(&note_mod, &rows),
        "mod row anchors via exact (old, new) pair"
    );

    // Same new line number but different old side -> no anchor.
    let mismatched_side_row = crate::wire::OutputRow {
        kind: crate::parse::RowKind::Ctx,
        old_line: Some(9),
        ..mod_row.clone()
    };
    assert!(
        !note_anchors_in_rows(&note_mod, &[mismatched_side_row]),
        "coordinates must match on both sides"
    );

    // Same coordinates but recolored presentation -> the complete hash
    // detects the difference.
    let mut recolored = mod_row.clone();
    recolored.runs[1].color = diffuse_syntax::color::TokenColor::Keyword;
    assert!(!note_anchors_in_rows(&note_mod, &[recolored]));

    // Old-side note anchors on single-side rows by exact pair.
    let note_old = ReviewNote {
        id: "n_old".into(),
        file: "foo.rs".into(),
        location: crate::notes::NoteLocation::Line {
            coord: DiffCoord::from_row(&old_only_row).unwrap(),
        },
        text: "old note".into(),
        original_source_rows: vec![old_only_row.clone()],
        content_hash: LineHash::of_rows(std::slice::from_ref(&old_only_row)),
        ..Default::default()
    };
    assert!(note_anchors_in_rows(&note_old, &rows));

    // Cross-side range: start row then scan forward until the end row.
    let second_del = row(crate::parse::RowKind::Del, Some(4), None, "old debt two");
    let range_rows = vec![old_only_row.clone(), second_del.clone()];
    let range_note = ReviewNote {
        id: "n_range".into(),
        file: "foo.rs".into(),
        location: crate::notes::NoteLocation::Range {
            start: DiffCoord::from_row(&old_only_row).unwrap(),
            end: DiffCoord::from_row(&second_del).unwrap(),
        },
        text: "range note".into(),
        original_source_rows: range_rows.clone(),
        content_hash: LineHash::of_rows(&range_rows),
        ..Default::default()
    };
    assert!(
        note_anchors_in_rows(&range_note, &range_rows),
        "range anchors when the window hash matches"
    );

    // A note whose hash covers only the start row does not anchor over the
    // full window.
    let start_only_hash = ReviewNote {
        content_hash: LineHash::of_rows(std::slice::from_ref(&old_only_row)),
        ..range_note.clone()
    };
    assert!(!note_anchors_in_rows(&start_only_hash, &range_rows));

    // End coord not present after start -> unanchored.
    let missing_end = ReviewNote {
        location: crate::notes::NoteLocation::Range {
            start: DiffCoord::from_row(&old_only_row).unwrap(),
            end: DiffCoord {
                old_line_num: NonZeroU32::new(999),
                new_line_num: None,
            },
        },
        ..range_note.clone()
    };
    assert!(!note_anchors_in_rows(&missing_end, &range_rows));

    // Hashing a window of row references equals hashing the owned rows.
    assert_eq!(
        LineHash::of_hashable(&rows.iter().collect::<Vec<_>>()),
        LineHash::of_rows(&rows).unwrap(),
        "reference and owned windows hash identically"
    );
}

#[test]
fn test_row_hash_is_complete_and_unambiguous() {
    use crate::{
        merge::RunKind,
        parse::RowKind,
        wire::{MovSide, OutputRun},
    };

    let run = |kind: RunKind, sgr: Option<u32>, text: &str| OutputRun {
        kind,
        color: TokenColor::Default,
        sgr,
        text: text.to_owned().into(),
    };
    let base = crate::wire::OutputRow {
        kind: RowKind::Mod,
        mov_side: None,
        old_line: Some(12),
        new_line: Some(15),
        runs: vec![
            run(RunKind::Ctx, None, "fn foo("),
            run(RunKind::Add, None, "new"),
        ],
        wholesale: false,
        group: None,
    };
    let h = |rows: &[crate::wire::OutputRow]| LineHash::of_rows(rows).unwrap();
    let single = std::slice::from_ref(&base).to_vec();
    let h0 = h(&single);

    // Field-for-field completeness: mutating any field changes the hash.
    let mut m = base.clone();
    m.kind = RowKind::Ctx;
    assert_ne!(h(std::slice::from_ref(&m)), h0, "kind");
    let mut m = base.clone();
    m.mov_side = Some(MovSide::Src);
    assert_ne!(h(std::slice::from_ref(&m)), h0, "mov_side");
    let mut m = base.clone();
    m.old_line = Some(13);
    assert_ne!(h(std::slice::from_ref(&m)), h0, "old_line");
    let mut m = base.clone();
    m.new_line = Some(16);
    assert_ne!(h(std::slice::from_ref(&m)), h0, "new_line");
    let mut m = base.clone();
    m.wholesale = true;
    assert_ne!(h(std::slice::from_ref(&m)), h0, "wholesale");
    let mut m = base.clone();
    m.group = Some(crate::parse::RowGroup::Peer {
        id: crate::parse::GroupId(9),
        slot: 0,
        n: 2,
        m: 2,
    });
    assert_ne!(h(std::slice::from_ref(&m)), h0, "group");
    let mut m = base.clone();
    m.runs[1].kind = RunKind::Ctx;
    assert_ne!(h(std::slice::from_ref(&m)), h0, "run kind");
    let mut m = base.clone();
    m.runs[0].sgr = Some(1);
    assert_ne!(h(std::slice::from_ref(&m)), h0, "run sgr");
    let mut m = base.clone();
    m.runs[1].text = "older".into();
    assert_ne!(h(std::slice::from_ref(&m)), h0, "run text");

    // Run-boundary ambiguity: distinct run splits of identical concatenated
    // text never collide.
    let split = crate::wire::OutputRow {
        runs: vec![
            run(RunKind::Ctx, None, "fn foo(n"),
            run(RunKind::Add, None, "ew"),
        ],
        ..base.clone()
    };
    assert_ne!(h(std::slice::from_ref(&split)), h0);

    // Run count participates.
    let merged = crate::wire::OutputRow {
        runs: vec![run(RunKind::Ctx, None, "fn foo(new")],
        ..base.clone()
    };
    assert_ne!(h(std::slice::from_ref(&merged)), h0);

    // Row count participates.
    let two = vec![base.clone(), base.clone()];
    assert_ne!(h(&two), h0);
}

#[test]
fn test_diff_coord_invariants() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // Both sides captured when available (mod/ctx rows carry both).
    let coord = DiffCoord::from_numbers(Some(3), Some(12)).unwrap();
    assert_eq!(coord.old_line_num.map(|n| n.get()), Some(3));
    assert_eq!(coord.new_line_num.map(|n| n.get()), Some(12));

    // Single-side coordinates remain single-side.
    let old_only = DiffCoord::from_numbers(Some(7), None).unwrap();
    assert!(old_only.new_line_num.is_none());
    assert!(old_only.old_line_num.is_some());
    let new_only = DiffCoord::from_numbers(None, Some(15)).unwrap();
    assert!(new_only.old_line_num.is_none());
    assert_eq!(new_only.new_line_num.map(|n| n.get()), Some(15));

    // Zero line numbers are 0-indexed sentinels and get dropped.
    let zeroed = DiffCoord::from_numbers(Some(0), Some(5)).unwrap();
    assert!(zeroed.old_line_num.is_none());
    assert_eq!(zeroed.new_line_num.map(|n| n.get()), Some(5));

    // Not-both-None invariant: by construction returns None...
    assert!(DiffCoord::from_numbers(None, None).is_none());
    // ...and `new` asserts under the DbC precondition.
    let panics = catch_unwind(AssertUnwindSafe(|| DiffCoord::new(None, None))).is_err();
    assert!(
        panics,
        "DiffCoord::new(None, None) must violate the precondition"
    );

    // `from_row` captures both sides of a mod/ctx row.
    let row = crate::wire::OutputRow {
        kind: crate::parse::RowKind::Ctx,
        mov_side: None,
        old_line: Some(4),
        new_line: Some(18),
        runs: vec![crate::wire::OutputRun {
            kind: crate::merge::RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "line".into(),
        }],
        wholesale: false,
        group: None,
    };
    let coord = DiffCoord::from_row(&row).unwrap();
    assert_eq!(coord.old_line_num.map(|n| n.get()), Some(4));
    assert_eq!(coord.new_line_num.map(|n| n.get()), Some(18));
}

#[test]
fn test_note_serialization_with_content_hash() {
    let hash = LineHash::from_hex("12345678").unwrap();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(10)),
        text: "my note".into(),
        created_at: 1000,
        content_hash: Some(hash),
        ..Default::default()
    };
    let json = serde_json::to_string(&note).unwrap();
    assert!(json.contains("\"contentHash\":\"12345678\""));
    assert!(
        json.contains("\"location\":{\"kind\":\"line\",\"coord\":{\"newLineNum\":10}}"),
        "wire shape: {json}"
    );

    // Deserialize note with contentHash
    let deserialized: ReviewNote = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.content_hash, Some(hash));

    // Deserialize note without contentHash
    let plain_json = r#"{"id":"n2","file":"lib.rs","location":{"kind":"line","coord":{"newLineNum":5}},"text":"hi","createdAt":500}"#;
    let plain_note: ReviewNote = serde_json::from_str(plain_json).unwrap();
    assert_eq!(plain_note.content_hash, None);
}

#[test]
fn test_query_notes_for_file_scoping_and_standalone() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(10)),
        text: "note on scoped commit".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/other.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(20)),
        text: "note on other file".into(),
        created_at: 1001,
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo", "commit123", note1.clone());
    add_note(&mut db, "standalone_repo", "", note2.clone());

    // 1. Exact match
    let exact = query_notes_for_file(&db, Some("myrepo"), Some("commit123"), "src/lib.rs");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].id, "n1");

    // 2. Prefix match on revision
    let prefix_match = query_notes_for_file(&db, Some("myrepo"), Some("commit12"), "src/lib.rs");
    assert_eq!(prefix_match.len(), 1);
    assert_eq!(prefix_match[0].id, "n1");

    // 3. Querying with rev = None queries across all revisions for that repo
    let all_revs = query_notes_for_file(&db, Some("myrepo"), None, "src/lib.rs");
    assert_eq!(all_revs.len(), 1);
    assert_eq!(all_revs[0].id, "n1");

    let standalone = query_notes_for_file(&db, Some("standalone_repo"), None, "src/other.rs");
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, "n2");

    // 4. Missing repo returns empty vec (fail-fast: notes disabled outside repo)
    let missing_repo = query_notes_for_file(&db, None, None, "src/lib.rs");
    assert!(missing_repo.is_empty());

    // 5. Querying scoped commit does not leak notes from other commits
    let mut db_multi = NotesDB::new();
    add_note(&mut db_multi, "myrepo", "commit123", note1.clone());
    add_note(&mut db_multi, "myrepo", "commit456", note2.clone());
    let leaked = query_notes_for_file(&db_multi, Some("myrepo"), Some("commit123"), "src/other.rs");
    assert!(
        leaked.is_empty(),
        "Notes from commit456 must not leak when querying commit123"
    );
}

#[test]
fn test_query_notes_for_file_multi_rev_order_is_deterministic() {
    let mk = |id: &str| ReviewNote {
        id: id.into(),
        file: "src/lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(1)),
        text: id.into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    let mut db = NotesDB::new();
    add_note(&mut db, "myrepo", "c-rev", mk("from-c"));
    add_note(&mut db, "myrepo", "a-rev", mk("from-a"));
    add_note(&mut db, "myrepo", "b-rev", mk("from-b"));

    // rev_key=None spans all revisions: must come back in sorted rev order.
    let all = query_notes_for_file(&db, Some("myrepo"), None, "src/lib.rs");
    let ids: Vec<&str> = all.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["from-a", "from-b", "from-c"]);
}

#[test]
fn test_note_files_for_repo() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(10)),
        text: "note on lib".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/untouched.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(20)),
        text: "note on untouched".into(),
        created_at: 1001,
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo", "commit123", note1);
    add_note(&mut db, "myrepo", "commit123", note2);

    // Exact rev match
    let files = note_files_for_repo(&db, Some("myrepo"), Some("commit123"));
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // Prefix rev match
    let files = note_files_for_repo(&db, Some("myrepo"), Some("commit12"));
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // Query across all revisions
    let files = note_files_for_repo(&db, Some("myrepo"), None);
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // None repo returns empty
    let files = note_files_for_repo(&db, None, None);
    assert!(files.is_empty());

    // Non-existent repo returns empty
    let empty_db = NotesDB::new();
    let files = note_files_for_repo(&empty_db, Some("other"), None);
    assert!(files.is_empty());
}

#[test]
fn test_note_status_lifecycle_and_serialization() {
    assert_eq!(NoteStatus::default(), NoteStatus::Open);
    assert!(NoteStatus::Open.is_open());
    assert!(!NoteStatus::Open.is_in_progress());
    assert!(!NoteStatus::Open.is_resolved());
    assert!(NoteStatus::Open.counts_as_open());
    assert_eq!(NoteStatus::Open.glyph(), "💬");
    assert_eq!(NoteStatus::Open.cycle(), NoteStatus::InProgress);

    assert!(!NoteStatus::InProgress.is_open());
    assert!(NoteStatus::InProgress.is_in_progress());
    assert!(!NoteStatus::InProgress.is_resolved());
    assert!(NoteStatus::InProgress.counts_as_open());
    assert_eq!(NoteStatus::InProgress.glyph(), "🔄");
    assert_eq!(NoteStatus::InProgress.cycle(), NoteStatus::Resolved);

    assert!(!NoteStatus::Resolved.is_open());
    assert!(!NoteStatus::Resolved.is_in_progress());
    assert!(NoteStatus::Resolved.is_resolved());
    assert!(!NoteStatus::Resolved.counts_as_open());
    assert_eq!(NoteStatus::Resolved.glyph(), "✅");
    assert_eq!(NoteStatus::Resolved.cycle(), NoteStatus::Open);

    // Serialization & backward compatibility
    let note_open = ReviewNote {
        id: "1".into(),
        file: "a.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(1)),
        text: "hello".into(),
        created_at: 100,
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    };
    let json_open = serde_json::to_string(&note_open).unwrap();
    assert!(!json_open.contains("\"status\""));
    assert!(!json_open.contains("\"contentHash\""));

    let note_in_progress = ReviewNote {
        status: NoteStatus::InProgress,
        content_hash: Some(LineHash::from_hex("deadbeef").unwrap()),
        ..note_open.clone()
    };
    let json_ip = serde_json::to_string(&note_in_progress).unwrap();
    assert!(json_ip.contains("\"status\":\"inProgress\""));
    assert!(json_ip.contains("\"contentHash\":"));

    let note_resolved = ReviewNote {
        status: NoteStatus::Resolved,
        ..note_open.clone()
    };
    let json_res = serde_json::to_string(&note_resolved).unwrap();
    assert!(json_res.contains("\"status\":\"resolved\""));

    // Deserialization without status defaults properly
    let raw = r#"{"id":"2","file":"b.rs","location":{"kind":"line","coord":{"newLineNum":5}},"text":"plain","createdAt":200}"#;
    let parsed: ReviewNote = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed.status, NoteStatus::Open);
    assert_eq!(parsed.content_hash, None);
}

#[test]
fn test_reply_to_note_text() {
    let original = "Single line comment";
    let reply = "Done in commit 123";
    let formatted = reply_to_note_text(original, reply);
    assert_eq!(formatted, "Done in commit 123\n\n> Single line comment");

    let multiline = "Line 1\nLine 2\nLine 3";
    let reply2 = "Fixed";
    let formatted2 = reply_to_note_text(multiline, reply2);
    assert_eq!(formatted2, "Fixed\n\n> Line 1\n> Line 2\n> Line 3");
}

#[test]
fn test_notes_db_status_and_reply_operations() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(10)),
        text: "initial comment".into(),
        created_at: 1000,
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    };
    add_note(&mut db, "repo1", "c1", note);

    // set_note_status_by_id
    assert!(set_note_status_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "n1",
        NoteStatus::InProgress
    ));
    let n = notes_for_file(&db, "repo1", "c1", "src/lib.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::InProgress);

    // resolve_note_by_id
    assert!(resolve_note_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "n1"
    ));
    let n = notes_for_file(&db, "repo1", "c1", "src/lib.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);

    // reopen_note_by_id
    assert!(reopen_note_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "n1"
    ));
    let n = notes_for_file(&db, "repo1", "c1", "src/lib.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::Open);

    // reply_to_note_by_id with status update
    assert!(reply_to_note_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "n1",
        "Addressed!",
        Some(NoteStatus::Resolved)
    ));
    let n = notes_for_file(&db, "repo1", "c1", "src/lib.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);
    assert_eq!(n.text, "Addressed!\n\n> initial comment");

    // Missing notes return false
    assert!(!set_note_status_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "missing",
        NoteStatus::Open
    ));
    assert!(!set_note_status_by_id(
        &mut db,
        "missing_repo",
        "c1",
        "src/lib.rs",
        "n1",
        NoteStatus::Open
    ));
    assert!(!reply_to_note_by_id(
        &mut db,
        "repo1",
        "c1",
        "src/lib.rs",
        "missing",
        "hi",
        None
    ));
}

#[test]
fn test_find_and_update_note_in_repo() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "abc-123".into(),
        file: "src/main.rs".into(),
        location: NoteLocation::line(DiffCoord::new_side(10)),
        text: "first note".into(),
        created_at: 100,
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    };
    let note2 = ReviewNote {
        id: "def-456".into(),
        file: "src/util.rs".into(),
        location: NoteLocation::line(DiffCoord::old_side(20)),
        text: "second note".into(),
        created_at: 200,
        content_hash: None,
        status: NoteStatus::InProgress,
        original_source_rows: Vec::new(),
    };
    add_note(&mut db, "myrepo", "commit1", note1);
    add_note(&mut db, "other", "commit2", note2);

    // Exact match by id with repo
    let (rev, file, n) = find_note_in_repo(&db, "myrepo", "abc-123").expect("found");
    assert_eq!(rev, "commit1");
    assert_eq!(file, "src/main.rs");
    assert_eq!(n.id, "abc-123");

    // Prefix match
    let (rev, file, n) = find_note_in_repo(&db, "myrepo", "abc").expect("found prefix");
    assert_eq!(rev, "commit1");
    assert_eq!(file, "src/main.rs");
    assert_eq!(n.id, "abc-123");

    // Note from other repo is NOT found in myrepo (fail-fast isolation!)
    assert!(find_note_in_repo(&db, "myrepo", "def").is_none());

    // Update note in repo
    let updated = update_note_in_repo(&mut db, "myrepo", "abc", |n| {
        n.status = NoteStatus::Resolved;
    });
    assert!(updated);
    let (_, _, n) = find_note_in_repo(&db, "myrepo", "abc-123").unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);

    // Delete note in repo
    assert!(delete_note_in_repo(&mut db, "myrepo", "abc"));
    assert!(find_note_in_repo(&db, "myrepo", "abc-123").is_none());
}

#[test]
fn test_generate_note_id_is_unique_and_non_empty() {
    let id1 = generate_note_id();
    let id2 = generate_note_id();
    assert!(!id1.is_empty());
    assert!(!id2.is_empty());
    assert_ne!(id1, id2);
    assert_eq!(id1.split('-').count(), 3);
}

#[test]
fn test_coord_and_location_label_formatting() {
    // 1. Single coordinate with new line: shows new line without labelling it
    let new_only = DiffCoord::new_side(42);
    assert_eq!(new_only.label(), "42");
    assert_eq!(NoteLocation::line(new_only).label(), "42");

    // 2. Single coordinate with both old and new (same): shows line number without
    //    labelling it
    let both_same = DiffCoord::both(15, 15);
    assert_eq!(both_same.label(), "15");
    assert_eq!(NoteLocation::line(both_same).label(), "15");

    // 3. Single coordinate with both old and new (different, e.g. on mod row):
    // only shows "new" coord without labelling it
    let both_diff = DiffCoord::both(30, 32);
    assert_eq!(both_diff.label(), "32");
    assert_eq!(NoteLocation::line(both_diff).label(), "32");

    // 4. Single coordinate with old line only (e.g. deletion row): shows "old"
    //    coord
    let old_only = DiffCoord::old_side(10);
    assert_eq!(old_only.label(), "old 10");
    assert_eq!(NoteLocation::line(old_only).label(), "old 10");

    // 5. Range over modification rows: old 1615/new 1617 to old 1616/new 1618 ->
    //    simply "1617-1618"
    let mod_start = DiffCoord::both(1615, 1617);
    let mod_end = DiffCoord::both(1616, 1618);
    let range_mod = NoteLocation::range(mod_start, mod_end);
    assert_eq!(range_mod.label(), "1617-1618");

    // 6. Range from modification row to deletion row: old 8/new 12 to old 10 ->
    //    "12-old 10"
    let mixed_start = DiffCoord::both(8, 12);
    let mixed_end = DiffCoord::old_side(10);
    let range_mixed = NoteLocation::range(mixed_start, mixed_end);
    assert_eq!(range_mixed.label(), "12-old 10");

    // 7. Range over deletion rows only: old 5 to old 9 -> "old 5-old 9"
    let del_start = DiffCoord::old_side(5);
    let del_end = DiffCoord::old_side(9);
    let range_del = NoteLocation::range(del_start, del_end);
    assert_eq!(range_del.label(), "old 5-old 9");

    // 8. Range from deletion row to addition row: old 5 to new 12 -> "old 5-12"
    let del_to_add = NoteLocation::range(DiffCoord::old_side(5), DiffCoord::new_side(12));
    assert_eq!(del_to_add.label(), "old 5-12");

    // 9. File-level note -> "file"
    assert_eq!(NoteLocation::File.label(), "file");

    // 10. Empty coordinate (both None) -> empty string
    let empty_coord = DiffCoord::from_numbers(None, None);
    assert_eq!(empty_coord, None);
}
