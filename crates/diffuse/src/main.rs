//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use std::{
    collections::HashMap,
    env, fs,
    io::{self, IsTerminal, Read},
    path::Path,
};

use clap::{Parser, Subcommand};
use serde::Deserialize;

use diffuse::{
    DEFAULT_EXPAND_LINES, DEFAULT_TOKEN_PRODUCT_CAP,
    core::{
        DEFAULT_CONTEXT_LINES, ENGINE_CONTEXT, PipelineOptions,
        git::load_git_repo,
        notes::{
            NotesDB, ReviewNote, Side, add_note, default_notes_path, load_notes, remove_notes,
            save_notes,
        },
        pipeline::run_pipeline_full,
        streaming::{spawn_git_stream_pipeline, stream_diff_pipeline},
        wire::PipelineOutput,
    },
    render::{
        DiffuseTheme, MarkdownOptions, render_file_diff, render_markdown_output,
        render_raw_file_diff, render_raw_pipeline_output,
    },
    tui::run_streaming_interactive_tui,
};

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(
    name = "diffuse",
    version,
    about = "diff-engine token-stream & terminal renderer"
)]
struct Args {
    /// Change to DIR before doing anything (equivalent to git -C <DIR>).
    #[arg(short = 'C', value_name = "DIR", action = clap::ArgAction::Append, global = true)]
    directory: Vec<std::path::PathBuf>,
    /// Read the diff from this file instead of stdin.
    #[arg(long, global = true)]
    diff: Option<String>,
    /// Path to notes database file (e.g. ~/.pi/review/notes.json).
    #[arg(long, global = true)]
    notes_file: Option<String>,
    /// Repository name key for notes lookup.
    #[arg(long, global = true)]
    repo: Option<String>,
    /// Unified diff context lines (default: 4).
    #[arg(short = 'U', long, default_value_t = DEFAULT_CONTEXT_LINES, global = true)]
    context: usize,
    /// Number of context lines to expand when activating a hidden gap separator
    /// (default: 6).
    #[arg(long, default_value_t = DEFAULT_EXPAND_LINES, global = true)]
    expand_lines: usize,
    /// Maximum quadratic token-product cap for subline diffing (default:
    /// 250000).
    #[arg(long, default_value_t = DEFAULT_TOKEN_PRODUCT_CAP, global = true)]
    token_product_cap: usize,
    /// Emit raw token-stream JSON (for machine/extension integration).
    #[arg(long, global = true)]
    json: bool,
    /// Emit raw token-stream JSON Lines (one JSON object per file as it is
    /// processed).
    #[arg(long, global = true)]
    jsonl: bool,
    /// Emit diff in GitHub-Flavored Markdown (GFM) format.
    #[arg(short = 'm', long, global = true)]
    markdown: bool,
    /// Control ANSI colorization ('always', 'never', or 'auto').
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    color: String,
    /// Disable ANSI colorization (equivalent to --color=never).
    #[arg(long, global = true)]
    no_color: bool,
    /// Color theme (`default`, `dark`, `black`). Overrides `DIFFUSE_THEME`.
    #[arg(long, value_name = "NAME", global = true)]
    theme: Option<String>,
    /// Run in non-interactive pager mode (print colored diff to terminal).
    #[arg(short = 'p', long, global = true)]
    pager: bool,
    /// Disable interactive TUI mode (alias for --pager).
    #[arg(long, global = true)]
    no_tui: bool,
    /// Emit raw colorized diff (flat green/red, no subword highlighting).
    /// Equivalent to the TUI's R (raw diff) toggle.
    #[arg(long, global = true)]
    raw: bool,

    #[command(subcommand)]
    command: Option<SubCommand>,

