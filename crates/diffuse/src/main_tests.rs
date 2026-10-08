use std::{env, sync::Mutex};

use clap::Parser;

use diffuse::DiffuseTheme;

use super::{Args, SubCommand, route_mode};

#[test]
fn extension_json_payload_is_json_mode() {
    // The pi extension pipes a JSON payload with no git args.
    assert!(route_mode(
        false,
        false,
        None,
        &[],
        false,
        "{\"diff\":\"x\"}"
    ));
}

#[test]
fn piped_raw_diff_stays_json_mode() {
    // `git diff | diffuse` — a raw (non-JSON) payload piped in.
    assert!(route_mode(
        false,
        false,
        None,
        &[],
        false,
        "diff --git a b\n"
    ));
}

#[test]
fn empty_piped_stdin_is_standalone() {
    // `diffuse < /dev/null | cat` — no payload, must route to the
    // standalone reviewer, never the JSON contract.
    assert!(!route_mode(false, false, None, &[], false, ""));
}

#[test]
fn interactive_stdin_is_standalone() {
    assert!(!route_mode(false, false, None, &[], true, ""));
}

#[test]
fn explicit_json_flag_is_json_mode() {
    assert!(route_mode(true, false, None, &[], true, ""));
}

#[test]
fn explicit_jsonl_flag_is_json_mode() {
    assert!(route_mode(false, true, None, &[], true, ""));
}

#[test]
fn diff_file_flag_is_json_mode() {
    assert!(route_mode(false, false, Some("x.diff"), &[], true, ""));
}

#[test]
fn git_args_are_always_standalone() {
    // `diffuse HEAD < file` — git args win over piped stdin content.
    assert!(!route_mode(
        false,
        false,
        None,
        &["HEAD".into()],
        false,
        "noise"
    ));
}

#[test]
fn test_init_color_mode_always() {
    let enabled = super::init_color_mode("always", false);
    assert!(enabled);
    assert!(yansi::is_enabled());
}

#[test]
fn test_init_color_mode_never() {
    let enabled = super::init_color_mode("never", false);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
}

#[test]
fn test_init_color_mode_no_color_flag() {
    let enabled = super::init_color_mode("always", true);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
}

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_init_color_mode_auto_default() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("NO_COLOR") };
    let enabled = super::init_color_mode("auto", false);
    assert!(enabled);
    assert!(yansi::is_enabled());
}

#[test]
fn test_init_color_mode_auto_with_no_color_env() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::set_var("NO_COLOR", "1") };
    let enabled = super::init_color_mode("auto", false);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
    unsafe { env::remove_var("NO_COLOR") };
}

#[test]
fn test_cli_args_no_color() {
    let args = Args::try_parse_from(["diffuse", "--no-color", "--pager"]).unwrap();
    assert!(args.no_color);
    assert!(args.pager);
}

#[test]
fn test_cli_args_theme_flag() {
    let args = Args::try_parse_from(["diffuse", "--theme", "dark"]).unwrap();
    assert_eq!(args.theme.as_deref(), Some("dark"));
}

#[test]
fn test_resolve_theme_prefers_cli_flag() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("DIFFUSE_THEME") };
    assert_eq!(super::resolve_theme(Some("dark")), DiffuseTheme::DEFAULT);
    assert_eq!(super::resolve_theme(Some("light")), DiffuseTheme::LIGHT);
    let auto_theme = super::resolve_theme(Some("auto"));
    assert!(auto_theme == DiffuseTheme::DEFAULT || auto_theme == DiffuseTheme::LIGHT);
    assert_eq!(super::resolve_theme(None), DiffuseTheme::from_env());
}

#[test]
fn test_resolve_theme_falls_back_on_unknown_name() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("DIFFUSE_THEME") };
    assert_eq!(
        super::resolve_theme(Some("bogus-theme")),
        DiffuseTheme::DEFAULT
    );
}

#[test]
fn test_cli_args_jobs_flags() {
    let args1 = Args::try_parse_from(["diffuse", "-j", "4"]).unwrap();
    assert_eq!(args1.jobs, Some(4));

    let args2 = Args::try_parse_from(["diffuse", "--jobs", "8"]).unwrap();
    assert_eq!(args2.jobs, Some(8));

    let args3 = Args::try_parse_from(["diffuse", "--threads", "2"]).unwrap();
    assert_eq!(args3.jobs, Some(2));

    let args4 = Args::try_parse_from(["diffuse", "--workers", "6"]).unwrap();
    assert_eq!(args4.jobs, Some(6));

    let args_default = Args::try_parse_from(["diffuse"]).unwrap();
    assert_eq!(args_default.jobs, None);
}

