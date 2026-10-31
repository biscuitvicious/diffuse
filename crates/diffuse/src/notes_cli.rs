//! CLI handlers for the `diffuse notes` subcommand suite.

use std::{
    env,
    io::{self, IsTerminal, Read},
    path::PathBuf,
    process,
};

use diffuse::{
    git::{ENGINE_CONTEXT, is_inside_work_tree, load_git_repo_with_notes},
    notes::{
        DiffCoord, LineHash, NoteLocation, find_note_in_db, generate_note_id, now_millis,
        reply_to_note_text, update_note_in_db,
    },
    prelude::*,
};

use crate::{Args, NotesAction, NotesArgs, RevisionArgs, init_color_mode};

/// Parse a line or inclusive range `N` or `N-M` into a `(start, end)` span.
pub fn parse_line_span(spec: &str, ctx: &str) -> (u32, u32) {
    let (s_str, e_str) = match spec.split_once('-') {
        Some((a, b)) => (a, b),
        None => (spec, spec),
    };
    let s: u32 = s_str.parse().unwrap_or_else(|_| {
        eprintln!("diffuse: bad line number '{s_str}' in {ctx}");
        process::exit(1);
    });
    let e: u32 = e_str.parse().unwrap_or_else(|_| {
        eprintln!("diffuse: bad line number '{e_str}' in {ctx}");
        process::exit(1);
    });
    if s == 0 || e == 0 {
        eprintln!("diffuse: line coordinates must be 1 or greater in {ctx}");
        process::exit(1);
    }
    (s.min(e), s.max(e))
}

/// A plain synthetic context row used as a note's provisional stored row
/// while the definitive rows are located in the pipeline output.
fn provisional_ctx_row(old: Option<u32>, new: Option<u32>, text: &str) -> OutputRow {
    OutputRow {
        kind: RowKind::Ctx,
        mov_side: None,
        old_line: old,
        new_line: new,
        runs: vec![OutputRun::new(RunKind::Ctx, text.to_string())],
        wholesale: false,
        group: None,
    }
}

/// Relocate a coordinate span onto actual output rows: the window of rows
/// whose numbers cover `old_span` and `new_span`. The two spans must be
/// equally long (validated by the caller). Returns `None` when the rows do
/// not cover the span (count or boundary mismatch).
///
/// The span is located on its key side: the new side when `new_span` is
/// given, otherwise the old side. When both spans are given they act as a
/// two-sided exact coordinate match: the located boundary rows must carry
/// the requested old numbers.
pub(crate) fn relocate_span(
    rows: &[&OutputRow],
    old_span: Option<(u32, u32)>,
    new_span: Option<(u32, u32)>,
) -> Option<Vec<OutputRow>> {
    let (key_is_old, span, both) = match (old_span, new_span) {
        (Some(_), Some(new)) => (false, new, true),
        (None, Some(new)) => (false, new, false),
        (Some(old), None) => (true, old, false),
        (None, None) => return None,
    };
    let (start, end) = span;
    if start > end {
        return None;
    }

    let first_idx = if both {
        let (os, _) = old_span.unwrap();
        rows.iter()
            .position(|r| r.old_line == Some(os) && r.new_line == Some(start))?
    } else if key_is_old {
        rows.iter().position(|r| r.old_line == Some(start))?
    } else {
        rows.iter().position(|r| r.new_line == Some(start))?
    };

    let last_idx = if both {
        let (_, oe) = old_span.unwrap();
        rows[first_idx..]
            .iter()
            .position(|r| r.old_line == Some(oe) && r.new_line == Some(end))
            .map(|offset| first_idx + offset)?
    } else if key_is_old {
        rows[first_idx..]
            .iter()
            .position(|r| r.old_line == Some(end))
            .map(|offset| first_idx + offset)?
    } else {
        rows[first_idx..]
            .iter()
            .position(|r| r.new_line == Some(end))
            .map(|offset| first_idx + offset)?
    };

    let start_coord = DiffCoord::from_row(rows[first_idx])?;
    let end_coord = DiffCoord::from_row(rows[last_idx])?;

    let window: Vec<&OutputRow> = rows[first_idx..=last_idx]
        .iter()
        .copied()
        .filter(|r| DiffCoord::row_in_span(r, start_coord, end_coord))
        .collect();

    // Verify all requested lines on the key side (or both sides) are present and in
    // order
    if key_is_old {
        let old_lines: Vec<u32> = window.iter().filter_map(|r| r.old_line).collect();
        if old_lines != (start..=end).collect::<Vec<_>>() {
            return None;
        }
    } else {
        let new_lines: Vec<u32> = window.iter().filter_map(|r| r.new_line).collect();
        if new_lines != (start..=end).collect::<Vec<_>>() {
            return None;
        }
        if both {
            let (os, oe) = old_span.unwrap();
            let old_lines: Vec<u32> = window.iter().filter_map(|r| r.old_line).collect();
            if old_lines != (os..=oe).collect::<Vec<_>>() {
                return None;
            }
        }
    }

    Some(window.into_iter().cloned().collect())
}

