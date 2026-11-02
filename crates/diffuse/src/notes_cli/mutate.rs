//! Handlers for showing, replying to, and updating note status.

use std::{
    io::{self, IsTerminal, Read},
    path::Path,
    process,
};

use diffuse::{
    notes::{find_note_in_repo, reply_to_note_text, update_note_in_repo},
    prelude::*,
};

use super::format::{plain_row_texts, print_note_text, print_rows_with_context};
use crate::{Args, init_color_mode};

pub fn handle_show(
    id: &str,
    show_source: bool,
    json: bool,
    markdown: bool,
    args: &Args,
    db: &NotesDB,
    repo_key: &str,
) -> io::Result<()> {
    let found = find_note_in_repo(db, repo_key, id);
    let (_rev, file, note) = match found {
        Some(res) => res,
        None => {
            eprintln!("diffuse: note '{id}' not found");
            process::exit(1);
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(note).unwrap());
        return Ok(());
    }

    if markdown {
        let glyph = note.status.glyph();
        let lines = note.location.label();
        println!("### {glyph} `{file}:{lines}` (`{}`)\n", note.id);
        println!("- **Status**: {glyph} {:?}", note.status);
        println!("- **Location**: {}", note.location.label());
        println!("- **Created**: {}", note.created_at);
        if show_source && !note.original_source_rows.is_empty() {
            println!(
                "\n```\n{}\n```\n",
                plain_row_texts(&note.original_source_rows)
            );
        }
        println!("\n{}", note.text);
        return Ok(());
    }

    let lines = note.location.label();
    let use_color = init_color_mode(&args.color, args.no_color);
    println!("Note:     {}", note.id);
    println!("Status:   {} {:?}", note.status.glyph(), note.status);
    println!("File:     {file}:{lines}");
    println!("Created:  {}", note.created_at);
    if show_source && !note.original_source_rows.is_empty() {
        println!("Source:");
        print_rows_with_context(&note.original_source_rows, use_color);
    }
    println!("Text:");
    print_note_text(&note.text, use_color);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn handle_reply(
    id: &str,
    message: &Option<String>,
    text: &Option<String>,
    status: &Option<String>,
    resolve: bool,
    db: &mut NotesDB,
    notes_path: &Path,
    repo_key: &str,
) -> io::Result<()> {
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
                eprintln!("diffuse: empty reply message (use -m \"message\" or supply text)");
                process::exit(1);
            }
        }
    };

    if reply_content.trim().is_empty() {
        eprintln!("diffuse: empty reply message");
        process::exit(1);
    }

    let target_status = if resolve {
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
    let updated = update_note_in_repo(db, repo_key, id, |n| {
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
    save_notes(notes_path, db)?;
    let st = updated_status.unwrap();
    println!("Replied to note {id} (status: {} {:?})", st.glyph(), st);

    Ok(())
}

pub fn handle_progress(
    id: &str,
    db: &mut NotesDB,
    notes_path: &Path,
    repo_key: &str,
) -> io::Result<()> {
    let updated = update_note_in_repo(db, repo_key, id, |n| {
        n.status = NoteStatus::InProgress;
    });
    if !updated {
        eprintln!("diffuse: note '{id}' not found");
        process::exit(1);
    }
    save_notes(notes_path, db)?;
    println!("Marked note {id} as in-progress 🔄");
    Ok(())
}

pub fn handle_resolve(
    id: &str,
    db: &mut NotesDB,
    notes_path: &Path,
    repo_key: &str,
) -> io::Result<()> {
    let updated = update_note_in_repo(db, repo_key, id, |n| {
        n.status = NoteStatus::Resolved;
    });
    if !updated {
        eprintln!("diffuse: note '{id}' not found");
        process::exit(1);
    }
    save_notes(notes_path, db)?;
    println!("Marked note {id} as resolved ✅");
    Ok(())
}

pub fn handle_open(
    id: &str,
    db: &mut NotesDB,
    notes_path: &Path,
    repo_key: &str,
) -> io::Result<()> {
    let updated = update_note_in_repo(db, repo_key, id, |n| {
        n.status = NoteStatus::Open;
    });
    if !updated {
        eprintln!("diffuse: note '{id}' not found");
        process::exit(1);
    }
    save_notes(notes_path, db)?;
    println!("Reopened note {id} 💬");
    Ok(())
}

pub fn handle_status(
    id: &str,
    status: &str,
    db: &mut NotesDB,
    notes_path: &Path,
    repo_key: &str,
) -> io::Result<()> {
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
    let updated = update_note_in_repo(db, repo_key, id, |n| {
        n.status = parsed_status;
    });
    if !updated {
        eprintln!("diffuse: note '{id}' not found");
        process::exit(1);
    }
    save_notes(notes_path, db)?;
    println!(
        "Updated note {id} status to {} {:?}",
        parsed_status.glyph(),
        parsed_status
    );
    Ok(())
}
