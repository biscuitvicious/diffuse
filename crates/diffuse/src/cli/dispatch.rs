//! Command routing and execution dispatch across TUI, pager, markdown, and JSON
//! contract modes.

use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process,
};

use diffuse::{
    git::is_inside_work_tree,
    notes::{default_notes_path, load_notes},
    prelude::*,
    streaming::{self, spawn_git_stream_pipeline, stream_diff_pipeline},
    wire::emit_file_jsonl,
};

use super::{
    args::{Args, SubCommand},
    input::{Input, InputPayload, process_input, target_for_input},
    theme::{init_color_mode, resolve_theme},
};
use crate::notes_cli;

/// Decide between the machine/extension JSON contract (`true`) and the
/// standalone reviewer producing terminal output (`false`).
///
/// The extension pipes a JSON payload on stdin (a non-terminal stdin with
/// content). A standalone run has git arguments, an interactive stdin, or
/// an empty piped stdin (e.g. `diffuse < /dev/null | cat`). Routing on the
/// *content* of a piped stdin keeps the extension contract intact while
/// letting fully-piped invocations fall through to the full render — and
/// the TUI is only ever entered when both ends are real terminals.
pub fn route_mode(
    is_json: bool,
    is_jsonl: bool,
    diff_opt: Option<&str>,
    git_revs: &[String],
    stdin_tty: bool,
    piped_stdin: &str,
) -> bool {
    if is_json || is_jsonl || diff_opt.is_some() {
        return true;
    }
    if !git_revs.is_empty() || stdin_tty {
        return false;
    }
    !piped_stdin.trim().is_empty()
}

