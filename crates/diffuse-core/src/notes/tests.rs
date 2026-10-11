//! Unit tests for note persistence, markdown formatting, and querying.

use std::{ffi::OsString, fs, mem, path::PathBuf, process::Command};

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
fn test_notes_add_and_query() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 10,
        end: 15,
        text: "Check `this` out".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", note.clone());

    let repo_notes: Vec<_> = notes_for_file(&db, "myrepo", "src/main.rs").collect();
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
        side: Side::New,
        start: 10,
        end: 15,
        text: "First".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 20,
        end: 25,
        text: "Second".into(),
        created_at: 1001,
        line_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", note1);
    add_note(&mut db, "myrepo", note2);

    let removed = remove_notes(&mut db, "myrepo", "src/main.rs", Side::New, 12, 18);
    assert_eq!(removed, 1);

    let remaining: Vec<_> = notes_for_file(&db, "myrepo", "src/main.rs").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
}

#[test]
fn test_remove_note_by_id_targeted() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 10,
        end: 15,
        text: "First".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 10,
        end: 15,
        text: "Second (same line)".into(),
        created_at: 1001,
        line_hash: None,
        ..Default::default()
    };

    add_note(&mut db, "myrepo", note1);
    add_note(&mut db, "myrepo", note2);

    // Target specifically note1
    assert!(remove_note_by_id(&mut db, "myrepo", "src/main.rs", "n1"));
    let remaining: Vec<_> = notes_for_file(&db, "myrepo", "src/main.rs").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
    assert_eq!(remaining[0].text, "Second (same line)");

    // Non-existent ID or repo returns false
    assert!(!remove_note_by_id(
        &mut db,
        "myrepo",
        "src/main.rs",
        "nonexistent"
    ));
    assert!(!remove_note_by_id(
        &mut db,
        "unknown_repo",
        "src/main.rs",
        "n2"
    ));
    assert!(!remove_note_by_id(
        &mut db,
        "myrepo",
        "unknown_file.rs",
        "n2"
    ));

    // Removing last note cleans up file entry in DB
    assert!(remove_note_by_id(&mut db, "myrepo", "src/main.rs", "n2"));
    assert_eq!(notes_for_file(&db, "myrepo", "src/main.rs").count(), 0);
    assert!(!db.get("myrepo").unwrap().contains_key("src/main.rs"));
}

#[test]
fn test_update_note_text_targeted() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 10,
        end: 15,
        text: "Initial text".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo", note);

    assert!(update_note_text(
        &mut db,
        "myrepo",
        "src/main.rs",
        "n1",
        "Updated text".into()
    ));

    let updated: Vec<_> = notes_for_file(&db, "myrepo", "src/main.rs").collect();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "n1");
    assert_eq!(updated[0].text, "Updated text");
    assert_eq!(updated[0].start, 10);
    assert_eq!(updated[0].end, 15);
    assert_eq!(updated[0].created_at, 1000);

    // Non-existent ID, file, or repo returns false
    assert!(!update_note_text(
        &mut db,
        "myrepo",
        "src/main.rs",
        "nonexistent",
        "New text".into()
    ));
    assert!(!update_note_text(
        &mut db,
        "unknown_repo",
        "src/main.rs",
        "n1",
        "New text".into()
    ));
    assert!(!update_note_text(
        &mut db,
        "myrepo",
        "other.rs",
        "n1",
        "New text".into()
    ));
}

#[test]
fn test_notes_key_scopes_repo_by_head() {
    assert_eq!(notes_key("myrepo", Some("abc123")), "myrepo@abc123");
    assert_eq!(notes_key("myrepo", None), "myrepo");
    assert_eq!(notes_key("myrepo", Some("")), "myrepo");
}

