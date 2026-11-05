use std::{env, sync::Mutex};

use clap::Parser;

use diffuse::prelude::*;

use super::{Args, SubCommand, ThemeTarget, is_terminal_interactive, route_mode};

#[test]
fn piped_diff_is_stream_mode() {
    assert!(route_mode(false, false, None, &[], false, true));
}

#[test]
fn interactive_stdin_is_git_mode() {
    assert!(!route_mode(false, false, None, &[], true, false));
}

#[test]
fn explicit_json_flag_is_diff_stream_mode() {
    assert!(route_mode(true, false, None, &[], true, false));
}

#[test]
fn explicit_jsonl_flag_is_diff_stream_mode() {
    assert!(route_mode(false, true, None, &[], true, false));
}

#[test]
fn diff_file_flag_is_diff_stream_mode() {
    assert!(route_mode(false, false, Some("x.diff"), &[], true, false));
}

#[test]
fn git_args_are_always_git_mode() {
    assert!(!route_mode(
        false,
        false,
        None,
        &["HEAD".into()],
        false,
        false
    ));
}

#[test]
fn test_is_terminal_interactive_true_when_stdin_tty() {
    assert!(is_terminal_interactive(true));
}

#[test]
fn test_is_terminal_interactive_matches_dev_tty_when_stdin_not_tty() {
    let result = is_terminal_interactive(false);
    #[cfg(unix)]
    {
        let can_open = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .is_ok();
        assert_eq!(result, can_open);
    }
    #[cfg(not(unix))]
    {
        assert!(!result);
    }
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
fn test_cli_args_split_revisions_from_pathspecs() {
    // Revisions and pathspecs are separate semantic inputs: everything before
    // `--` is a revision/range, everything after `--` is a pathspec.
    let args = Args::try_parse_from([
        "diffuse",
        "-p",
        "68cb770..6613139",
        "--",
        "crates/diffuse/src/main.rs",
    ])
    .unwrap();
    assert_eq!(args.revision.git_revs, ["68cb770..6613139"]);
    assert_eq!(
        args.revision.pathspecs,
        ["crates/diffuse/src/main.rs"],
        "pathspecs after `--` must parse into their own field, never into git_revs"
    );

    // Without `--`, revision args stay verbatim and pathspecs stay empty.
    let args = Args::try_parse_from(["diffuse", "-p", "HEAD~1"]).unwrap();
    assert_eq!(args.revision.git_revs, ["HEAD~1"]);
    assert!(args.revision.pathspecs.is_empty());
}

#[test]
fn test_cli_args_split_revisions_from_pathspecs_review_subcommand() {
    let args =
        Args::try_parse_from(["diffuse", "review", "main..feature", "--", "src/lib.rs"]).unwrap();
    let SubCommand::Review { revision } = args.command.unwrap() else {
        panic!("expected review subcommand");
    };
    assert_eq!(revision.git_revs, ["main..feature"]);
    assert_eq!(revision.pathspecs, ["src/lib.rs"]);
}

#[test]
fn test_resolve_theme_prefers_cli_flag() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("DIFFUSE_THEME") };
    assert_eq!(
        super::resolve_theme(Some("dark"), ThemeTarget::Batch),
        DiffuseTheme::dark()
    );
    assert_eq!(
        super::resolve_theme(Some("light"), ThemeTarget::Batch),
        DiffuseTheme::light()
    );
    let auto_theme = super::resolve_theme(Some("auto"), ThemeTarget::Batch);
    assert!(auto_theme == DiffuseTheme::dark() || auto_theme == DiffuseTheme::light());
    assert_eq!(
        super::resolve_theme(None, ThemeTarget::Batch),
        DiffuseTheme::from_env()
    );
}

/// Batch resolution must stay on the environment-only pipeline even for
/// explicit `auto`: no OSC 11 query may be issued, so no late terminal
/// response can leak into the shell as typed input. The pager arm may add
/// the query, but only while its output is a terminal, and never changes the
/// environment tiers. (The pty end-to-end tests prove the byte-level
/// behavior; here the routing inputs are pinned.)
#[test]
fn test_resolve_theme_non_interactive_auto_matches_env_only() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe {
        env::remove_var("DIFFUSE_THEME");
        env::remove_var("COLORFGBG");
        env::remove_var("TERM_BACKGROUND");
    }
    // Named themes resolve identically in both modes (no query either way).
    assert_eq!(
        super::resolve_theme(Some("dark"), ThemeTarget::Batch),
        super::resolve_theme(Some("dark"), ThemeTarget::Tui)
    );
    assert_eq!(
        super::resolve_theme(Some("light"), ThemeTarget::Batch),
        super::resolve_theme(Some("light"), ThemeTarget::Tui)
    );
    // Explicit auto in a non-TTY test session falls back to the default;
    // the interactive and pager arms may only add the query, never change
    // env tiers.
    let auto_safe = super::resolve_theme(Some("auto"), ThemeTarget::Batch);
    assert!(auto_safe == DiffuseTheme::dark() || auto_safe == DiffuseTheme::light());
    let auto_pager = super::resolve_theme(Some("auto"), ThemeTarget::Pager);
    assert_eq!(auto_pager, auto_safe);
    unsafe {
        env::set_var("DIFFUSE_THEME", "light");
        assert_eq!(
            super::resolve_theme(None, ThemeTarget::Batch),
            DiffuseTheme::light()
        );
        assert_eq!(
            super::resolve_theme(None, ThemeTarget::Tui),
            DiffuseTheme::light()
        );
        assert_eq!(
            super::resolve_theme(None, ThemeTarget::Pager),
            DiffuseTheme::light()
        );
        env::remove_var("DIFFUSE_THEME");
    }
}