pub fn dispatch(args: &Args) -> io::Result<()> {
    if let Some(SubCommand::Notes(ref notes_args)) = args.command {
        return notes_cli::run_notes_command(notes_args, args);
    }

    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let colorize = init_color_mode(&args.color, args.no_color);

    let workers = args
        .jobs
        .unwrap_or_else(streaming::default_worker_count)
        .max(1);
    streaming::init_thread_pool(workers);

    let (explicit_cmd, git_revs, pathspecs, is_json, is_jsonl, is_markdown, is_raw, diff_opt) =
        match &args.command {
            Some(SubCommand::Review {
                revision: sub_revision,
            }) => {
                let mut ga = args.revision.git_revs.clone();
                ga.extend(sub_revision.git_revs.iter().cloned());
                let mut ps = args.revision.pathspecs.clone();
                ps.extend(sub_revision.pathspecs.iter().cloned());
                (
                    Some("review"),
                    ga,
                    ps,
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
                resolved: _,
                all: _,
                show_source: _,
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
                args.revision.git_revs.clone(),
                args.revision.pathspecs.clone(),
                args.json,
                args.jsonl,
                args.markdown,
                args.raw,
                args.diff.clone(),
            ),
            Some(SubCommand::Notes(_)) => unreachable!(),
        };

    let show_resolved = match &args.command {
        Some(SubCommand::Parse { resolved, all, .. }) => {
            *resolved || *all || args.resolved || args.all
        }
        _ => args.resolved || args.all,
    };
    let show_source = match &args.command {
        Some(SubCommand::Parse { show_source, .. }) => *show_source || args.show_source,
        _ => args.show_source,
    };

    let no_tui = args.no_tui || args.pager || is_markdown || is_raw;

    // Peek a piped stdin only when the routing decision depends on its
    // content (no git args, no explicit contract flags, no explicit
    // subcommand).
    let mut piped_stdin = String::new();
    if explicit_cmd.is_none()
        && !stdin_tty
        && git_revs.is_empty()
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
            &git_revs,
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
        if !is_inside_work_tree(&repo_root) {
            eprintln!(
                "diffuse: not a git repository (or any of the parent directories): {}",
                repo_root.display()
            );
            process::exit(2);
        }
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);

        let target = ReviewTarget::from_parts(
            Repo::from_root(&repo_root).with_key_opt(args.repo_key.clone()),
            git_revs.clone(),
            pathspecs.clone(),
            args.staged,
        )
        .with_rev_key_opt(args.rev_key.clone());

        let pipeline_opts = PipelineOptions::default()
            .with_notes(
                Some(loaded_db.clone()),
                target.repo_key().map(ToString::to_string),
                target.rev_key().map(ToString::to_string),
            )
            .with_show_resolved(show_resolved)
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap)
            .with_cluster_product_cap(args.cluster_budget)
            .with_cluster_bandwidth_min(args.cluster_bandwidth_min)
            .with_cluster_bandwidth_slack(args.cluster_bandwidth_slack);

        let staged = args.staged;

        // Interactive TUI only when both ends are real terminals and not in
        // pager or markdown mode; any piped or redirected end falls
        // back to the full ANSI or Markdown render.
        if !no_tui && stdin_tty && stdout_tty {
            // Theme resolves here — TUI takeover is now certain, so `auto`
            // may issue the OSC 11 background query: the app owns the
            // terminal (raw mode, full stdin consumption) for its whole
            // lifetime, and any late response lands in its own event loop,
            // never in the shell. Every other branch below resolves
            // query-free.
            let theme = resolve_theme(args.theme.as_deref(), true);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                pathspecs.clone(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts
                    .with_lazy_collapsed(true)
                    .with_show_resolved(true),
                args.jobs,
            );
            let initial_out = PipelineOutput {
                files: Vec::new(),
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            run_tui(
                initial_out,
                Some(consumer.into_receiver()),
                Some(meta_rx),
                target,
                notes_path,
                args.expand_lines,
                theme,
                show_resolved,
                show_source,
                !args.no_watch,
            )?;
        } else if is_markdown {
            // One-shot render: resolve query-free so no terminal response
            // can outrun process exit and leak into the shell.
            let theme = resolve_theme(args.theme.as_deref(), false);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                Vec::new(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts,
                args.jobs,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                process::exit(2);
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
                show_resolved_notes: show_resolved,
                show_source,
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
            // One-shot render: resolve query-free (see markdown branch).
            let theme = resolve_theme(args.theme.as_deref(), false);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                Vec::new(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts,
                args.jobs,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                process::exit(2);
            }
            let mut stdout = io::stdout().lock();
            let term_width = detect_terminal_width();
            if is_raw {
                for file in consumer {
                    let mut rendered =
                        render_raw_file_diff_with_width(&file, None, &theme, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
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
                    let mut rendered =
                        render_file_diff_with_width(&file, None, &theme, true, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
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
                let _ = stdout.write_all(SGR_RESET.as_bytes());
            }
            let _ = stdout.flush();
        }
        return Ok(());
    }

    if diff_opt.is_none() && stdin_tty && piped_stdin.is_empty() {
        let repo_root = env::current_dir()?;
        if !is_inside_work_tree(&repo_root) {
            eprintln!(
                "diffuse: not a git repository (or any of the parent directories): {}",
                repo_root.display()
            );
            process::exit(2);
        }
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);
        let target = ReviewTarget::from_repo_args(
            Repo::from_root(&repo_root).with_key_opt(args.repo_key.clone()),
            git_revs.clone(),
            args.staged,
        )
        .with_rev_key_opt(args.rev_key.clone());
        let pipeline_opts = PipelineOptions::default()
            .with_notes(
                Some(loaded_db),
                target.repo_key().map(ToString::to_string),
                target.rev_key().map(ToString::to_string),
            )
            .with_show_resolved(show_resolved)
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap)
            .with_cluster_product_cap(args.cluster_budget)
            .with_cluster_bandwidth_min(args.cluster_bandwidth_min)
            .with_cluster_bandwidth_slack(args.cluster_bandwidth_slack);

        // One-shot path (raw/jsonl/json render, no TUI): resolve query-free
        // so no terminal response can outrun process exit.
        let theme = resolve_theme(args.theme.as_deref(), false);
        let (consumer, meta_rx) = spawn_git_stream_pipeline(
            repo_root,
            git_revs,
            Vec::new(),
            args.staged,
            ENGINE_CONTEXT,
            pipeline_opts,
            args.jobs,
        );
        if let Ok(Err(e)) = meta_rx.recv() {
            eprintln!("diffuse: {e}");
            process::exit(2);
        }
        if is_raw {
            let mut stdout = io::stdout().lock();
            let term_width = detect_terminal_width();
            for file in consumer {
                let rendered = render_raw_file_diff_with_width(&file, None, &theme, term_width);
                if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
            }
            let _ = stdout.write_all(SGR_RESET.as_bytes());
            let _ = stdout.flush();
        } else if is_jsonl {
            let mut stdout = io::stdout().lock();
            for file in consumer {
                if let Err(e) = emit_file_jsonl(&file, &mut stdout) {
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

    // JSON-contract tail path: one-shot by construction, resolve query-free.
    let theme = resolve_theme(args.theme.as_deref(), false);

    let payload: InputPayload = serde_json::from_str(&raw).unwrap_or_else(|_| {
        InputPayload::Single(Box::new(Input {
            diff: raw,
            ..Default::default()
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
        show_resolved_notes: show_resolved,
        show_source,
        ..MarkdownOptions::default()
    };

    match payload {
        InputPayload::Batch(inputs) => {
            let outputs: Vec<_> = inputs.into_iter().map(|i| process_input(i, args)).collect();
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
                let term_width = detect_terminal_width();
                for out in &outputs {
                    let mut rendered =
                        render_raw_pipeline_output_with_width(out, &theme, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
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
                        if let Err(e) = emit_file_jsonl(file, &mut stdout) {
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
                let output = process_input(*input, args);
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
                let output = process_input(*input, args);
                let term_width = detect_terminal_width();
                let mut rendered =
                    render_raw_pipeline_output_with_width(&output, &theme, term_width);
                if !colorize {
                    rendered = strip_ansi(&rendered).to_string();
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
                let explicit_notes_path = args.notes_file.as_ref().or(input.notes_file.as_ref());
                let notes_path = match (explicit_notes_path, input.repo_root.as_deref()) {
                    (Some(p), _) => Some(PathBuf::from(p)),
                    (None, Some(_)) => Some(default_notes_path()),
                    (None, None) => None,
                };
                let loaded_db = notes_path.as_ref().map(|p| load_notes(p));
                let target = target_for_input(&input, args);
                let target_repo_key = target.repo_key().map(ToString::to_string);
                let target_rev_key = target.rev_key().map(ToString::to_string);
                let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
                let budget = input.cluster_budget.unwrap_or(args.cluster_budget);
                let bw_min = input
                    .cluster_bandwidth_min
                    .unwrap_or(args.cluster_bandwidth_min);
                let bw_slack = input
                    .cluster_bandwidth_slack
                    .unwrap_or(args.cluster_bandwidth_slack);
                let opts = PipelineOptions::default()
                    .with_notes(loaded_db, target_repo_key, target_rev_key)
                    .with_show_resolved(show_resolved)
                    .with_visual_context(input.context.unwrap_or(args.context))
                    .with_expand_lines(args.expand_lines)
                    .with_token_product_cap(cap)
                    .with_cluster_product_cap(budget)
                    .with_cluster_bandwidth_min(bw_min)
                    .with_cluster_bandwidth_slack(bw_slack);
                let consumer = stream_diff_pipeline(
                    &input.diff,
                    input.new_content,
                    input.old_content,
                    opts,
                    args.jobs,
                );
                let mut stdout = io::stdout().lock();
                for file in consumer {
                    if let Err(e) = emit_file_jsonl(&file, &mut stdout) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else {
                let output = process_input(*input, args);
                println!("{}", serde_json::to_string(&output).expect("serialize"));
            }
        }
    }
    Ok(())
}