/// Exit when a file is not present in the reviewed content snapshots.
pub fn missing_file_exit(path: &str) -> ! {
    eprintln!("diffuse: file '{path}' is not part of the current diff");
    process::exit(1);
}

/// Reject spans running past the tracked content lines.
pub fn check_span_bounds(sp: (u32, u32), lines: &[&str], path: &str) {
    if sp.0 as usize > lines.len() || sp.1 as usize > lines.len() {
        eprintln!(
            "diffuse: target range {}-{} exceeds the {} lines of '{path}'",
            sp.0,
            sp.1,
            lines.len()
        );
        process::exit(1);
    }
}

/// The stored original source rows as plain text lines (Markdown fences).
pub fn plain_row_texts(rows: &[OutputRow]) -> String {
    rows.iter()
        .map(OutputRow::text)
        .collect::<Vec<String>>()
        .join("\n")
}

/// Format a note's stored original source rows for CLI display wrapped at
/// terminal width, with standard diff row coloring (or plain text when color is
/// disabled), without extra line-prefix indentation.
pub fn format_rows_with_context(rows: &[OutputRow], use_color: bool, width: usize) -> Vec<String> {
    let theme = DiffuseTheme::DEFAULT;
    let rendered = render_source_rows(rows, &theme, width, true);
    rendered
        .into_iter()
        .map(|l| {
            if use_color {
                l
            } else {
                strip_ansi(&l).to_string()
            }
        })
        .collect()
}

/// Print a note's stored original source rows wrapped at terminal width,
/// stripping ANSI when color is disabled.
fn print_rows_with_context(rows: &[OutputRow], use_color: bool) {
    let width = detect_terminal_width();
    for l in format_rows_with_context(rows, use_color, width) {
        println!("{l}");
    }
}

