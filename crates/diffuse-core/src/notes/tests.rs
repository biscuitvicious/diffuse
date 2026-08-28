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

    let remaining: Vec<_> = notes_for_file(&db, "myrepo", "src/main.rs").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "n2");
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
