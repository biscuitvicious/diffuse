//! Command routing and execution dispatch across TUI, pager, markdown, direct
//! path diffing, and streaming diff modes.

use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process,
};

use diffuse::{
    git::{diff_paths, is_inside_work_tree},
    notes::{default_notes_path, load_notes},
    prelude::*,
    streaming::{self, spawn_git_stream_pipeline, stream_diff_reader, stream_parsed_files},
    wire::emit_file_jsonl,
};

use super::{
    args::{Args, SubCommand},
    theme::{init_color_mode, resolve_theme},
};
use crate::notes_cli;

/// Decide whether an invocation routes to non-interactive diff stream parsing
/// (`true`) or to the git review/direct reviewer (`false`).
pub fn route_mode(
    is_json: bool,
    is_jsonl: bool,
    diff_opt: Option<&str>,
    git_revs: &[String],
    stdin_tty: bool,
    has_piped_stdin: bool,
) -> bool {
    if is_json || is_jsonl || diff_opt.is_some() {
        return true;
    }
    if !git_revs.is_empty() || stdin_tty {
        return false;
    }
    has_piped_stdin
}

/// Returns true if an interactive terminal session can be driven via stdin or
/// the controlling terminal (`/dev/tty`).
pub fn is_terminal_interactive(stdin_tty: bool) -> bool {
    if stdin_tty {
        return true;
    }
    #[cfg(unix)]
    {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .is_ok()
    }
    #[cfg(not(unix))]
    {
        false
    }
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

    let no_tui = args.no_tui || args.pager || is_markdown || is_raw || is_json || is_jsonl;

    // Direct path diffing: `diffuse <file1> <file2>` or `diffuse <dir1> <dir2>`.
    if explicit_cmd.is_none() && git_revs.len() == 2 {
        let p1 = Path::new(&git_revs[0]);
        let p2 = Path::new(&git_revs[1]);
        if p1.exists() && p2.exists() {
            return dispatch_path_diff(
                args,
                p1,
                p2,
                is_json,
                is_jsonl,
                is_markdown,
                is_raw,
                show_resolved,
                show_source,
                no_tui,
                stdin_tty,
                stdout_tty,
                colorize,
            );
        }
    }

    let (has_piped_stdin, reader) = if explicit_cmd.is_none()
        && !stdin_tty
        && git_revs.is_empty()
        && !is_json
        && !is_jsonl
        && diff_opt.is_none()
    {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        match io::stdin().read(&mut buf) {
            Ok(n) if n > 0 && !buf[..n].trim_ascii().is_empty() => {
                let chained =
                    io::BufReader::new(io::Cursor::new(buf[..n].to_vec()).chain(io::stdin()));
                (
                    true,
                    Some(Box::new(chained) as Box<dyn io::BufRead + Send + 'static>),
                )
            }
            _ => (false, None),
        }
    } else {
        (false, None)
    };

    let is_diff_stream = match explicit_cmd {
        Some("parse") => true,
        Some("review") => is_json || is_jsonl,
        _ => route_mode(
            is_json,
            is_jsonl,
            diff_opt.as_deref(),
            &git_revs,
            stdin_tty,
            has_piped_stdin,
        ),
    };

    if is_diff_stream {
        return dispatch_diff_stream(
            args,
            reader,
            diff_opt.as_deref(),
            is_json,
            is_jsonl,
            is_markdown,
            is_raw,
            show_resolved,
            show_source,
            no_tui,
            stdin_tty,
            stdout_tty,
            colorize,
        );
    }

    dispatch_git_review(
        args,
        &git_revs,
        &pathspecs,
        is_json,
        is_jsonl,
        is_markdown,
        is_raw,
        show_resolved,
        show_source,
        no_tui,
        stdin_tty,
        stdout_tty,
        colorize,
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_path_diff(
    args: &Args,
    p1: &Path,
    p2: &Path,
    is_json: bool,
    is_jsonl: bool,
    is_markdown: bool,
    is_raw: bool,
    show_resolved: bool,
    show_source: bool,
    no_tui: bool,
    stdin_tty: bool,
    stdout_tty: bool,
    colorize: bool,
) -> io::Result<()> {
    let res = diff_paths(
        p1,
        p2,
        args.context as u32,
        diffuse::git::WhitespaceMode::TrimEquality,
    )?;

    let explicit_notes_path = args.notes_file.as_ref();
    let notes_path = explicit_notes_path
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_path);
    let loaded_db = load_notes(&notes_path);

    let display_title = args
        .repo_key
        .clone()
        .unwrap_or_else(|| format!("{} ↔ {}", p1.display(), p2.display()));
    let target = ReviewTarget::from_repo(Repo::from_key(display_title))
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

    let consumer = stream_parsed_files(
        res.files,
        Some(res.new_content),
        Some(res.old_content),
        pipeline_opts,
        args.jobs,
    );

    if !no_tui && stdout_tty && is_terminal_interactive(stdin_tty) {
        let theme = resolve_theme(args.theme.as_deref(), stdin_tty);
        let initial_out = PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        run_tui(
            initial_out,
            Some(consumer.into_receiver()),
            None,
            target,
            notes_path,
            args.expand_lines,
            theme,
            show_resolved,
            show_source,
            false,
        )?;
        return Ok(());
    }

    consume_and_render(
        consumer,
        args,
        is_json,
        is_jsonl,
        is_markdown,
        is_raw,
        show_resolved,
        show_source,
        stdout_tty,
        colorize,
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_diff_stream(
    args: &Args,
    reader: Option<Box<dyn io::BufRead + Send + 'static>>,
    diff_opt: Option<&str>,
    is_json: bool,
    is_jsonl: bool,
    is_markdown: bool,
    is_raw: bool,
    show_resolved: bool,
    show_source: bool,
    no_tui: bool,
    stdin_tty: bool,
    stdout_tty: bool,
    colorize: bool,
) -> io::Result<()> {
    let explicit_notes_path = args.notes_file.as_ref();
    let notes_path = explicit_notes_path
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_path);
    let loaded_db = load_notes(&notes_path);

    let display_title = match (args.repo_key.as_deref(), diff_opt) {
        (Some(key), _) => key,
        (None, Some(path)) => path,
        (None, None) => "<stdin>",
    };
    let target = ReviewTarget::from_repo(Repo::from_key(display_title))
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

    let consumer = if let Some(path) = diff_opt {
        let file = fs::File::open(path)?;
        stream_diff_reader(
            io::BufReader::new(file),
            pipeline_opts,
            args.jobs,
            args.old_dir.clone(),
            args.new_dir.clone(),
        )
    } else if let Some(r) = reader {
        stream_diff_reader(
            r,
            pipeline_opts,
            args.jobs,
            args.old_dir.clone(),
            args.new_dir.clone(),
        )
    } else {
        stream_diff_reader(
            io::BufReader::new(io::stdin()),
            pipeline_opts,
            args.jobs,
            args.old_dir.clone(),
            args.new_dir.clone(),
        )
    };

    if !no_tui && stdout_tty && is_terminal_interactive(stdin_tty) {
        let theme = resolve_theme(args.theme.as_deref(), stdin_tty);
        let initial_out = PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        run_tui(
            initial_out,
            Some(consumer.into_receiver()),
            None,
            target,
            notes_path,
            args.expand_lines,
            theme,
            show_resolved,
            show_source,
            false,
        )?;
        return Ok(());
    }

    consume_and_render(
        consumer,
        args,
        is_json,
        is_jsonl,
        is_markdown,
        is_raw,
        show_resolved,
        show_source,
        stdout_tty,
        colorize,
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_git_review(
    args: &Args,
    git_revs: &[String],
    pathspecs: &[String],
    is_json: bool,
    is_jsonl: bool,
    is_markdown: bool,
    is_raw: bool,
    show_resolved: bool,
    show_source: bool,
    no_tui: bool,
    stdin_tty: bool,
    stdout_tty: bool,
    colorize: bool,
) -> io::Result<()> {
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
        git_revs.to_vec(),
        pathspecs.to_vec(),
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

    let staged = args.staged;

    if !no_tui && stdout_tty && is_terminal_interactive(stdin_tty) {
        let theme = resolve_theme(args.theme.as_deref(), stdin_tty);
        let (consumer, meta_rx) = spawn_git_stream_pipeline(
            repo_root,
            git_revs.to_vec(),
            pathspecs.to_vec(),
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
        return Ok(());
    }

    let theme = resolve_theme(args.theme.as_deref(), false);
    let (consumer, meta_rx) = spawn_git_stream_pipeline(
        repo_root,
        git_revs.to_vec(),
        pathspecs.to_vec(),
        staged,
        ENGINE_CONTEXT,
        pipeline_opts,
        args.jobs,
    );
    if let Ok(Err(e)) = meta_rx.recv() {
        eprintln!("diffuse: {e}");
        process::exit(2);
    }

    if is_raw {
        let files: Vec<_> = consumer.collect();
        let out = PipelineOutput {
            files,
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        let term_width = detect_terminal_width();
        let mut rendered = render_raw_pipeline_output_with_width(&out, &theme, term_width);
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
    } else if is_markdown {
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
    } else if is_json {
        let files: Vec<_> = consumer.collect();
        let out = PipelineOutput {
            files,
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        println!("{}", serde_json::to_string(&out).expect("serialize"));
    } else {
        let mut stdout = io::stdout().lock();
        let term_width = detect_terminal_width();
        for file in consumer {
            let mut rendered = render_file_diff_with_width(&file, None, &theme, true, term_width);
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
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn consume_and_render(
    consumer: streaming::StreamingConsumer,
    args: &Args,
    is_json: bool,
    is_jsonl: bool,
    is_markdown: bool,
    is_raw: bool,
    show_resolved: bool,
    show_source: bool,
    stdout_tty: bool,
    colorize: bool,
) -> io::Result<()> {
    let theme = resolve_theme(args.theme.as_deref(), false);

    if is_markdown {
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
        let files: Vec<_> = consumer.collect();
        let out = PipelineOutput {
            files,
            branch: String::new(),
            untracked_skipped: Vec::new(),
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
    } else if is_raw {
        let files: Vec<_> = consumer.collect();
        let out = PipelineOutput {
            files,
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        let term_width = detect_terminal_width();
        let mut rendered = render_raw_pipeline_output_with_width(&out, &theme, term_width);
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
    } else if is_json {
        let files: Vec<_> = consumer.collect();
        let out = PipelineOutput {
            files,
            branch: String::new(),
            untracked_skipped: Vec::new(),
        };
        println!("{}", serde_json::to_string(&out).expect("serialize"));
    } else {
        let mut stdout = io::stdout().lock();
        let term_width = detect_terminal_width();
        for file in consumer {
            let mut rendered = render_file_diff_with_width(&file, None, &theme, true, term_width);
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
    }

    Ok(())
}
