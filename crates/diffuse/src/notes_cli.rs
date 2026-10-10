//! CLI handlers for the `diffuse notes` subcommand suite.

use std::{
    env,
    io::{self, IsTerminal, Read},
    path::PathBuf,
    process,
};

use diffuse::{
    git::is_inside_work_tree,
    notes::{
        find_note_in_db, generate_note_id, hash_line_content, now_millis, reply_to_note_text,
        synthesize_source_lines, update_note_in_db,
    },
    prelude::*,
};

use crate::{Args, NotesAction, NotesArgs, RevisionArgs, init_color_mode};

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
        ReviewTarget::from_repo_args(repo.clone(), args.revision.git_args.clone(), args.staged);
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

            let list_target =
                ReviewTarget::from_repo_args(repo, combined_revision.git_args, args.staged);
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
                .sort_by(|(f1, n1), (f2, n2)| f1.cmp(f2).then_with(|| n1.start.cmp(&n2.start)));

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
                    let lines = if note.start == note.end {
                        format!("{}", note.start)
                    } else {
                        format!("{}-{}", note.start, note.end)
                    };
                    println!("### {glyph} `{file}:{lines}` (`{}`)\n", note.id);
                    if show_source && let Some(ref src) = note.source_lines {
                        println!("```\n{src}\n```\n");
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
                let lines = if note.start == note.end {
                    format!("{}", note.start)
                } else {
                    format!("{}-{}", note.start, note.end)
                };
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

                if show_source && let Some(ref src) = note.source_lines {
                    for l in src.lines() {
                        if use_color {
                            println!("  \x1b[2m│ \x1b[0m\x1b[38;5;248m{l}\x1b[0m");
                        } else {
                            println!("  │ {l}");
                        }
                    }
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
            line,
            end,
            pos2,
            pos3,
            file,
            side,
            old,
            message,
            text,
            source,
            no_source,
            status,
            json,
        }) => {
            // 1. Resolve file path and line coordinates
            let mut resolved_file = file.clone();
            let mut resolved_start: Option<u32> = *line;
            let mut resolved_end: Option<u32> = *end;
            let mut positional_msg: Option<String> = None;

            if let Some(t) = target {
                if let Some((f, range)) = t.split_once(':') {
                    resolved_file = Some(f.to_string());
                    if let Some((s_str, e_str)) = range.split_once('-') {
                        let s: u32 = s_str.parse().unwrap_or_else(|_| {
                            eprintln!("diffuse: invalid start line '{s_str}' in target '{t}'");
                            process::exit(1);
                        });
                        let e: u32 = e_str.parse().unwrap_or_else(|_| {
                            eprintln!("diffuse: invalid end line '{e_str}' in target '{t}'");
                            process::exit(1);
                        });
                        resolved_start = Some(s);
                        resolved_end = Some(e);
                    } else {
                        let s: u32 = range.parse().unwrap_or_else(|_| {
                            eprintln!("diffuse: invalid line '{range}' in target '{t}'");
                            process::exit(1);
                        });
                        resolved_start = Some(s);
                        resolved_end = resolved_end.or(Some(s));
                    }
                    if let Some(p2) = pos2 {
                        positional_msg = Some(p2.clone());
                    } else if let Some(p3) = pos3 {
                        positional_msg = Some(p3.clone());
                    }
                } else {
                    resolved_file = Some(t.clone());
                    if resolved_start.is_none() {
                        if let Some(p2) = pos2 {
                            if let Ok(s) = p2.parse::<u32>() {
                                resolved_start = Some(s);
                                if let Some(p3) = pos3 {
                                    if let Ok(e) = p3.parse::<u32>() {
                                        resolved_end = Some(e);
                                    } else {
                                        positional_msg = Some(p3.clone());
                                    }
                                }
                            } else {
                                eprintln!(
                                    "diffuse: missing line number for file '{t}' (expected e.g. '{t}:<line>' or '{t} <line>')"
                                );
                                process::exit(1);
                            }
                        } else {
                            eprintln!(
                                "diffuse: missing line number for file '{t}' (expected e.g. '{t}:<line>' or '{t} <line>')"
                            );
                            process::exit(1);
                        }
                    } else if let Some(p2) = pos2 {
                        positional_msg = Some(p2.clone());
                    }
                }
            } else if resolved_file.is_some() {
                if resolved_start.is_none() {
                    if let Some(p2) = pos2 {
                        if let Ok(s) = p2.parse::<u32>() {
                            resolved_start = Some(s);
                            if let Some(p3) = pos3 {
                                if let Ok(e) = p3.parse::<u32>() {
                                    resolved_end = Some(e);
                                } else {
                                    positional_msg = Some(p3.clone());
                                }
                            }
                        } else {
                            eprintln!(
                                "diffuse: missing line number (use --line or provide line number)"
                            );
                            process::exit(1);
                        }
                    } else {
                        eprintln!(
                            "diffuse: missing line number (use --line or provide line number)"
                        );
                        process::exit(1);
                    }
                }
            } else {
                eprintln!(
                    "diffuse: missing target file (e.g. diffuse notes add <file>:<line> -m \"...\")"
                );
                process::exit(1);
            }

            let file_path = resolved_file.expect("file_path resolved");
            let start_line = resolved_start.unwrap_or(1);
            let end_line = resolved_end.unwrap_or(start_line);

            if start_line == 0 {
                eprintln!("diffuse: line numbers must be 1 or greater");
                process::exit(1);
            }
            if end_line < start_line {
                eprintln!(
                    "diffuse: end line ({end_line}) cannot be less than start line ({start_line})"
                );
                process::exit(1);
            }

            // 2. Resolve note message
            let msg = if let Some(m) = message {
                m.clone()
            } else if let Some(t) = text {
                t.clone()
            } else if let Some(p) = positional_msg {
                p
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

            // 3. Resolve side
            let side_val = if *old || side.eq_ignore_ascii_case("old") {
                Side::Old
            } else {
                Side::New
            };

            // 4. Resolve status
            let status_val = match status.to_lowercase().as_str() {
                "in-progress" | "progress" | "inprogress" => NoteStatus::InProgress,
                "resolved" | "resolve" => NoteStatus::Resolved,
                _ => NoteStatus::Open,
            };

            // 5. Resolve source lines
            let source_lines_val = if *no_source {
                None
            } else if let Some(s) = source {
                Some(s.clone())
            } else {
                let repo_root = if is_inside_work_tree(&current_dir) {
                    Some(current_dir.as_path())
                } else {
                    None
                };
                synthesize_source_lines(repo_root, &file_path, side_val, start_line, end_line)
            };

            // 6. Calculate line hash from the last line of the source range (anchoring
            //    point)
            let line_hash_val = source_lines_val
                .as_ref()
                .and_then(|s| s.lines().last().map(hash_line_content));

            // 7. Generate note ID
            let note_id = generate_note_id();

            // 8. Create note and persist
            let note = ReviewNote {
                id: note_id.clone(),
                file: file_path.clone(),
                side: side_val,
                start: start_line,
                end: end_line,
                text: msg,
                created_at: now_millis(),
                line_hash: line_hash_val,
                status: status_val,
                source_lines: source_lines_val,
            };

            add_note(&mut db, &notes_key, note.clone());
            save_notes(&notes_path, &db)?;

            // 9. Output
            if *json {
                println!("{}", serde_json::to_string(&note).expect("serialize note"));
            } else {
                let range_str = if start_line == end_line {
                    format!("{start_line}")
                } else {
                    format!("{start_line}-{end_line}")
                };
                let side_str = match side_val {
                    Side::New => "new",
                    Side::Old => "old",
                };
                println!(
                    "Created note {note_id} on {file_path}:{range_str} ({side_str}) [{:?}] {}",
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
                let lines = if note.start == note.end {
                    format!("{}", note.start)
                } else {
                    format!("{}-{}", note.start, note.end)
                };
                println!("### {glyph} `{file}:{lines}` (`{}`)\n", note.id);
                println!("- **Status**: {glyph} {:?}", note.status);
                println!("- **Side**: {:?}", note.side);
                println!("- **Created**: {}", note.created_at);
                if *show_source && let Some(ref src) = note.source_lines {
                    println!("\n```\n{src}\n```\n");
                }
                println!("\n{}", note.text);
                return Ok(());
            }

            let lines = if note.start == note.end {
                format!("{}", note.start)
            } else {
                format!("{}-{}", note.start, note.end)
            };
            println!("Note:     {}", note.id);
            println!("Status:   {} {:?}", note.status.glyph(), note.status);
            println!("File:     {file}:{lines} ({:?})", note.side);
            println!("Created:  {}", note.created_at);
            if *show_source && let Some(ref src) = note.source_lines {
                println!("Source:");
                for l in src.lines() {
                    println!("  │ {l}");
                }
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
