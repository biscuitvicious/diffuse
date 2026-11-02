//! Handler for listing review notes across files and revisions.

use std::{io, path::Path};

use diffuse::prelude::*;

use super::format::{plain_row_texts, print_note_text, print_rows_with_context};
use crate::{Args, NotesAction, NotesArgs, RevisionArgs, init_color_mode};

#[allow(clippy::too_many_arguments)]
pub fn handle_list(
    action: Option<&NotesAction>,
    notes_args: &NotesArgs,
    args: &Args,
    db: &NotesDB,
    repo: Repo,
    repo_key: &str,
    current_dir: &Path,
) -> io::Result<()> {
    let (resolved, in_progress, all, show_source, json, markdown, sub_revision, list_rev) =
        if let Some(NotesAction::List {
            resolved,
            in_progress,
            all,
            show_source,
            json,
            markdown,
            revision,
            rev,
        }) = action
        {
            (
                *resolved || notes_args.resolved,
                *in_progress || notes_args.in_progress,
                *all || notes_args.all,
                *show_source || notes_args.show_source,
                *json || notes_args.json,
                *markdown || notes_args.markdown,
                revision.clone(),
                rev.clone(),
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
                None,
            )
        };

    let mut combined_revision = args.revision.clone();
    combined_revision.extend(&notes_args.revision);
    combined_revision.extend(&sub_revision);
    if let Some(r) = list_rev.as_ref().or(notes_args.rev.as_ref()) {
        combined_revision.git_revs.push(r.clone());
    }

    let list_target = ReviewTarget::from_parts(
        repo,
        combined_revision.git_revs,
        combined_revision.pathspecs,
        args.staged,
    )
    .with_rev_key_opt(args.rev_key.clone());
    let rev_keys = list_target.matched_rev_keys(db);

    let mut all_notes: Vec<(&str, &ReviewNote)> = Vec::new();
    if let Some(rev_map) = db.get(repo_key) {
        for rk in &rev_keys {
            if let Some(file_map) = rev_map.get(*rk) {
                for (file, notes) in file_map {
                    if !list_target.matches_path(file, current_dir) {
                        continue;
                    }
                    for note in notes {
                        all_notes.push((file.as_str(), note));
                    }
                }
            }
        }
    }
    all_notes.sort_by(|(f1, n1), (f2, n2)| f1.cmp(f2).then_with(|| n1.start().cmp(&n2.start())));

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

        print_note_text(&note.text, use_color);
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

    Ok(())
}