#[test]
fn test_resolve_theme_falls_back_on_unknown_name() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe {
        env::remove_var("DIFFUSE_THEME");
        env::remove_var("COLORFGBG");
        env::remove_var("TERM_BACKGROUND");
    }
    assert_eq!(
        super::resolve_theme(Some("bogus-theme"), ThemeTarget::Batch),
        DiffuseTheme::dark()
    );
}

/// Phase 7: `--theme` accepts a file path to a custom TOML theme; the
/// resolved theme carries the document's name and compiled palette.
#[test]
fn test_resolve_theme_loads_theme_file_from_path() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("DIFFUSE_THEME") };
    let dir = tempfile::TempDir::new().unwrap();
    let theme_path = dir.path().join("custom.toml");
    std::fs::write(
        &theme_path,
        "inherits = \"dark\"\nname = \"path-custom\"\n[ui]\naccent = 200\n",
    )
    .unwrap();

    let theme = super::resolve_theme(Some(theme_path.to_str().unwrap()), ThemeTarget::Batch);
    assert_eq!(theme.name, "path-custom");
    assert_eq!(theme.mode, diffuse::render::theme::Mode::Dark);
    assert_eq!(
        theme.palette.ui.accent,
        diffuse::render::Color::Fixed(200),
        "the custom accent override survives the cascade"
    );
}