    /// Git diff arguments (e.g. `diffuse HEAD~1`, `diffuse --cached`, `diffuse
    /// path/to/file`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    git_args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// Interactive diff reviewer (default when invoked in a repository).
    Review {
        /// Git diff arguments (e.g. `HEAD~1`, `--cached`, `src/`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        git_args: Vec<String>,
    },
    /// Parse a diff and emit structured token-stream JSON, JSON Lines, or
    /// Markdown.
    Parse {
        /// Read raw diff or JSON payload from file instead of stdin.
        #[arg(long)]
        diff: Option<String>,
        /// Emit single JSON document (default).
        #[arg(long)]
        json: bool,
        /// Emit JSON Lines.
        #[arg(long)]
        jsonl: bool,
        /// Emit GitHub-Flavored Markdown.
        #[arg(short = 'm', long)]
        markdown: bool,
        /// Emit raw colorized diff (flat green/red, no subword highlighting).
        #[arg(long)]
        raw: bool,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    note: Option<ReviewNote>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    side: Option<Side>,
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    end: Option<u32>,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    new_content: Option<HashMap<String, String>>,
    #[serde(default)]
    old_content: Option<HashMap<String, String>>,
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
    #[serde(default)]
    token_product_cap: Option<usize>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InputPayload {
    Batch(Vec<Input>),
    Single(Box<Input>),
}

fn process_input(input: Input, args: &Args) -> PipelineOutput {
    let notes_file_path = args.notes_file.clone().or_else(|| input.notes_file.clone());
    let notes_path = notes_file_path
        .as_ref()
        .map(Path::new)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_notes_path);

    if let Some("addNote") = input.action.as_deref()
        && let (Some(n), Some(r)) = (input.note.clone(), input.repo.as_deref())
    {
        let mut db = load_notes(&notes_path);
        add_note(&mut db, r, n);
        let _ = save_notes(&notes_path, &db);
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
        remove_notes(&mut db, r, f, s, st, en);
        let _ = save_notes(&notes_path, &db);
    }

    let loaded_db = load_notes(&notes_path);
    let repo_str = args.repo.clone().or_else(|| input.repo.clone());
    let visual = input.context.unwrap_or(args.context);
    let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
    let opts = PipelineOptions::default()
        .with_notes(Some(loaded_db.clone()), repo_str.clone())
        .with_visual_context(visual)
        .with_expand_lines(args.expand_lines)
        .with_token_product_cap(cap);

    if let Some(ref repo_root) = input.repo_root
        && let Ok(git_res) = load_git_repo(
            Path::new(repo_root),
            &input.base_args,
            &input.paths,
            input.cached,
            ENGINE_CONTEXT,
        )
    {
        let mut out = run_pipeline_full(
            &git_res.diff,
            Some(git_res.new_content),
            Some(git_res.old_content),
            opts,
        );
        out.branch = git_res.branch;
        out.untracked_skipped = git_res.untracked_skipped;
        return out;
    }

    run_pipeline_full(&input.diff, input.new_content, input.old_content, opts)
}

/// Decide between the machine/extension JSON contract (`true`) and the
/// standalone reviewer producing terminal output (`false`).
///
/// The extension pipes a JSON payload on stdin (a non-terminal stdin with
/// content). A standalone run has git arguments, an interactive stdin, or
/// an empty piped stdin (e.g. `diffuse < /dev/null | cat`). Routing on the
use std::io::Write;

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
    jsonl: bool,
    diff: Option<&str>,
    git_args: &[String],
    stdin_tty: bool,
    piped_stdin: &str,
) -> bool {
    json || jsonl
        || diff.is_some()
        || (git_args.is_empty() && !stdin_tty && !piped_stdin.trim().is_empty())
}

/// Resolve the active [`DiffuseTheme`] from the `--theme` CLI flag,
/// falling back to the `DIFFUSE_THEME` environment variable and then the
/// default theme. An unknown `--theme` name warns on stderr and falls back.
pub fn resolve_theme(cli_theme: Option<&str>) -> DiffuseTheme {
    if let Some(name) = cli_theme {
        if let Some(theme) = DiffuseTheme::from_name(name) {
            return theme;
        }
        eprintln!("diffuse: unknown theme '{name}', falling back to default theme");
    }
    DiffuseTheme::from_env()
}

/// Configure global ANSI styling condition via `yansi` from CLI flags and
/// environment.
pub fn init_color_mode(color: &str, no_color: bool) -> bool {
    let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if no_color || color == "never" || (color == "auto" && no_color_env) {
        yansi::disable();
        false
    } else {
        yansi::enable();
        true
    }
}