pub fn run_notes_command(notes_args: &NotesArgs, args: &Args) -> io::Result<()> {
    let notes_path = args
        .notes_file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_path);
    let mut db = load_notes(&notes_path);

    let current_dir = env::current_dir()?;
    let repo = if is_inside_work_tree(&current_dir) {
        Repo::from_root(&current_dir).with_name_opt(args.repo.clone())
    } else {
        Repo::from_name(args.repo.clone().unwrap_or_else(|| "diffuse".to_string()))
    };
    let target =
        ReviewTarget::from_repo_args(repo.clone(), args.revision.git_revs.clone(), args.staged);
    let repo_name = Some(target.repo_name().to_string());
    let notes_key = target.notes_key();

    match &notes_args.action {
        None | Some(NotesAction::List { .. }) => {
            let (resolved, in_progress, all, show_source, json, markdown, sub_revision) =
                if let Some(NotesAction::List {
                    resolved,
                    in_progress,
                    all,
                    show_source,
                    json,
                    markdown,
                    ref revision,
                }) = notes_args.action
                {
                    (
                        resolved || notes_args.resolved,
                        in_progress || notes_args.in_progress,
                        all || notes_args.all,
                        show_source || notes_args.show_source,
                        json || notes_args.json,
                        markdown || notes_args.markdown,
                        revision.clone(),
                    )
                } else {
                    (
                        notes_args.resolved,
                        notes_args.in_progress,
                        notes_args.all,
                        notes_args.show_source,
                        notes_args.json,
                        notes_args.markdown,
                        RevisionArgs::default(),
                    )
                };

            let mut combined_revision = args.revision.clone();
            combined_revision.extend(&notes_args.revision);
            combined_revision.extend(&sub_revision);

            let list_target = ReviewTarget::from_parts(
                repo,
                combined_revision.git_revs,
                combined_revision.pathspecs,
                args.staged,
            );
            let keys = list_target.matched_notes_db_keys(&db);

            let mut all_notes: Vec<(&str, &ReviewNote)> = Vec::new();
            for k in &keys {
                if let Some(repo_map) = db.get(*k) {
                    for (file, notes) in repo_map {
                        if !list_target.matches_path(file, &current_dir) {
                            continue;
                        }
                        for note in notes {
                            all_notes.push((file.as_str(), note));
                        }
                    }
                }
            }
            all_notes
                .sort_by(|(f1, n1), (f2, n2)| f1.cmp(f2).then_with(|| n1.start().cmp(&n2.start())));

            let total_resolved = all_notes
                .iter()
                .filter(|(_, n)| n.status.is_resolved())
                .count();

            let filtered: Vec<(&str, &ReviewNote)> = all_notes
                .into_iter()
                .filter(|(_, n)| {
                    if all {
                        true
                    } else if resolved {
                        n.status.is_resolved()
                    } else if in_progress {
                        n.status.is_in_progress()
                    } else {
                        n.status.counts_as_open()
                    }
                })
                .collect();

            if json {
                let json_notes: Vec<&ReviewNote> = filtered.iter().map(|(_, n)| *n).collect();
                let payload = serde_json::json!({
                    "notes": json_notes,
                    "resolvedCount": total_resolved,
                });
                println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                return Ok(());
            }

            if markdown {
                if filtered.is_empty() {
                    if total_resolved > 0 && !resolved && !all {
                        println!("*No open notes. (✅ {total_resolved} resolved)*");
                    } else {
                        println!("*No review notes found.*");
                    }
                    return Ok(());
                }

                for (file, note) in &filtered {
                    let glyph = note.status.glyph();
                    let lines = note.location.label();
                    println!("### {glyph} `{file}:{lines}` (`{}`)\n", note.id);
                    if show_source && !note.original_source_rows.is_empty() {
                        println!(
                            "```\n{}\n```\n",
                            plain_row_texts(&note.original_source_rows)
                        );
                    }
                    println!("{}\n", note.text);
                }
                if total_resolved > 0 && !resolved && !all {
                    println!("*✅ {total_resolved} resolved*\n");
                }
                return Ok(());
            }

            let use_color = init_color_mode(&args.color, args.no_color);

            if filtered.is_empty() {
                if total_resolved > 0 && !resolved && !all {
                    println!("No open notes. (✅ {total_resolved} resolved)");
                } else {
                    println!("No review notes found.");
                }
                return Ok(());
            }

            for (file, note) in &filtered {
                let lines = note.location.label();
                if use_color {
                    let glyph_color = match note.status {
                        NoteStatus::Open => "\x1b[38;5;214m",
                        NoteStatus::InProgress => "\x1b[38;5;75m",
                        NoteStatus::Resolved => "\x1b[38;5;120m",
                    };
                    println!(
                        "{glyph_color}{}\x1b[0m \x1b[1m{file}:{lines}\x1b[0m \x1b[2m[{}]\x1b[0m",
                        note.status.glyph(),
                        note.id
                    );
                } else {
                    println!("{} {file}:{lines} [{}]", note.status.glyph(), note.id);
                }

                if show_source && !note.original_source_rows.is_empty() {
                    print_rows_with_context(&note.original_source_rows, use_color);
                }

                for l in note.text.lines() {
                    println!("  {l}");
                }
                println!();
            }

            if total_resolved > 0 && !resolved && !all {
                if use_color {
                    println!(
                        "\x1b[38;5;120m✅\x1b[0m \x1b[2m{total_resolved} resolved (use --resolved or --all to view)\x1b[0m"
                    );
                } else {
                    println!("✅ {total_resolved} resolved (use --resolved or --all to view)");
                }
            }
        }
        Some(NotesAction::Add {
            target,
            old,
            new,
            message,
            text,
            status,
            json,
        }) => {
            // 1. Resolve the target file and coordinate spans.
            let mut file_path: Option<String> = None;
            let mut target_span: Option<(u32, u32)> = None;
            if let Some(t) = target {
                match t.split_once(':') {
                    Some((f, range)) => {
                        file_path = Some(f.to_string());
                        target_span = Some(parse_line_span(range, &format!("'{t}'")));
                    }
                    None => file_path = Some(t.clone()),
                }
            }
            let Some(file_path) = file_path else {
                eprintln!(
                    "diffuse: missing target file (e.g. diffuse notes create <file>:<line> -m \"...\")"
                );
                process::exit(1);
            };

            let old_span = old.as_deref().map(|spec| parse_line_span(spec, "--old"));
            let mut new_span = new.as_deref().map(|spec| parse_line_span(spec, "--new"));
            if let Some(sp) = target_span {
                match new_span {
                    Some(existing) if existing != sp => {
                        eprintln!(
                            "diffuse: conflicting new-side coordinates between the target and --new {new}",
                            new = new.as_deref().unwrap_or_default()
                        );
                        process::exit(1);
                    }
                    Some(_) => {}
                    None => new_span = Some(sp),
                }
            }
            if new_span.is_none() && old_span.is_none() {
                eprintln!(
                    "diffuse: missing line coordinates (provide {file}:<line>, --new N, or --old N)",
                    file = file_path
                );
                process::exit(1);
            }
            // A both-side anchor locates one set of rows: the spans must be
            // equally long.
            let span_len = |sp: (u32, u32)| sp.1 - sp.0 + 1;
            if let (Some(os), Some(ns)) = (old_span, new_span)
                && span_len(os) != span_len(ns)
            {
                eprintln!(
                    "diffuse: --old range {}-{} and --new range {}-{} must cover the same number of lines",
                    os.0, os.1, ns.0, ns.1
                );
                process::exit(1);
            }

            // 2. Resolve note message
            let msg = if let Some(m) = message {
                m.clone()
            } else if let Some(t) = text {
                t.clone()
            } else if !io::stdin().is_terminal() {
                let mut buf = String::new();
                io::stdin().read_to_string(&mut buf)?;
                buf.trim().to_string()
            } else {
                eprintln!("diffuse: note message required (use -m/--message or provide text)");
                process::exit(1);
            };

            if msg.trim().is_empty() {
                eprintln!("diffuse: note message cannot be empty");
                process::exit(1);
            }

            // 3. Resolve status
            let status_val = match status.to_lowercase().as_str() {
                "in-progress" | "progress" | "inprogress" => NoteStatus::InProgress,
                "resolved" | "resolve" => NoteStatus::Resolved,
                _ => NoteStatus::Open,
            };

            // 4. The note stores the diff rows it was created against. When the file is
            //    part of the reviewed diff, the pipeline runs with the provisional note
            //    registered (so gap synthesis includes its span) and the note is relocated
            //    onto the produced rows. Files outside the reviewed diff fall back to plain
            //    rows read from the working-tree content; the note then displays on the
            //    file banner.
            let reviewed = if is_inside_work_tree(&current_dir) {
                Some(
                    load_git_repo_with_notes(
                        &current_dir,
                        &args.revision.git_revs,
                        std::slice::from_ref(&file_path),
                        args.staged,
                        ENGINE_CONTEXT,
                        None,
                        None,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("diffuse: cannot load diff: {e}");
                        process::exit(1);
                    }),
                )
            } else {
                None
            };
            if reviewed.is_none() && new_span.is_none() {
                eprintln!(
                    "diffuse: --old coordinates require the file to be part of a reviewed git diff"
                );
                process::exit(1);
            }
            let reviewed_content = reviewed.as_ref().map(|g| {
                if new_span.is_some() {
                    g.new_content.get(&file_path)
                } else {
                    g.old_content.get(&file_path)
                }
            });

            // Provisional rows: plain context rows read from the tracked
            // content at the given coordinates, on the span's primary side
            // (new side when given, else the old side).
            let (content, side_new) = match (reviewed_content, new_span, old_span) {
                (Some(Some(c)), _, _) => (c.clone(), new_span.is_some()),
                (Some(None), _, Some(_)) => {
                    // Reviewed file where the requested content side has no
                    // snapshot (e.g. a new-side span on a deleted file).
                    missing_file_exit(&file_path);
                }
                (_, _, _) => {
                    // Not part of the reviewed diff: fall back to the
                    // working-tree content (new side only).
                    match std::fs::read_to_string(current_dir.join(&file_path)) {
                        Ok(c) => (c, true),
                        Err(_) => missing_file_exit(&file_path),
                    }
                }
            };
            let (start, end) = new_span.or(old_span).expect("coordinates validated above");
            let lines: Vec<&str> = content.lines().collect();
            check_span_bounds((start, end), &lines, &file_path);
            let provisional_rows: Vec<OutputRow> = (start..=end)
                .map(|n| {
                    if side_new {
                        provisional_ctx_row(None, Some(n), lines[(n - 1) as usize])
                    } else {
                        provisional_ctx_row(Some(n), None, lines[(n - 1) as usize])
                    }
                })
                .collect();
            let span_coord = |n: u32| {
                if side_new {
                    DiffCoord::from_numbers(None, Some(n))
                } else {
                    DiffCoord::from_numbers(Some(n), None)
                }
                .expect("provisional coordinates carry a line number")
            };
            let provisional_location = if start == end {
                NoteLocation::Line {
                    coord: span_coord(start),
                }
            } else {
                NoteLocation::Range {
                    start: span_coord(start),
                    end: span_coord(end),
                }
            };

            let fallback_rows = provisional_rows.clone();
            let note_id = generate_note_id();
            let provisional_note = ReviewNote {
                id: note_id.clone(),
                file: file_path.clone(),
                location: provisional_location,
                text: msg.clone(),
                created_at: now_millis(),
                content_hash: None,
                status: status_val,
                original_source_rows: provisional_rows,
            };

            // The pipeline runs with the provisional note registered so gap
            // synthesis includes its span; afterwards the note is finalized
            // from the produced rows. The provisional note lives only in a
            // scratch database: the persistent database receives the finalized
            // note exclusively.
            let window: Vec<OutputRow> = if let Some(git_res) =
                reviewed_content.is_some().then_some(reviewed).flatten()
            {
                let mut prov_db = db.clone();
                add_note(&mut prov_db, &notes_key, provisional_note);
                let opts =
                    PipelineOptions::default().with_notes(Some(prov_db), Some(notes_key.clone()));
                let out = run_pipeline_full(
                    &git_res.diff,
                    Some(git_res.new_content),
                    Some(git_res.old_content),
                    opts,
                );

                let Some(f) = out.files.iter().find(|f| f.path == file_path) else {
                    missing_file_exit(&file_path);
                };
                let rows: Vec<&OutputRow> = if f.rows.is_empty() {
                    f.hunks.iter().flat_map(|h| h.rows.iter()).collect()
                } else {
                    f.rows.iter().collect()
                };
                match relocate_span(&rows, old_span, new_span) {
                    Some(window) => window,
                    None => {
                        if let (Some(os), Some(ns)) = (old_span, new_span) {
                            eprintln!(
                                "diffuse: --old {}-{} and --new {}-{} do not locate the same diff rows in '{file_path}'",
                                os.0, os.1, ns.0, ns.1,
                            );
                            process::exit(1);
                        }
                        // Spans the pipeline did not materialize degrade to
                        // the provisional rows: the note persists unanchored
                        // and displays on the file banner.
                        fallback_rows
                    }
                }
            } else {
                fallback_rows
            };

            // 5. Finalize: coordinates come from the located rows (fixing provisional
            //    single-side coordinates), the stored rows are the actual output rows, and
            //    the hash covers them completely.
            let final_note = ReviewNote {
                id: note_id.clone(),
                file: file_path.clone(),
                location: if window.len() == 1 {
                    NoteLocation::Line {
                        coord: DiffCoord::from_row(&window[0])
                            .expect("relocated row carries coordinates"),
                    }
                } else {
                    NoteLocation::Range {
                        start: DiffCoord::from_row(
                            window.first().expect("non-empty relocation window"),
                        )
                        .expect("relocated row carries coordinates"),
                        end: DiffCoord::from_row(
                            window.last().expect("non-empty relocation window"),
                        )
                        .expect("relocated row carries coordinates"),
                    }
                },
                text: msg,
                created_at: now_millis(),
                content_hash: LineHash::of_rows(&window),
                status: status_val,
                original_source_rows: window,
            };

            // 6. Persist and output.
            add_note(&mut db, &notes_key, final_note.clone());
            save_notes(&notes_path, &db)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string(&final_note).expect("serialize note")
                );
            } else {
                println!(
                    "Created note {} on {}:{} [{:?}] {}",
                    note_id,
                    final_note.file,
                    final_note.location.label(),
                    status_val,
                    status_val.glyph()
                );
            }
        }
        Some(NotesAction::Show {
            id,
            show_source,
            json,
            markdown,
        }) => {
            let found = find_note_in_db(&db, repo_name.as_deref(), id);
            let (_key, file, note) = match found {
                Some(res) => res,
                None => {
                    eprintln!("diffuse: note '{id}' not found");
                    process::exit(1);
                }
            };

            if *json {
                println!("{}", serde_json::to_string_pretty(note).unwrap());
                return Ok(());
            }

            if *markdown {
                let glyph = note.status.glyph();
                let lines = note.location.label();
                println!("### {glyph} `{file}:{lines}` (`{}`)\n", note.id);
                println!("- **Status**: {glyph} {:?}", note.status);
                println!("- **Location**: {}", note.location.label());
                println!("- **Created**: {}", note.created_at);
                if *show_source && !note.original_source_rows.is_empty() {
                    println!(
                        "\n```\n{}\n```\n",
                        plain_row_texts(&note.original_source_rows)
                    );
                }
                println!("\n{}", note.text);
                return Ok(());
            }

            let lines = note.location.label();
            println!("Note:     {}", note.id);
            println!("Status:   {} {:?}", note.status.glyph(), note.status);
            println!("File:     {file}:{lines}");
            println!("Created:  {}", note.created_at);
            if *show_source && !note.original_source_rows.is_empty() {
                println!("Source:");
                let use_color = init_color_mode(&args.color, args.no_color);
                print_rows_with_context(&note.original_source_rows, use_color);
            }
            println!("Text:");
            for l in note.text.lines() {
                println!("  {l}");
            }
        }
        Some(NotesAction::Reply {
            id,
            message,
            text,
            status,
            resolve,
        }) => {
            let reply_content = match (message.as_deref(), text.as_deref()) {
                (Some("-"), _) | (None, Some("-")) => {
                    let mut buf = String::new();
                    io::stdin().read_to_string(&mut buf)?;
                    buf.trim().to_string()
                }
                (Some(m), _) => m.to_string(),
                (None, Some(t)) => t.to_string(),
                (None, None) => {
                    if !io::stdin().is_terminal() {
                        let mut buf = String::new();
                        io::stdin().read_to_string(&mut buf)?;
                        buf.trim().to_string()
                    } else {
                        eprintln!(
                            "diffuse: empty reply message (use -m \"message\" or supply text)"
                        );
                        process::exit(1);
                    }
                }
            };

            if reply_content.trim().is_empty() {
                eprintln!("diffuse: empty reply message");
                process::exit(1);
            }

            let target_status = if *resolve {
                Some(NoteStatus::Resolved)
            } else if let Some(st) = status {
                match st.to_lowercase().as_str() {
                    "open" => Some(NoteStatus::Open),
                    "in-progress" | "inprogress" | "progress" => Some(NoteStatus::InProgress),
                    "resolved" => Some(NoteStatus::Resolved),
                    other => {
                        eprintln!(
                            "diffuse: invalid status '{other}' (expected open, in-progress, or resolved)"
                        );
                        process::exit(1);
                    }
                }
            } else {
                None
            };

            let mut updated_status = None;
            let updated = update_note_in_db(&mut db, repo_name.as_deref(), id, |n| {
                n.text = reply_to_note_text(&n.text, &reply_content);
                if let Some(s) = target_status {
                    n.status = s;
                }
                updated_status = Some(n.status);
            });

            if !updated {
                eprintln!("diffuse: note '{id}' not found");
                process::exit(1);
            }
            save_notes(&notes_path, &db)?;
            let st = updated_status.unwrap();
            println!("Replied to note {id} (status: {} {:?})", st.glyph(), st);
        }
        Some(NotesAction::Progress { id }) => {
            let updated = update_note_in_db(&mut db, repo_name.as_deref(), id, |n| {
                n.status = NoteStatus::InProgress;
            });
            if !updated {
                eprintln!("diffuse: note '{id}' not found");
                process::exit(1);
            }
            save_notes(&notes_path, &db)?;
            println!("Marked note {id} as in-progress 🔄");
        }
        Some(NotesAction::Resolve { id }) => {
            let updated = update_note_in_db(&mut db, repo_name.as_deref(), id, |n| {
                n.status = NoteStatus::Resolved;
            });
            if !updated {
                eprintln!("diffuse: note '{id}' not found");
                process::exit(1);
            }
            save_notes(&notes_path, &db)?;
            println!("Marked note {id} as resolved ✅");
        }
        Some(NotesAction::Open { id }) | Some(NotesAction::Reopen { id }) => {
            let updated = update_note_in_db(&mut db, repo_name.as_deref(), id, |n| {
                n.status = NoteStatus::Open;
            });
            if !updated {
                eprintln!("diffuse: note '{id}' not found");
                process::exit(1);
            }
            save_notes(&notes_path, &db)?;
            println!("Reopened note {id} 💬");
        }
        Some(NotesAction::Status { id, status }) => {
            let parsed_status = match status.to_lowercase().as_str() {
                "open" => NoteStatus::Open,
                "in-progress" | "inprogress" | "progress" => NoteStatus::InProgress,
                "resolved" => NoteStatus::Resolved,
                other => {
                    eprintln!(
                        "diffuse: invalid status '{other}' (expected open, in-progress, or resolved)"
                    );
                    process::exit(1);
                }
            };
            let updated = update_note_in_db(&mut db, repo_name.as_deref(), id, |n| {
                n.status = parsed_status;
            });
            if !updated {
                eprintln!("diffuse: note '{id}' not found");
                process::exit(1);
            }
            save_notes(&notes_path, &db)?;
            println!(
                "Updated note {id} status to {} {:?}",
                parsed_status.glyph(),
                parsed_status
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffuse_core::{
        merge::RunKind,
        parse::RowKind,
        wire::{OutputRow, OutputRun},
    };

    #[test]
    fn test_format_rows_with_context_no_bar_and_wraps_at_width() {
        let rows = vec![OutputRow {
            kind: RowKind::Add,
            mov_side: None,
            new_line: Some(392),
            old_line: None,
            runs: vec![OutputRun::new(
                RunKind::Add,
                "fn for_each_diff_line(row: &crate::core::wire::OutputRow, mut f: impl FnMut(char, String)) {".to_string(),
            )],
            wholesale: false,
            group: None,
        }];

        // Test at width 60: must wrap and must NOT contain indentation '│'
        let formatted_60 = format_rows_with_context(&rows, false, 60);
        assert!(
            formatted_60.len() >= 2,
            "must wrap at 60 columns: got {:?}",
            formatted_60
        );
        assert!(
            formatted_60[0].contains(" 392 +"),
            "first line has diff gutter prefix: {:?}",
            formatted_60[0]
        );
        for line in &formatted_60 {
            assert!(
                !line.contains('│'),
                "must remove the indentation '│' for source lines: got {line:?}"
            );
        }

        // Test at width 120: fits on a single line without wrapping
        let formatted_120 = format_rows_with_context(&rows, false, 120);
        assert_eq!(
            formatted_120.len(),
            1,
            "must fit on single line at width 120: got {:?}",
            formatted_120
        );
        assert!(!formatted_120[0].contains('│'));
        assert!(formatted_120[0].contains(" 392 +"));
    }
}
