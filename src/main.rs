//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use clap::Parser;
use diffuse::{
    bin_pipeline::run_pipeline_with_notes,
    git::load_git_repo,
    notes::{NotesDB, load_notes},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    io::{self, IsTerminal, Read},
    path::Path,
};

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(
    name = "diffuse",
    version,
    about = "diff-engine token-stream & terminal renderer"
)]
struct Args {
    /// Read the diff from this file instead of stdin.
    #[arg(long)]
    diff: Option<String>,
    /// Path to notes database file (e.g. ~/.pi/review/notes.json).
    #[arg(long)]
    notes_file: Option<String>,
    /// Repository name key for notes lookup.
    #[arg(long)]
    repo: Option<String>,
    /// Emit raw token-stream JSON (for machine/extension integration).
    #[arg(long)]
    json: bool,
    /// Disable interactive TUI mode and print colored diff to terminal.
    #[arg(long)]
    no_tui: bool,
    /// Git diff arguments (e.g. `diffuse HEAD~1`, `diffuse --cached`, `diffuse
    /// path/to/file`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    git_args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    note: Option<diffuse::notes::ReviewNote>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    side: Option<diffuse::notes::Side>,
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    end: Option<u32>,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    contents: HashMap<String, String>,
    #[serde(default)]
    repo_root: Option<String>,
    #[serde(default)]
    base_args: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    context: Option<usize>,
    #[allow(dead_code)]
    #[serde(default)]
    notes: Option<NotesDB>,
    #[serde(default)]
    notes_file: Option<String>,
    #[serde(default)]
    repo: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InputPayload {
    Batch(Vec<Input>),
    Single(Box<Input>),
}

#[cfg(test)]
mod tests {
    use super::route_mode;

    #[test]
    fn extension_json_payload_is_json_mode() {
        // The pi extension pipes a JSON payload with no git args.
        assert!(route_mode(false, None, &[], false, "{\"diff\":\"x\"}"));
    }

    #[test]
    fn piped_raw_diff_stays_json_mode() {
        // `git diff | diffuse` — a raw (non-JSON) payload piped in.
        assert!(route_mode(false, None, &[], false, "diff --git a b\n"));
    }

    #[test]
    fn empty_piped_stdin_is_standalone() {
        // `diffuse < /dev/null | cat` — no payload, must route to the
        // standalone reviewer, never the JSON contract.
        assert!(!route_mode(false, None, &[], false, ""));
    }

    #[test]
    fn interactive_stdin_is_standalone() {
        assert!(!route_mode(false, None, &[], true, ""));
    }

    #[test]
    fn explicit_json_flag_is_json_mode() {
        assert!(route_mode(true, None, &[], true, ""));
    }

    #[test]
    fn diff_file_flag_is_json_mode() {
        assert!(route_mode(false, Some("x.diff"), &[], true, ""));
    }