#[test]
fn test_notes_isolated_across_heads() {
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "lib.rs".into(),
        side: Side::New,
        start: 1,
        end: 1,
        text: "head-scoped".into(),
        created_at: 1,
        line_hash: None,
        ..Default::default()
    };
    let key = notes_key("myrepo", Some("h1"));
    add_note(&mut db, &key, note);

    assert_eq!(notes_for_file(&db, &key, "lib.rs").count(), 1);
    // Bare repo name and a different head must not see the note.
    assert_eq!(notes_for_file(&db, "myrepo", "lib.rs").count(), 0);
    assert_eq!(
        notes_for_file(&db, &notes_key("myrepo", Some("h2")), "lib.rs").count(),
        0
    );
}

#[test]
fn test_head_commit_resolves_temp_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // Not a repo yet: no HEAD.
    assert_eq!(head_commit(dir), None);

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    // No commits yet: no HEAD.
    assert_eq!(head_commit(dir), None);

    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);

    let head = head_commit(dir).expect("HEAD must resolve after a commit");
    assert_eq!(head.len(), 40);
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()));

    // The convenience wrapper keys off the live HEAD.
    assert_eq!(notes_key_for_repo(dir, "myrepo"), format!("myrepo@{head}"));
    assert_eq!(
        notes_key_for_repo(Path::new("/nonexistent-diffuse-test-dir"), "myrepo"),
        "myrepo"
    );
}

#[test]
fn test_notes_commit_for_ranges() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);

    fs::write(dir.join("a.rs"), "1\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);
    let c1 = head_commit(dir).unwrap();

    fs::write(dir.join("a.rs"), "2\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit2"]);
    let c2 = head_commit(dir).unwrap();

    fs::write(dir.join("a.rs"), "3\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit3"]);
    let c3 = head_commit(dir).unwrap();

    fs::write(dir.join("a.rs"), "4\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit4"]);
    let c4 = head_commit(dir).unwrap();

    // 1. Working tree / no range -> keys off HEAD (c4)
    assert_eq!(notes_commit(dir, &[]), Some(c4.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &[]),
        format!("myrepo@{c4}")
    );

    // 2. Single-commit range c1..c2 -> next newest in range is c1 (commit before
    //    c2)
    let r_1_2 = vec![format!("{c1}..{c2}")];
    assert_eq!(notes_commit(dir, &r_1_2), Some(c1.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &r_1_2),
        format!("myrepo@{c1}")
    );

    // 3. Multi-commit range c1..c4 (commits: c2, c3, c4) -> second to last commit
    //    in range (next newest) is c3
    let r_1_4 = vec![format!("{c1}..{c4}")];
    assert_eq!(notes_commit(dir, &r_1_4), Some(c3.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &r_1_4),
        format!("myrepo@{c3}")
    );

    // 4. Single-commit notation c3^! -> commit before c3 is c2
    let r_c3_head = vec![format!("{c3}^!")];
    assert_eq!(notes_commit(dir, &r_c3_head), Some(c2.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &r_c3_head),
        format!("myrepo@{c2}")
    );

    // 5. Two ref arguments [c1, c3] (commits: c2, c3) -> next newest is c2
    let r_two_refs = vec![c1.clone(), c3.clone()];
    assert_eq!(notes_commit(dir, &r_two_refs), Some(c2.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &r_two_refs),
        format!("myrepo@{c2}")
    );
}