/// A path that exists but fails to parse falls back to the default theme
/// with an informative error (graceful degradation, not a panic).
#[test]
fn test_resolve_theme_falls_back_on_malformed_theme_file() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe { env::remove_var("DIFFUSE_THEME") };
    let dir = tempfile::TempDir::new().unwrap();
    let theme_path = dir.path().join("broken.toml");
    std::fs::write(&theme_path, "not [ valid [ toml").unwrap();

    assert_eq!(
        super::resolve_theme(Some(theme_path.to_str().unwrap()), ThemeTarget::Batch),
        DiffuseTheme::dark()
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
fn test_diff_paths_files_and_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();

    let f_old = old_dir.join("test.txt");
    let f_new = new_dir.join("test.txt");
    std::fs::write(&f_old, "line 1\nline 2\n").unwrap();
    std::fs::write(&f_new, "line 1\nline 2 modified\n").unwrap();

    // 1. Direct file diffing
    let file_diff = diffuse::git::diff_paths(
        &f_old,
        &f_new,
        3,
        diffuse::git::WhitespaceMode::TrimEquality,
    )
    .unwrap();
    assert_eq!(file_diff.files.len(), 1);
    assert_eq!(
        file_diff.files[0].status,
        diffuse::core::FileStatus::Modified
    );
    assert_eq!(file_diff.files[0].hunks.len(), 1);

    // 2. Direct directory diffing
    let dir_diff = diffuse::git::diff_paths(
        &old_dir,
        &new_dir,
        3,
        diffuse::git::WhitespaceMode::TrimEquality,
    )
    .unwrap();
    assert_eq!(dir_diff.files.len(), 1);
    assert_eq!(dir_diff.files[0].path, "test.txt");
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
        "--old",
        "9-19",
    ])
    .unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::Add {
                target: Some(ref t),
                message: Some(ref msg),
                old: Some(ref old),
                new: None,
                ..
            }) if t == "src/lib.rs:10-20" && msg == "hello" && old == "9-19"
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from([
        "diffuse",
        "notes",
        "create",
        "src/lib.rs",
        "--line",
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
                new: Some(ref new),
                ..
            }) if t == "src/lib.rs" && new == "15"
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from([
        "diffuse",
        "notes",
        "create",
        "src/lib.rs:42",
        "-m",
        "range note",
        "-r",
        "c1..c2",
    ])
    .unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::Add {
                target: Some(ref t),
                rev: Some(ref r),
                ..
            }) if t == "src/lib.rs:42" && r == "c1..c2"
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "list", "-r", "HEAD~1"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::List {
                rev: Some(ref r),
                ..
            }) if r == "HEAD~1"
        ));
    } else {
        panic!("expected Notes subcommand");
    }

    let args = Args::try_parse_from(["diffuse", "notes", "-r", "HEAD~1"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert_eq!(notes.rev.as_deref(), Some("HEAD~1"));
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

#[test]
fn test_clap_debug_assert() {
    use clap::CommandFactory;
    Args::command().debug_assert();
}

#[test]
fn test_clap_root_help_sections_and_options() {
    use clap::CommandFactory;
    let mut cmd = Args::command();
    let help = cmd.render_help().to_string();

    assert!(help.contains("Execution Modes:"));
    assert!(help.contains("Output Formats:"));
    assert!(help.contains("Theme & Styling:"));
    assert!(help.contains("Review Notes:"));
    assert!(help.contains("Diff & Performance Tuning:"));
    assert!(help.contains("Repository & Input:"));
    assert!(help.contains("TYPICAL USAGE & EXAMPLES:"));
    assert!(help.contains("ENVIRONMENT VARIABLES:"));

    assert!(help.contains("--pager"));
    assert!(help.contains("--no-tui"));
    assert!(help.contains("--json"));
    assert!(help.contains("--jsonl"));
    assert!(help.contains("-m, --markdown"));
    assert!(help.contains("--theme"));
    assert!(help.contains("--notes-file"));
    assert!(!help.contains(".pi"));
    assert!(help.contains("~/.local/state/diffuse/notes.json"));

    // Verify "Given..." text is removed from root long_about
    assert!(!help.contains("Given a unified diff"));

    // Verify tuning options are hidden from help output
    assert!(!help.contains("--token-product-cap"));
    assert!(!help.contains("--cluster-budget"));
    assert!(!help.contains("--cluster-bandwidth-min"));
    assert!(!help.contains("--cluster-bandwidth-slack"));

    // Verify hidden options still parse correctly from CLI
    let parsed = Args::try_parse_from([
        "diffuse",
        "--token-product-cap",
        "99999",
        "--cluster-budget",
        "12345",
        "--cluster-bandwidth-min",
        "42",
        "--cluster-bandwidth-slack",
        "16",
    ])
    .unwrap();
    assert_eq!(parsed.token_product_cap, 99999);
    assert_eq!(parsed.cluster_budget, 12345);
    assert_eq!(parsed.cluster_bandwidth_min, 42);
    assert_eq!(parsed.cluster_bandwidth_slack, 16);
}

#[test]
fn test_clap_subcommands_help() {
    use clap::CommandFactory;
    let cmd = Args::command();

    let subcommands = ["review", "parse", "notes"];
    for sub in subcommands {
        let sub_cmd = cmd
            .find_subcommand(sub)
            .unwrap_or_else(|| panic!("missing subcommand {sub}"));
        let help = sub_cmd.clone().render_help().to_string();
        assert!(!help.is_empty(), "empty help for {sub}");
    }

    let notes_cmd = cmd.find_subcommand("notes").unwrap();
    let notes_subcommands = [
        "list", "add", "show", "reply", "progress", "resolve", "open", "reopen", "status",
    ];
    for sub in notes_subcommands {
        let sub_cmd = notes_cmd
            .find_subcommand(sub)
            .unwrap_or_else(|| panic!("missing notes subcommand {sub}"));
        let help = sub_cmd.clone().render_help().to_string();
        assert!(!help.is_empty(), "empty help for notes {sub}");
        assert!(
            help.contains("EXAMPLES:"),
            "missing EXAMPLES section in notes {sub}"
        );
    }

    let add_help = notes_cmd
        .find_subcommand("add")
        .unwrap()
        .clone()
        .render_help()
        .to_string();
    assert!(add_help.contains("Target & Coordinates:"));
    assert!(add_help.contains("Message Content:"));
    assert!(add_help.contains("Status & Output:"));

    let reply_help = notes_cmd
        .find_subcommand("reply")
        .unwrap()
        .clone()
        .render_help()
        .to_string();
    assert!(reply_help.contains("Reply Content:"));
    assert!(reply_help.contains("Status Transitions:"));
}

#[test]
fn test_notes_cli_args_revisions_and_staged() {
    use super::{NotesAction, RevisionArgs};

    // 1. `diffuse notes list HEAD~1`
    let args = Args::try_parse_from(["diffuse", "notes", "list", "HEAD~1"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::List { ref revision, .. }) if revision.git_revs == ["HEAD~1"]
        ));
    } else {
        panic!("expected Notes");
    }

    // 2. `diffuse notes list HEAD~1 src/lib.rs`
    let args = Args::try_parse_from(["diffuse", "notes", "list", "HEAD~1", "src/lib.rs"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(matches!(
            notes.action,
            Some(NotesAction::List { ref revision, .. }) if revision.git_revs == ["HEAD~1", "src/lib.rs"]
        ));
    } else {
        panic!("expected Notes");
    }

    // 3. `diffuse notes HEAD~1`
    let args = Args::try_parse_from(["diffuse", "notes", "HEAD~1"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(notes.action.is_none());
        assert_eq!(notes.revision.git_revs, vec!["HEAD~1"]);
    } else {
        panic!("expected Notes");
    }

    // 4. `diffuse notes src/lib.rs`
    let args = Args::try_parse_from(["diffuse", "notes", "src/lib.rs"]).unwrap();
    if let Some(SubCommand::Notes(notes)) = args.command {
        assert!(notes.action.is_none());
        assert_eq!(notes.revision.git_revs, vec!["src/lib.rs"]);
    } else {
        panic!("expected Notes");
    }

    // 5. `diffuse notes list --staged`
    let args = Args::try_parse_from(["diffuse", "notes", "list", "--staged"]).unwrap();
    assert!(args.staged);

    // 6. `diffuse notes list --cached`
    let args = Args::try_parse_from(["diffuse", "notes", "list", "--cached"]).unwrap();
    assert!(args.staged);

    // 7. `diffuse HEAD~1` and `diffuse review HEAD~1` sharing RevisionArgs
    let args_root = Args::try_parse_from(["diffuse", "HEAD~1"]).unwrap();
    assert_eq!(args_root.revision.git_revs, vec!["HEAD~1"]);

    let args_review = Args::try_parse_from(["diffuse", "review", "HEAD~1"]).unwrap();
    if let Some(SubCommand::Review { revision }) = args_review.command {
        assert_eq!(revision.git_revs, vec!["HEAD~1"]);
    } else {
        panic!("expected Review");
    }

    // 8. RevisionArgs helper methods
    let r1 = RevisionArgs::new(vec!["HEAD~1".into()]);
    assert!(!r1.is_empty());
    let r2 = RevisionArgs::new(vec!["src/main.rs".into()]);
    let combined = r1.combined(&r2);
    assert_eq!(combined.git_revs, vec!["HEAD~1", "src/main.rs"]);
}

#[test]
fn test_pager_routing_and_detect_terminal_width() {
    let args = Args::try_parse_from(["diffuse", "-p", "src/"]).unwrap();
    assert!(args.pager);

    let term_width = detect_terminal_width();
    assert!(term_width > 0);
}

#[test]
fn test_no_watch_flag() {
    let args_default = Args::try_parse_from(["diffuse"]).unwrap();
    assert!(!args_default.no_watch);

    let args_no_watch = Args::try_parse_from(["diffuse", "--no-watch"]).unwrap();
    assert!(args_no_watch.no_watch);
}

#[test]
fn test_relocate_span_interleaved_rows() {
    use diffuse::core::{
        parse::{RowKind, RunKind},
        wire::{OutputRow, OutputRun},
    };

    let mk_row = |kind: RowKind, old: Option<u32>, new: Option<u32>, text: &str| OutputRow {
        kind,
        mov_side: None,
        old_line: old,
        new_line: new,
        runs: vec![OutputRun::new(RunKind::Ctx, text.to_string())],
        wholesale: false,
        group: None,
    };

    let rows = [
        mk_row(RowKind::Ctx, Some(10), Some(10), "common line 10"),
        mk_row(RowKind::Del, Some(11), None, "deleted line 11"),
        mk_row(RowKind::Add, None, Some(11), "added line 11"),
        mk_row(RowKind::Ctx, Some(12), Some(12), "common line 12"),
    ];

    let row_refs: Vec<&OutputRow> = rows.iter().collect();
    let relocated = crate::notes_cli::relocate_span(&row_refs, None, Some((10, 12)))
        .expect("should relocate span spanning 10-12");

    assert_eq!(
        relocated.len(),
        4,
        "interleaved deletion row must be included in the relocation window"
    );
    assert_eq!(relocated[1].kind, RowKind::Del);
    assert_eq!(relocated[2].kind, RowKind::Add);

    // Old-side relocation also captures interleaved additions
    let relocated_old = crate::notes_cli::relocate_span(&row_refs, Some((10, 12)), None)
        .expect("should relocate old span 10-12");
    assert_eq!(relocated_old.len(), 4);

    // Both spans given
    let relocated_both = crate::notes_cli::relocate_span(&row_refs, Some((10, 12)), Some((10, 12)))
        .expect("should relocate matching both spans");
    assert_eq!(relocated_both.len(), 4);

    // Inverted span returns None
    assert!(crate::notes_cli::relocate_span(&row_refs, None, Some((12, 10))).is_none());

    // Single row
    let single = crate::notes_cli::relocate_span(&row_refs, None, Some((10, 10)))
        .expect("single row relocation");
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].new_line, Some(10));
}