    #[test]
    fn git_args_are_always_standalone() {
        // `diffuse HEAD < file` — git args win over piped stdin content.
        assert!(!route_mode(false, None, &["HEAD".into()], false, "noise"));
    }
}

fn process_input(input: Input, args: &Args) -> diffuse::bin_pipeline::PipelineOutput {
    let notes_file_path = args.notes_file.clone().or_else(|| input.notes_file.clone());
    let notes_path = notes_file_path
        .as_ref()
        .map(Path::new)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(diffuse::notes::default_notes_path);

    if let Some("addNote") = input.action.as_deref()
        && let (Some(n), Some(r)) = (input.note.clone(), input.repo.as_deref())
    {
        let mut db = load_notes(&notes_path);
        diffuse::notes::add_note(&mut db, r, n);
        let _ = diffuse::notes::save_notes(&notes_path, &db);
    }

    if let Some("removeNotes") = input.action.as_deref()
        && let (Some(r), Some(f), Some(s), Some(st), Some(en)) = (
            input.repo.as_deref(),
            input.file.as_deref(),
            input.side,
            input.start,
            input.end,
        )
    {
        let mut db = load_notes(&notes_path);
        diffuse::notes::remove_notes(&mut db, r, f, s, st, en);
        let _ = diffuse::notes::save_notes(&notes_path, &db);
    }

    let loaded_db = load_notes(&notes_path);
    let db_ref = Some(&loaded_db);
    let repo_ref = args.repo.as_deref().or(input.repo.as_deref());

    if let Some(ref repo_root) = input.repo_root {
        let context = input.context.unwrap_or(6);
        if let Ok(git_res) = load_git_repo(
            Path::new(repo_root),
            &input.base_args,
            &input.paths,
            input.cached,
            context,
        ) {
            let mut out = diffuse::bin_pipeline::run_pipeline_full(
                &git_res.diff_text,
                &git_res.contents,
                Some(&git_res.old_contents),
                db_ref,
                repo_ref,
            );
            out.branch = git_res.branch;
            out.untracked_skipped = git_res.untracked_skipped;
            return out;
        }
    }

    run_pipeline_with_notes(&input.diff, &input.contents, db_ref, repo_ref)
}

/// Decide between the machine/extension JSON contract (`true`) and the
/// standalone reviewer producing terminal output (`false`).
///
/// The extension pipes a JSON payload on stdin (a non-terminal stdin with
/// content). A standalone run has git arguments, an interactive stdin, or
/// an empty piped stdin (e.g. `diffuse < /dev/null | cat`). Routing on the
/// *content* of a piped stdin keeps the extension contract intact while
/// letting fully-piped invocations fall through to the full render — and
/// the TUI is only ever entered when both ends are real terminals (see
/// `main`).
fn route_mode(
    json: bool,
    diff: Option<&str>,
    git_args: &[String],
    stdin_tty: bool,
    piped_stdin: &str,
) -> bool {
    json || diff.is_some() || (git_args.is_empty() && !stdin_tty && !piped_stdin.trim().is_empty())
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let stdin_tty = std::io::stdin().is_terminal();
    let stdout_tty = std::io::stdout().is_terminal();

    // Peek a piped stdin only when the routing decision depends on its
    // content (no git args, no explicit contract flags). Never block a
    // long-running pipe for a repo review (`diffuse HEAD < <(tail -f …)`).
    let mut piped_stdin = String::new();
    if !stdin_tty && args.git_args.is_empty() && !args.json && args.diff.is_none() {
        io::stdin().read_to_string(&mut piped_stdin)?;
    }

    // Standalone reviewer: `diffuse [git-args...]` from a terminal or with
    // an empty/closed piped stdin. When either end is non-interactive
    // (piped/redirected), render the full ANSI diff instead of the TUI.
    if !route_mode(
        args.json,
        args.diff.as_deref(),
        &args.git_args,
        stdin_tty,
        &piped_stdin,
    ) {
        let repo_root = std::env::current_dir()?;
        let git_res = load_git_repo(&repo_root, &args.git_args, &[], false, 6);

        let git_res = match git_res {
            Ok(g) => g,
            Err(e) => {
                eprintln!("diffuse: {e}");
                std::process::exit(2);
            }
        };

        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(diffuse::notes::default_notes_path);
        let loaded_db = load_notes(&notes_path);
        let db_ref = Some(&loaded_db);
        let repo_ref = args.repo.as_deref();

        let mut out = diffuse::bin_pipeline::run_pipeline_full(
            &git_res.diff_text,
            &git_res.contents,
            Some(&git_res.old_contents),
            db_ref,
            repo_ref,
        );
        out.branch = git_res.branch;
        out.untracked_skipped = git_res.untracked_skipped;

        let repo_name = args.repo.clone().unwrap_or_else(|| {
            repo_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "diffuse".to_string())
        });

        // Interactive TUI only when both ends are real terminals; any
        // piped or redirected end falls back to the full ANSI render.
        if !args.no_tui && stdin_tty && stdout_tty {
            diffuse::tui::run_interactive_tui(out, repo_name)?;
        } else {
            let theme = diffuse::render::default_theme();
            let rendered = diffuse::render::render_pipeline_output(&out, &theme, true);
            print!("{rendered}");
        }
        return Ok(());
    }

    let raw = match &args.diff {
        Some(path) => fs::read_to_string(path)?,
        None => {
            if piped_stdin.is_empty() {
                let mut s = String::new();
                io::stdin().read_to_string(&mut s)?;
                s
            } else {
                piped_stdin
            }
        }
    };

    let payload: InputPayload = serde_json::from_str(&raw).unwrap_or_else(|_| {
        InputPayload::Single(Box::new(Input {
            action: None,
            note: None,
            file: None,
            side: None,
            start: None,
            end: None,
            diff: raw,
            contents: HashMap::new(),
            repo_root: None,
            base_args: Vec::new(),
            paths: Vec::new(),
            cached: false,
            context: None,
            notes: None,
            notes_file: None,
            repo: None,
        }))
    });

    match payload {
        InputPayload::Batch(inputs) => {
            let outputs: Vec<_> = inputs
                .into_iter()
                .map(|i| process_input(i, &args))
                .collect();
            println!("{}", serde_json::to_string(&outputs).expect("serialize"));
        }
        InputPayload::Single(input) => {
            let output = process_input(*input, &args);
            println!("{}", serde_json::to_string(&output).expect("serialize"));
        }
    }
    Ok(())
}