#[test]
fn test_notes_persistence() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let file = tmp.path();

    let mut db = NotesDB::new();
    add_note(
        &mut db,
        "repo1",
        ReviewNote {
            id: "n1".into(),
            file: "lib.rs".into(),
            side: Side::Old,
            start: 1,
            end: 1,
            text: "**important**".into(),
            created_at: 500,
            line_hash: None,
            ..Default::default()
        },
    );

    save_notes(file, &db).unwrap();

    let loaded = load_notes(file);
    assert_eq!(loaded.len(), 1);
    let loaded_notes: Vec<_> = notes_for_file(&loaded, "repo1", "lib.rs").collect();
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
fn test_hash_line_content() {
    // 1. Same content, different indentation -> identical hash
    let h1 = hash_line_content("    let x = 42;");
    let h2 = hash_line_content("let x = 42;");
    let h3 = hash_line_content("\t\tlet x = 42;\n");
    assert_eq!(h1, h2);
    assert_eq!(h1, h3);

    // 2. Different content -> different hash
    let h4 = hash_line_content("let x = 43;");
    assert_ne!(h1, h4);

    // 3. Exact 8 hex chars (short hash)
    let s = h1.to_hex();
    assert_eq!(s.len(), 8);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(format!("{h1}"), s);
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
    let hash = LineHash::from_content("fn compute_sum(a: i32, b: i32) -> i32 {");
    assert!(hash.matches_content("fn compute_sum(a: i32, b: i32) -> i32 {"));
    assert!(hash.matches_content("    fn compute_sum(a: i32, b: i32) -> i32 {\n"));
    assert!(!hash.matches_content("fn compute_sum(a: i64, b: i64) -> i64 {"));

    // Parsing from hex
    let hex_str = hash.to_hex();
    let parsed = LineHash::from_hex(&hex_str).unwrap();
    assert_eq!(hash, parsed);
    assert_eq!(hash.get(), parsed.get());

    // Non-zero guarantee: zero wraps to 1
    let zero_hash = LineHash::from_hex("00000000").unwrap();
    assert_eq!(zero_hash.get(), 1);
}

#[test]
fn test_note_anchors_cleanly_predicate() {
    let hash = hash_line_content("fn foo() {}");
    let note_with_hash = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "my note".into(),
        created_at: 1000,
        line_hash: Some(hash),
        ..Default::default()
    };

    // Clean match on line content
    assert!(note_anchors_cleanly(&note_with_hash, "fn foo() {}"));
    assert!(note_anchors_cleanly(&note_with_hash, "    fn foo() {}\n"));

    // Mismatched content on that line -> dropped
    assert!(!note_anchors_cleanly(&note_with_hash, "fn bar() {}"));

    // Legacy note without line_hash -> anchors cleanly by line number
    let legacy_note = ReviewNote {
        id: "n2".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "legacy note".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    assert!(note_anchors_cleanly(&legacy_note, "any changed content"));

    // Note with source_lines (no line_hash) -> matches source lines
    let note_with_source = ReviewNote {
        id: "n3".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "source note".into(),
        line_hash: None,
        source_lines: Some("fn foo() {}".into()),
        ..Default::default()
    };
    assert!(note_anchors_cleanly(&note_with_source, "fn foo() {}"));
    assert!(!note_anchors_cleanly(&note_with_source, "fn bar() {}"));
}

