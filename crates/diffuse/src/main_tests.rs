use std::{env, sync::Mutex};

use clap::Parser;

use diffuse::DiffuseTheme;

use super::{Args, route_mode};

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
    };
    add_note(&mut db, "some_repo@commit", note);
    save_notes(&notes_path, &db).unwrap();

    let base_args = Args::try_parse_from(["diffuse"]).unwrap();

    // 1. Standalone piped diff with NO notes_file -> notes are inactive (0 notes)
    let input_no_notes = super::Input {
        action: None,
        note: None,
        file: None,
        side: None,
        start: None,
        end: None,
        diff: diff.to_string(),
        new_content: None,
        old_content: None,
        repo_root: None,
        base_args: Vec::new(),
        paths: Vec::new(),
        staged: false,
        context: None,
        notes: None,
        notes_file: None,
        repo: None,
        token_product_cap: None,
        cluster_budget: None,
        cluster_bandwidth_min: None,
        cluster_bandwidth_slack: None,
    };
    let out_no_notes = super::process_input(input_no_notes.clone(), &base_args);
    assert_eq!(out_no_notes.files.len(), 1);
    assert_eq!(out_no_notes.files[0].counts.notes, 0);
    assert!(out_no_notes.files[0].notes.is_empty());

    // 2. Standalone piped diff WITH explicit notes_file -> notes are active and
    //    matched by path
    let input_with_notes = super::Input {
        notes_file: Some(notes_path.to_str().unwrap().to_string()),
        ..super::Input {
            action: None,
            note: None,
            file: None,
            side: None,
            start: None,
            end: None,
            diff: diff.to_string(),
            new_content: None,
            old_content: None,
            repo_root: None,
            base_args: Vec::new(),
            paths: Vec::new(),
            staged: false,
            context: None,
            notes: None,
            notes_file: None,
            repo: None,
            token_product_cap: None,
            cluster_budget: None,
            cluster_bandwidth_min: None,
            cluster_bandwidth_slack: None,
        }
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
