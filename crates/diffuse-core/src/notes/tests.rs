//! Unit tests for note persistence, markdown formatting, and querying.

use super::*;
use std::{ffi::OsString, path::PathBuf};

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
        let out = std::process::Command::new("git")
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
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
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
        let out = std::process::Command::new("git")
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

    std::fs::write(dir.join("a.rs"), "1\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);
    let c1 = head_commit(dir).unwrap();

    std::fs::write(dir.join("a.rs"), "2\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit2"]);
    let c2 = head_commit(dir).unwrap();

    std::fs::write(dir.join("a.rs"), "3\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit3"]);
    let c3 = head_commit(dir).unwrap();

    std::fs::write(dir.join("a.rs"), "4\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit4"]);
    let c4 = head_commit(dir).unwrap();

    // 1. Working tree / no range -> keys off HEAD (c4)
    assert_eq!(notes_commit(dir, &[]), Some(c4.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &[]),
        format!("myrepo@{c4}")
    );

    // 2. Single-commit range c1..c2 -> next newest in range is c1 (commit
    //    before c2)
    let r_1_2 = vec![format!("{c1}..{c2}")];
    assert_eq!(notes_commit(dir, &r_1_2), Some(c1.clone()));
    assert_eq!(
        notes_key_for_repo_args(dir, "myrepo", &r_1_2),
        format!("myrepo@{c1}")
    );

    // 3. Multi-commit range c1..c4 (commits: c2, c3, c4) -> second to last
    //    commit in range (next newest) is c3
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
fn test_render_note_markdown_inline() {
    let text = "This is **bold** and `code` span";
    let tokens = render_note_markdown(text);

    let texts: Vec<&str> = tokens.iter().map(|t| t.text).collect();
    assert_eq!(
        texts,
        vec![
            "This is ", "**", "bold", "**", " and ", "`", "code", "`", " span"
        ]
    );

    let colors: Vec<&str> = tokens.iter().map(|t| t.color).collect();
    assert_eq!(colors[0], "warning");
    assert_eq!(colors[2], "mdBold");
    assert_eq!(colors[6], "syntaxString");
}

#[test]
fn test_render_note_markdown_underscores_and_asterisks() {
    let text = "**Markdown** __note!__ and *italic* _also italic_";
    let tokens = render_note_markdown(text);

    let texts: Vec<&str> = tokens.iter().map(|t| t.text).collect();
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

    let colors: Vec<&str> = tokens.iter().map(|t| t.color).collect();
    assert_eq!(
        colors,
        vec![
            "accent", "mdBold", "accent", "warning", "accent", "mdBold", "accent", "warning",
            "accent", "mdItalic", "accent", "warning", "accent", "mdItalic", "accent"
        ]
    );

    // Verify SGR modifiers are set for bold and italic inner content
    assert_eq!(tokens[1].sgr, Some(crate::syntax::profile::MD_SGR_BOLD));
    assert_eq!(tokens[5].sgr, Some(crate::syntax::profile::MD_SGR_BOLD));
    assert_eq!(tokens[9].sgr, Some(crate::syntax::profile::MD_SGR_ITALIC));
    assert_eq!(tokens[13].sgr, Some(crate::syntax::profile::MD_SGR_ITALIC));
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
    assert_eq!(std::mem::size_of::<LineHash>(), 4);
    assert_eq!(std::mem::size_of::<Option<LineHash>>(), 4);
    assert_eq!(std::mem::align_of::<Option<LineHash>>(), 4);
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
    };
    assert!(note_anchors_cleanly(&legacy_note, "any changed content"));
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