#[test]
fn test_compound_line_hash_and_note_anchors_in_file() {
    use crate::notes::note_anchors_in_file;

    // 1. Single-line equivalence
    let single = LineHash::from_lines(["fn foo() {}"]);
    assert_eq!(single, LineHash::from_content("fn foo() {}"));

    // 2. Multi-line compound hash
    let multi1 = LineHash::from_lines(["fn foo() {", "    let x = 42;", "}"]);
    let multi2 = LineHash::from_lines(["  fn foo() {\n", "let x = 42;", "  }\t"]);
    assert_eq!(multi1, multi2);

    let multi_diff = LineHash::from_lines(["fn foo() {", "    let x = 43;", "}"]);
    assert_ne!(multi1, multi_diff);

    // 3. note_anchors_in_file with multi-line note
    let file_lines = vec![
        "// header".to_string(),
        "fn foo() {".to_string(),
        "    let x = 42;".to_string(),
        "}".to_string(),
        "// footer".to_string(),
    ];

    let note_multi = ReviewNote {
        id: "n_multi".into(),
        file: "foo.rs".into(),
        side: Side::New,
        start: 2,
        end: 4,
        text: "check function".into(),
        line_hash: Some(multi1),
        ..Default::default()
    };

    assert!(note_anchors_in_file(&note_multi, &file_lines));

    // Modified one line in the middle -> must fail to anchor
    let file_lines_modified = vec![
        "// header".to_string(),
        "fn foo() {".to_string(),
        "    let x = 43;".to_string(),
        "}".to_string(),
        "// footer".to_string(),
    ];
    assert!(!note_anchors_in_file(&note_multi, &file_lines_modified));

    // Out of bounds -> must fail to anchor
    let short_file = vec!["// header".to_string(), "fn foo() {".to_string()];
    assert!(!note_anchors_in_file(&note_multi, &short_file));

    // Note without hash or source_lines cannot reanchor
    let legacy_in_bounds = ReviewNote {
        id: "n_leg".into(),
        file: "foo.rs".into(),
        side: Side::New,
        start: 2,
        end: 4,
        text: "legacy note".into(),
        line_hash: None,
        ..Default::default()
    };
    assert!(!note_anchors_in_file(&legacy_in_bounds, &file_lines));
    assert!(!note_anchors_in_file(&legacy_in_bounds, &short_file));

    // Note with source_lines (no line_hash) -> succeeds when matching, fails when
    // modified
    let note_source = ReviewNote {
        id: "n_src".into(),
        file: "foo.rs".into(),
        side: Side::New,
        start: 2,
        end: 4,
        text: "source lines note".into(),
        line_hash: None,
        source_lines: Some("fn foo() {\n    let x = 42;\n}".into()),
        ..Default::default()
    };
    assert!(note_anchors_in_file(&note_source, &file_lines));
    assert!(!note_anchors_in_file(&note_source, &file_lines_modified));

    // 4. Uniqueness check across the file:
    // If the selected chunk appears more than once anywhere in the file,
    // reanchoring fails.
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
    assert!(
        !note_anchors_in_file(&note_multi, &file_with_duplicate),
        "chunk is not unique across the file, must fail to anchor"
    );
}

#[test]
fn test_note_serialization_with_line_hash() {
    let hash = LineHash::from_hex("12345678").unwrap();
    let note = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "my note".into(),
        created_at: 1000,
        line_hash: Some(hash),
        ..Default::default()
    };
    let json = serde_json::to_string(&note).unwrap();
    assert!(json.contains("\"lineHash\":\"12345678\""));

    // Deserialize note with lineHash
    let deserialized: ReviewNote = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.line_hash, Some(hash));

    // Deserialize legacy note without lineHash
    let legacy_json =
        r#"{"id":"n2","file":"lib.rs","side":"new","start":5,"end":5,"text":"hi","createdAt":500}"#;
    let legacy_note: ReviewNote = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(legacy_note.line_hash, None);
}

#[test]
fn test_query_notes_for_file_scoping_and_standalone() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "note on scoped commit".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/other.rs".into(),
        side: Side::New,
        start: 20,
        end: 20,
        text: "note on other file".into(),
        created_at: 1001,
        line_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo@commit123", note1.clone());
    add_note(&mut db, "standalone_repo", note2.clone());

    // 1. Exact match
    let exact = query_notes_for_file(&db, Some("myrepo@commit123"), "src/lib.rs");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].id, "n1");

    // 2. Prefix match (repo without commit)
    let prefix_match = query_notes_for_file(&db, Some("myrepo"), "src/lib.rs");
    assert_eq!(prefix_match.len(), 1);
    assert_eq!(prefix_match[0].id, "n1");

    // 3. Standalone match (repo is None) finds notes by file path across all
    //    buckets
    let standalone = query_notes_for_file(&db, None, "src/lib.rs");
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, "n1");

    let standalone_other = query_notes_for_file(&db, None, "src/other.rs");
    assert_eq!(standalone_other.len(), 1);
    assert_eq!(standalone_other[0].id, "n2");

    // 4. Missing file returns empty vec
    let missing = query_notes_for_file(&db, None, "src/nonexistent.rs");
    assert!(missing.is_empty());

    // 5. Querying scoped commit does not leak notes from other commits
    let mut db_multi = NotesDB::new();
    add_note(&mut db_multi, "myrepo@commit123", note1.clone());
    add_note(&mut db_multi, "myrepo@commit456", note2.clone());
    let leaked = query_notes_for_file(&db_multi, Some("myrepo@commit123"), "src/other.rs");
    assert!(
        leaked.is_empty(),
        "Notes from commit456 must not leak when querying commit123"
    );
}

