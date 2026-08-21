//! Unit tests for note persistence, markdown formatting, and querying.

use super::*;
use std::{env, ffi::OsString, fs, path::PathBuf, process};

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
    };

    add_note(&mut db, "myrepo", note.clone());

    let repo_notes = notes_for_file(&db, "myrepo", "src/main.rs");
    assert_eq!(repo_notes.len(), 1);
    assert_eq!(repo_notes[0].id, "n1");

    let all = all_notes(&db, "myrepo");
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
    };
    let note2 = ReviewNote {
        id: "n2".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 20,
        end: 25,
        text: "Second".into(),
        created_at: 1001,
    };

    add_note(&mut db, "myrepo", note1);
    add_note(&mut db, "myrepo", note2);

    let removed = remove_notes(&mut db, "myrepo", "src/main.rs", Side::New, 12, 18);
    assert_eq!(removed, 1);

    let remaining = notes_for_file(&db, "myrepo", "src/main.rs");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
}

#[test]
fn test_notes_persistence() {
    let file = env::temp_dir().join(format!("diffuse_test_notes_{}.json", process::id()));

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
        },
    );

    save_notes(&file, &db).unwrap();

    let loaded = load_notes(&file);
    assert_eq!(loaded.len(), 1);
    let loaded_notes = notes_for_file(&loaded, "repo1", "lib.rs");
    assert_eq!(loaded_notes.len(), 1);
    assert_eq!(loaded_notes[0].text, "**important**");

    let _ = fs::remove_file(file);
}

#[test]
fn test_render_note_markdown_inline() {
    let text = "This is **bold** and `code` span";
    let tokens = render_note_markdown(text);

    let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "This is ", "**", "bold", "**", " and ", "`", "code", "`", " span"
        ]
    );

    let colors: Vec<&str> = tokens.iter().map(|t| t.color.as_str()).collect();
    assert_eq!(colors[0], "warning");
    assert_eq!(colors[2], "mdBold");
    assert_eq!(colors[6], "syntaxString");
}