#[test]
fn test_standalone_piped_notes_inactivity_and_explicit_file() {
    use diffuse::core::notes::{ReviewNote, Side, add_note, save_notes};

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
 line 1
-line 2
+line 2 modified
 line 3
";
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let notes_path = tmp.path().to_path_buf();

    let mut db = std::collections::HashMap::new();
    let note = ReviewNote {
        id: "note-1".into(),
        file: "test.rs".into(),
        side: Side::New,
        start: 2,
        end: 2,
        text: "piped note".into(),
        created_at: 1000,
        line_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "some_repo@commit", note);
    save_notes(&notes_path, &db).unwrap();

    let base_args = Args::try_parse_from(["diffuse"]).unwrap();

    // 1. Standalone piped diff with NO notes_file -> notes are inactive (0 notes)
    let input_no_notes = super::Input {
        diff: diff.to_string(),
        ..Default::default()
    };
    let out_no_notes = super::process_input(input_no_notes.clone(), &base_args);
    assert_eq!(out_no_notes.files.len(), 1);
    assert_eq!(out_no_notes.files[0].counts.notes, 0);
    assert!(out_no_notes.files[0].notes.is_empty());

    // 2. Standalone piped diff WITH explicit notes_file -> notes are active and
    //    matched by path
    let input_with_notes = super::Input {
        notes_file: Some(notes_path.to_str().unwrap().to_string()),
        ..input_no_notes.clone()
    };
    let out_with_notes = super::process_input(input_with_notes, &base_args);
    assert_eq!(out_with_notes.files.len(), 1);
    assert_eq!(out_with_notes.files[0].counts.notes, 1);
    assert_eq!(out_with_notes.files[0].notes.len(), 1);
    assert_eq!(out_with_notes.files[0].notes[0].id, "note-1");

    // 3. addNote in standalone mode without notes_file is a no-op
    let new_note = ReviewNote {
        id: "note-2".into(),
        file: "test.rs".into(),
        side: Side::New,
        start: 3,
        end: 3,
        text: "another note".into(),
        created_at: 1002,
        line_hash: None,
        ..Default::default()
    };
    let input_add_no_file = super::Input {
        action: Some("addNote".into()),
        note: Some(new_note.clone()),
        repo: Some("standalone_repo".into()),
        notes_file: None,
        ..input_no_notes.clone()
    };
    let _ = super::process_input(input_add_no_file, &base_args);
    // db on disk was NOT modified
    let reloaded = diffuse::core::notes::load_notes(&notes_path);
    assert_eq!(
        reloaded
            .get("some_repo@commit")
            .unwrap()
            .get("test.rs")
            .unwrap()
            .len(),
        1
    );

    // 4. addNote in standalone mode WITH notes_file saves to that file
    let input_add_with_file = super::Input {
        action: Some("addNote".into()),
        note: Some(new_note),
        repo: Some("standalone_repo".into()),
        notes_file: Some(notes_path.to_str().unwrap().to_string()),
        ..input_no_notes
    };
    let _ = super::process_input(input_add_with_file, &base_args);
    let reloaded_after = diffuse::core::notes::load_notes(&notes_path);
    assert!(reloaded_after.contains_key("standalone_repo"));
    assert_eq!(
        reloaded_after
            .get("standalone_repo")
            .unwrap()
            .get("test.rs")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_json_protocol_note_status_and_reply_actions() {
    use diffuse::core::notes::{NoteStatus, ReviewNote, Side, load_notes, save_notes};

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let notes_path = tmp.path().to_path_buf();

    let mut db = std::collections::HashMap::new();
    let note = ReviewNote {
        id: "note-abc".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 5,
        end: 5,
        text: "Please refactor".into(),
        created_at: 1000,
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: None,
    };
    diffuse::core::notes::add_note(&mut db, "repo1", note);
    save_notes(&notes_path, &db).unwrap();

    let base_args = Args::try_parse_from(["diffuse"]).unwrap();
    let base_input = super::Input {
        action: None,
        note: None,
        file: None,
        side: None,
        start: None,
        end: None,
        diff: "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n".into(),
        new_content: None,
        old_content: None,
        repo_root: None,
        base_args: vec![],
        paths: vec![],
        staged: false,
        context: None,
        notes: None,
        notes_file: Some(notes_path.to_str().unwrap().to_string()),
        repo: Some("repo1".into()),
        token_product_cap: None,
        cluster_budget: None,
        cluster_bandwidth_min: None,
        cluster_bandwidth_slack: None,
        note_id: Some("note-abc".into()),
        status: None,
        reply_text: None,
        ..Default::default()
    };

    // 1. setNoteStatus to InProgress
    let mut input_progress = base_input.clone();
    input_progress.action = Some("setNoteStatus".into());
    input_progress.status = Some(NoteStatus::InProgress);
    let _ = super::process_input(input_progress, &base_args);
    let db = load_notes(&notes_path);
    assert_eq!(
        db.get("repo1").unwrap().get("src/main.rs").unwrap()[0].status,
        NoteStatus::InProgress
    );

    // 2. replyNote with Resolved status
    let mut input_reply = base_input.clone();
    input_reply.action = Some("replyNote".into());
    input_reply.reply_text = Some("Refactored into helper function".into());
    input_reply.status = Some(NoteStatus::Resolved);
    let _ = super::process_input(input_reply, &base_args);
    let db = load_notes(&notes_path);
    let note = &db.get("repo1").unwrap().get("src/main.rs").unwrap()[0];
    assert_eq!(note.status, NoteStatus::Resolved);
    assert_eq!(
        note.text,
        "Refactored into helper function\n\n> Please refactor"
    );

    // 3. reopenNote
    let mut input_reopen = base_input.clone();
    input_reopen.action = Some("reopenNote".into());
    let _ = super::process_input(input_reopen, &base_args);
    let db = load_notes(&notes_path);
    assert_eq!(
        db.get("repo1").unwrap().get("src/main.rs").unwrap()[0].status,
        NoteStatus::Open
    );

    // 4. resolveNote
    let mut input_resolve = base_input.clone();
    input_resolve.action = Some("resolveNote".into());
    let _ = super::process_input(input_resolve, &base_args);
    let db = load_notes(&notes_path);
    assert_eq!(
        db.get("repo1").unwrap().get("src/main.rs").unwrap()[0].status,
        NoteStatus::Resolved
    );
}

#[test]
fn test_notes_cli_arg_parsing() {
    use super::{NotesAction, SubCommand};

    let args = Args::try_parse_from(["diffuse", "notes"]).unwrap();
    assert!(matches!(args.command, Some(SubCommand::Notes(_))));

    let args = Args::try_parse_from(["diffuse", "notes", "list", "--resolved"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::List { resolved: true, .. })
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args =
        Args::try_parse_from(["diffuse", "notes", "show", "abc-123", "--show-source"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(
            matches!(notes.action, Some(NotesAction::Show { ref id, show_source: true, .. }) if id == "abc-123")
        );
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from([
        "diffuse",
        "notes",
        "reply",
        "abc-123",
        "-m",
        "All good",
        "--resolve",
    ])
    .unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(
            matches!(notes.action, Some(NotesAction::Reply { ref id, message: Some(ref msg), resolve: true, .. }) if id == "abc-123" && msg == "All good")
        );
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "progress", "abc-123"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(notes.action, Some(NotesAction::Progress { ref id }) if id == "abc-123"));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "resolve", "abc-123"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(notes.action, Some(NotesAction::Resolve { ref id }) if id == "abc-123"));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "open", "abc-123"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(notes.action, Some(NotesAction::Open { ref id }) if id == "abc-123"));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "reopen", "abc-123"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(notes.action, Some(NotesAction::Reopen { ref id }) if id == "abc-123"));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "status", "abc-123", "resolved"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(
            matches!(notes.action, Some(NotesAction::Status { ref id, ref status }) if id == "abc-123" && status == "resolved")
        );
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from([
        "diffuse",
        "notes",
        "add",
        "src/lib.rs:10-20",
        "-m",
        "hello",
        "--source",
        "code",
    ])
    .unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::Add {
                target: Some(ref t),
                message: Some(ref msg),
                source: Some(ref src),
                ..
            }) if t == "src/lib.rs:10-20" && msg == "hello" && src == "code"
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from([
        "diffuse",
        "notes",
        "create",
        "src/lib.rs",
        "15",
        "-m",
        "hello",
    ])
    .unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::Add {
                target: Some(ref t),
                pos2: Some(ref p2),
                ..
            }) if t == "src/lib.rs" && p2 == "15"
        ));
    } else {
        panic!("expected Notes subcommand");
    }
}

#[test]
fn test_cli_args_resolved_all_show_source() {
    let args = Args::try_parse_from(["diffuse", "--resolved"]).unwrap();
    assert!(args.resolved);
    assert!(!args.all);
    assert!(!args.show_source);

    let args = Args::try_parse_from(["diffuse", "--all"]).unwrap();
    assert!(args.all);

    let args = Args::try_parse_from(["diffuse", "-a"]).unwrap();
    assert!(args.all);

    let args = Args::try_parse_from(["diffuse", "--all-notes"]).unwrap();
    assert!(args.all);

    let args = Args::try_parse_from(["diffuse", "--show-source"]).unwrap();
    assert!(args.show_source);

    let args = Args::try_parse_from(["diffuse", "parse", "--resolved", "--show-source"]).unwrap();
    if let Some(SubCommand::Parse {
        resolved,
        show_source,
        all,
        ..
    }) = args.command
    {
        assert!(resolved);
        assert!(show_source);
        assert!(!all);
    } else {
        panic!("expected Parse subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "parse", "-a"]).unwrap();
    if let Some(SubCommand::Parse { all, .. }) = args.command {
        assert!(all);
    } else {
        panic!("expected Parse subcommand");
    }
}