#[test]
fn test_note_files_for_repo() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "n1".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "note on lib".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/untouched.rs".into(),
        side: Side::New,
        start: 20,
        end: 20,
        text: "note on untouched".into(),
        created_at: 1001,
        line_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "myrepo@commit123", note1);
    add_note(&mut db, "myrepo@commit123", note2);

    // Exact repo match
    let files = note_files_for_repo(&db, Some("myrepo@commit123"));
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // Prefix match
    let files = note_files_for_repo(&db, Some("myrepo"));
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // Fallback across all buckets
    let files = note_files_for_repo(&db, None);
    assert_eq!(
        files,
        vec!["src/lib.rs".to_string(), "src/untouched.rs".to_string()]
    );

    // Non-existent repo without matches in other buckets
    let empty_db = NotesDB::new();
    let files = note_files_for_repo(&empty_db, Some("other"));
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
        side: Side::New,
        start: 1,
        end: 1,
        text: "hello".into(),
        created_at: 100,
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: None,
    };
    let json_open = serde_json::to_string(&note_open).unwrap();
    assert!(!json_open.contains("\"status\""));
    assert!(!json_open.contains("\"sourceLines\""));

    let note_in_progress = ReviewNote {
        status: NoteStatus::InProgress,
        source_lines: Some("fn foo() {}".into()),
        ..note_open.clone()
    };
    let json_ip = serde_json::to_string(&note_in_progress).unwrap();
    assert!(json_ip.contains("\"status\":\"inProgress\""));
    assert!(json_ip.contains("\"sourceLines\":\"fn foo() {}\""));

    let note_resolved = ReviewNote {
        status: NoteStatus::Resolved,
        ..note_open.clone()
    };
    let json_res = serde_json::to_string(&note_resolved).unwrap();
    assert!(json_res.contains("\"status\":\"resolved\""));

    // Deserialization without status or sourceLines defaults properly
    let raw_legacy = r#"{"id":"2","file":"b.rs","side":"new","start":5,"end":5,"text":"legacy","createdAt":200}"#;
    let parsed: ReviewNote = serde_json::from_str(raw_legacy).unwrap();
    assert_eq!(parsed.status, NoteStatus::Open);
    assert_eq!(parsed.source_lines, None);
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
        side: Side::New,
        start: 10,
        end: 10,
        text: "initial comment".into(),
        created_at: 1000,
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: Some("let x = 1;".into()),
    };
    add_note(&mut db, "repo1", note);

    // set_note_status_by_id
    assert!(set_note_status_by_id(
        &mut db,
        "repo1",
        "src/lib.rs",
        "n1",
        NoteStatus::InProgress
    ));
    let n = notes_for_file(&db, "repo1", "src/lib.rs").next().unwrap();
    assert_eq!(n.status, NoteStatus::InProgress);

    // resolve_note_by_id
    assert!(resolve_note_by_id(&mut db, "repo1", "src/lib.rs", "n1"));
    let n = notes_for_file(&db, "repo1", "src/lib.rs").next().unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);

    // reopen_note_by_id
    assert!(reopen_note_by_id(&mut db, "repo1", "src/lib.rs", "n1"));
    let n = notes_for_file(&db, "repo1", "src/lib.rs").next().unwrap();
    assert_eq!(n.status, NoteStatus::Open);

    // reply_to_note_by_id with status update
    assert!(reply_to_note_by_id(
        &mut db,
        "repo1",
        "src/lib.rs",
        "n1",
        "Addressed!",
        Some(NoteStatus::Resolved)
    ));
    let n = notes_for_file(&db, "repo1", "src/lib.rs").next().unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);
    assert_eq!(n.text, "Addressed!\n\n> initial comment");

    // Missing notes return false
    assert!(!set_note_status_by_id(
        &mut db,
        "repo1",
        "src/lib.rs",
        "missing",
        NoteStatus::Open
    ));
    assert!(!set_note_status_by_id(
        &mut db,
        "missing_repo",
        "src/lib.rs",
        "n1",
        NoteStatus::Open
    ));
    assert!(!reply_to_note_by_id(
        &mut db,
        "repo1",
        "src/lib.rs",
        "missing",
        "hi",
        None
    ));
}