fn main() -> io::Result<()> {
    let _ = diffuse::core::logging::init_tracing_subscriber();
    let args = Args::parse();

    for dir in &args.directory {
        if let Err(e) = env::set_current_dir(dir) {
            eprintln!(
                "diffuse: cannot change directory to '{}': {e}",
                dir.display()
            );
            std::process::exit(1);
        }
    }

    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let colorize = init_color_mode(&args.color, args.no_color);
    let theme = resolve_theme(args.theme.as_deref());

    let (explicit_cmd, git_args, is_json, is_jsonl, is_markdown, is_raw, diff_opt) =
        match &args.command {
            Some(SubCommand::Review {
                git_args: sub_git_args,
            }) => {
                let mut ga = args.git_args.clone();
                ga.extend(sub_git_args.iter().cloned());
                (
                    Some("review"),
                    ga,
                    args.json,
                    args.jsonl,
                    args.markdown,
                    args.raw,
                    args.diff.clone(),
                )
            }
            Some(SubCommand::Parse {
                diff,
                json,
                jsonl,
                markdown,
                raw,
            }) => {
                let md = *markdown || args.markdown;
                let rw = *raw || args.raw;
                let j = if !*json && !*jsonl && !md && !rw {
                    true
                } else {
                    *json || args.json
                };
                let jl = *jsonl || args.jsonl;
                (
                    Some("parse"),
                    Vec::new(),
                    j,
                    jl,
                    md,
                    rw,
                    diff.clone().or_else(|| args.diff.clone()),
                )
            }
            None => (
                None,
                args.git_args.clone(),
                args.json,
                args.jsonl,
                args.markdown,
                args.raw,
                args.diff.clone(),
            ),
        };

    let no_tui = args.no_tui || args.pager || is_markdown || is_raw;

    // Peek a piped stdin only when the routing decision depends on its
    // content (no git args, no explicit contract flags, no explicit
    // subcommand).
    let mut piped_stdin = String::new();
    if explicit_cmd.is_none()
        && !stdin_tty
        && git_args.is_empty()
        && !is_json
        && !is_jsonl
        && diff_opt.is_none()
    {
        io::stdin().read_to_string(&mut piped_stdin)?;
    }

    let is_json_contract = match explicit_cmd {
        Some("parse") => true,
        Some("review") => is_json || is_jsonl,
        _ => route_mode(
            is_json,
            is_jsonl,
            diff_opt.as_deref(),
            &git_args,
            stdin_tty,
            &piped_stdin,
        ),
    };

    // Standalone reviewer: `diffuse [git-args...]` or `diffuse review
    // [git-args...]` When either end is non-interactive or
    // --pager/--no-tui/--markdown is requested, render the full ANSI diff or
    // GFM Markdown instead of the TUI.
    if !is_json_contract {
        let repo_root = env::current_dir()?;
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);

        let repo_name = args.repo.clone().unwrap_or_else(|| {
            repo_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "diffuse".to_string())
        });

        let pipeline_opts = PipelineOptions::default()
            .with_notes(Some(loaded_db.clone()), args.repo.clone())
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap);

        // Interactive TUI only when both ends are real terminals and not in
        // pager or markdown mode; any piped or redirected end falls
        // back to the full ANSI or Markdown render.
        if !no_tui && stdin_tty && stdout_tty {
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_args.clone(),
                Vec::new(),
                false,
                ENGINE_CONTEXT,
                pipeline_opts,
                None,
            );
            let initial_out = PipelineOutput {
                files: Vec::new(),
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            run_streaming_interactive_tui(
                initial_out,
                Some(consumer.into_receiver()),
                Some(meta_rx),
                repo_name,
                notes_path,
                repo_root.clone(),
                git_args.clone(),
                args.expand_lines,
                theme,
            )?;
        } else if is_markdown {
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_args.clone(),
                Vec::new(),
                false,
                ENGINE_CONTEXT,
                pipeline_opts,
                None,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                std::process::exit(2);
            }
            let files: Vec<_> = consumer.collect();
            let out = PipelineOutput {
                files,
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            let md_colorize = if colorize {
                if args.color == "always" {
                    true
                } else {
                    stdout_tty
                }
            } else {
                false
            };
            let md_opts = MarkdownOptions {
                colorize: md_colorize,
                theme,
                ..MarkdownOptions::default()
            };
            let md = render_markdown_output(&out, &md_opts);
            let mut stdout = io::stdout().lock();
            if let Err(e) = stdout.write_all(md.as_bytes()) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(e);
            }
            let _ = stdout.flush();
        } else {
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_args.clone(),
                Vec::new(),
                false,
                ENGINE_CONTEXT,
                pipeline_opts,
                None,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                std::process::exit(2);
            }
            let mut stdout = io::stdout().lock();
            if is_raw {
                for file in consumer {
                    let mut rendered = render_raw_file_diff(&file);
                    if !colorize {
                        rendered = diffuse::render::ansi::strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
            } else {
                for file in consumer {
                    let mut rendered = render_file_diff(&file, &theme, true);
                    if !colorize {
                        rendered = diffuse::render::ansi::strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
            }
            if colorize {
                let _ = stdout.write_all(diffuse::render::ansi::SGR_RESET.as_bytes());
            }
            let _ = stdout.flush();
        }
        return Ok(());
    }

    if diff_opt.is_none() && stdin_tty && piped_stdin.is_empty() {
        let repo_root = env::current_dir()?;
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);
        let pipeline_opts = PipelineOptions::default()
            .with_notes(Some(loaded_db), args.repo.clone())
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap);

        let (consumer, meta_rx) = spawn_git_stream_pipeline(
            repo_root,
            git_args,
            Vec::new(),
            false,
            ENGINE_CONTEXT,
            pipeline_opts,
            None,
        );
        if let Ok(Err(e)) = meta_rx.recv() {
            eprintln!("diffuse: {e}");
            std::process::exit(2);
        }
        if is_raw {
            let mut stdout = io::stdout().lock();
            for file in consumer {
                let rendered = render_raw_file_diff(&file);
                if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
            }
            let _ = stdout.write_all(diffuse::render::ansi::SGR_RESET.as_bytes());
            let _ = stdout.flush();
        } else if is_jsonl {
            let mut stdout = io::stdout().lock();
            for file in consumer {
                if let Err(e) = diffuse::core::wire::jsonl::emit_file_jsonl(&file, &mut stdout) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
            }
            let _ = stdout.flush();
        } else {
            let files: Vec<_> = consumer.collect();
            let out = PipelineOutput {
                files,
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            println!("{}", serde_json::to_string(&out).expect("serialize"));
        }
        return Ok(());
    }

    let raw = match &diff_opt {
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
            new_content: None,
            old_content: None,
            repo_root: None,
            base_args: Vec::new(),
            paths: Vec::new(),
            cached: false,
            context: None,
            notes: None,
            notes_file: None,
            repo: None,
            token_product_cap: None,
        }))
    });

    let md_colorize = if colorize {
        if args.color == "always" {
            true
        } else {
            stdout_tty
        }
    } else {
        false
    };
    let md_opts = MarkdownOptions {
        colorize: md_colorize,
        theme,
        ..MarkdownOptions::default()
    };

    match payload {
        InputPayload::Batch(inputs) => {
            let outputs: Vec<_> = inputs
                .into_iter()
                .map(|i| process_input(i, &args))
                .collect();
            if is_markdown {
                let mut stdout = io::stdout().lock();
                for out in &outputs {
                    let md = render_markdown_output(out, &md_opts);
                    if let Err(e) = stdout.write_all(md.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else if is_raw {
                let mut stdout = io::stdout().lock();
                for out in &outputs {
                    let mut rendered = render_raw_pipeline_output(out);
                    if !colorize {
                        rendered = diffuse::render::ansi::strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else if is_jsonl {
                let mut stdout = io::stdout().lock();
                for out in &outputs {
                    for file in &out.files {
                        if let Err(e) =
                            diffuse::core::wire::jsonl::emit_file_jsonl(file, &mut stdout)
                        {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                return Ok(());
                            }
                            return Err(e);
                        }
                    }
                }
                let _ = stdout.flush();
            } else {
                println!("{}", serde_json::to_string(&outputs).expect("serialize"));
            }
        }
        InputPayload::Single(input) => {
            if is_markdown {
                let output = process_input(*input, &args);
                let md = render_markdown_output(&output, &md_opts);
                let mut stdout = io::stdout().lock();
                if let Err(e) = stdout.write_all(md.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
                let _ = stdout.flush();
            } else if is_raw {
                let output = process_input(*input, &args);
                let mut rendered = render_raw_pipeline_output(&output);
                if !colorize {
                    rendered = diffuse::render::ansi::strip_ansi(&rendered).to_string();
                }
                let mut stdout = io::stdout().lock();
                if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
                let _ = stdout.flush();
            } else if is_jsonl {
                let notes_file_path = args.notes_file.clone().or_else(|| input.notes_file.clone());
                let notes_path = notes_file_path
                    .as_ref()
                    .map(Path::new)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(default_notes_path);
                let loaded_db = load_notes(&notes_path);
                let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
                let opts = PipelineOptions::default()
                    .with_notes(Some(loaded_db), args.repo.clone().or(input.repo.clone()))
                    .with_visual_context(input.context.unwrap_or(args.context))
                    .with_expand_lines(args.expand_lines)
                    .with_token_product_cap(cap);
                let consumer = stream_diff_pipeline(
                    &input.diff,
                    input.new_content,
                    input.old_content,
                    opts,
                    None,
                );
                let mut stdout = io::stdout().lock();
                for file in consumer {
                    if let Err(e) = diffuse::core::wire::jsonl::emit_file_jsonl(&file, &mut stdout)
                    {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else {
                let output = process_input(*input, &args);
                println!("{}", serde_json::to_string(&output).expect("serialize"));
            }
        }
    }
    Ok(())
}

#[path = "main_tests.rs"]
#[cfg(test)]
mod tests;
