//! CLI handlers for the `diffuse notes` subcommand suite.

use std::{env, io, path::PathBuf, process};

use diffuse::{git::is_inside_work_tree, prelude::*};

use crate::{Args, NotesAction, NotesArgs};

pub mod add;
pub mod format;
pub mod list;
pub mod mutate;
pub mod span;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use format::{format_note_text, format_rows_with_context};
#[cfg(test)]
pub(crate) use span::relocate_span;

pub fn run_notes_command(notes_args: &NotesArgs, args: &Args) -> io::Result<()> {
    let notes_path = args
        .notes_file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_notes_path);
    let mut db = load_notes(&notes_path);

    let current_dir = env::current_dir()?;
    let repo = if is_inside_work_tree(&current_dir) {
        Repo::from_root(&current_dir).with_key_opt(args.repo_key.clone())
    } else if let Some(key) = args.repo_key.clone() {
        Repo::from_key(key)
    } else {
        Repo::default()
    };
    let target =
        ReviewTarget::from_repo_args(repo.clone(), args.revision.git_revs.clone(), args.staged)
            .with_rev_key_opt(args.rev_key.clone());
    let Some(repo_key) = target.repo_key() else {
        eprintln!(
            "diffuse: not in a git repository; specify --repo-key <KEY> to manage notes outside a repository"
        );
        process::exit(1);
    };

    match &notes_args.action {
        None | Some(NotesAction::List { .. }) => list::handle_list(
            notes_args.action.as_ref(),
            notes_args,
            args,
            &db,
            repo,
            repo_key,
            &current_dir,
        ),
        Some(NotesAction::Add {
            target,
            old,
            new,
            message,
            text,
            status,
            json,
            rev,
        }) => add::handle_add(
            target,
            old,
            new,
            message,
            text,
            status,
            *json,
            rev,
            notes_args,
            args,
            &mut db,
            &notes_path,
            repo,
            repo_key,
            &current_dir,
        ),
        Some(NotesAction::Show {
            id,
            show_source,
            json,
            markdown,
        }) => mutate::handle_show(id, *show_source, *json, *markdown, args, &db, repo_key),
        Some(NotesAction::Reply {
            id,
            message,
            text,
            status,
            resolve,
        }) => mutate::handle_reply(
            id,
            message,
            text,
            status,
            *resolve,
            &mut db,
            &notes_path,
            repo_key,
        ),
        Some(NotesAction::Progress { id }) => {
            mutate::handle_progress(id, &mut db, &notes_path, repo_key)
        }
        Some(NotesAction::Resolve { id }) => {
            mutate::handle_resolve(id, &mut db, &notes_path, repo_key)
        }
        Some(NotesAction::Open { id }) | Some(NotesAction::Reopen { id }) => {
            mutate::handle_open(id, &mut db, &notes_path, repo_key)
        }
        Some(NotesAction::Status { id, status }) => {
            mutate::handle_status(id, status, &mut db, &notes_path, repo_key)
        }
    }
}