#[test]
fn test_find_and_update_note_in_db() {
    let mut db = NotesDB::new();
    let note1 = ReviewNote {
        id: "abc-123".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "first note".into(),
        created_at: 100,
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: Some("println!();".into()),
    };
    let note2 = ReviewNote {
        id: "def-456".into(),
        file: "src/util.rs".into(),
        side: Side::Old,
        start: 20,
        end: 20,
        text: "second note".into(),
        created_at: 200,
        line_hash: None,
        status: NoteStatus::InProgress,
        source_lines: None,
    };
    add_note(&mut db, "myrepo@commit1", note1);
    add_note(&mut db, "other@commit2", note2);

    // Exact match by id with repo
    let (key, file, n) = find_note_in_db(&db, Some("myrepo"), "abc-123").expect("found");
    assert_eq!(key, "myrepo@commit1");
    assert_eq!(file, "src/main.rs");
    assert_eq!(n.id, "abc-123");

    // Prefix match
    let (key, file, n) = find_note_in_db(&db, None, "def").expect("found prefix");
    assert_eq!(key, "other@commit2");
    assert_eq!(file, "src/util.rs");
    assert_eq!(n.id, "def-456");

    // Not found
    assert!(find_note_in_db(&db, None, "xyz").is_none());

    // Update note in db
    let updated = update_note_in_db(&mut db, None, "abc", |n| {
        n.status = NoteStatus::Resolved;
    });
    assert!(updated);
    let (_, _, n) = find_note_in_db(&db, None, "abc-123").unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);
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
fn test_extract_source_lines() {
    let content = "line 1\nline 2\nline 3\nline 4\nline 5";
    // Single line
    assert_eq!(extract_source_lines(content, 1, 1), Some("line 1".into()));
    assert_eq!(extract_source_lines(content, 3, 3), Some("line 3".into()));
    // Multi-line range
    assert_eq!(
        extract_source_lines(content, 2, 4),
        Some("line 2\nline 3\nline 4".into())
    );
    // Full content
    assert_eq!(extract_source_lines(content, 1, 5), Some(content.into()));
    // End clamped to EOF
    assert_eq!(
        extract_source_lines(content, 4, 10),
        Some("line 4\nline 5".into())
    );
    // Out of bounds
    assert_eq!(extract_source_lines(content, 0, 1), None);
    assert_eq!(extract_source_lines(content, 4, 2), None);
    assert_eq!(extract_source_lines(content, 6, 10), None);
    // Empty content
    assert_eq!(extract_source_lines("", 1, 1), None);
}

#[test]
fn test_synthesize_source_lines_from_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("foo.rs");
    fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    // From direct path
    let lines = synthesize_source_lines(None, file_path.to_str().unwrap(), Side::New, 2, 2);
    assert_eq!(lines, Some("    println!(\"hello\");".into()));

    // From repo relative path
    let rel_lines = synthesize_source_lines(Some(tmp.path()), "foo.rs", Side::New, 1, 2);
    assert_eq!(
        rel_lines,
        Some("fn main() {\n    println!(\"hello\");".into())
    );

    // Non-existent file
    let missing = synthesize_source_lines(Some(tmp.path()), "nonexistent.rs", Side::New, 1, 1);
    assert_eq!(missing, None);
}
