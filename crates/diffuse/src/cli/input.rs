//! Input deserialization and review target preparation.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use diffuse::{
    git::load_git_repo_with_notes,
    notes::{
        add_note, default_notes_path, load_notes, remove_notes_in_range, reply_to_note_text,
        save_notes, update_note_in_repo,
    },
    prelude::*,
};

use super::args::Args;

#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Input {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub note: Option<ReviewNote>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub start: Option<u32>,
    #[serde(default)]
    pub end: Option<u32>,
    #[serde(default)]
    pub diff: String,
    #[serde(default)]
    pub new_content: Option<HashMap<String, String>>,
    #[serde(default)]
    pub old_content: Option<HashMap<String, String>>,
    #[serde(default)]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub base_args: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default, alias = "cached")]
    pub staged: bool,
    #[serde(default)]
    pub context: Option<usize>,
    #[allow(dead_code)]
    #[serde(default)]
    pub notes: Option<NotesDB>,
    #[serde(default)]
    pub notes_file: Option<String>,
    #[serde(default)]
    pub repo_key: Option<String>,
    #[serde(default)]
    pub rev_key: Option<String>,
    #[serde(default)]
    pub token_product_cap: Option<usize>,
    #[serde(default)]
    pub cluster_budget: Option<usize>,
    #[serde(default)]
    pub cluster_bandwidth_min: Option<usize>,
    #[serde(default)]
    pub cluster_bandwidth_slack: Option<usize>,
    #[serde(default)]
    pub note_id: Option<String>,
    #[serde(default)]
    pub status: Option<NoteStatus>,
    #[serde(default)]
    pub reply_text: Option<String>,
    #[serde(default)]
    pub show_resolved: Option<bool>,
    #[allow(dead_code)]
    #[serde(default)]
    pub show_source: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum InputPayload {
    Batch(Vec<Input>),
    Single(Box<Input>),
}

pub fn target_for_input(input: &Input, args: &Args) -> ReviewTarget {
    let repo_key = args.repo_key.clone().or_else(|| input.repo_key.clone());
    let rev_key = args.rev_key.clone().or_else(|| input.rev_key.clone());
    let repo = if let Some(ref root) = input.repo_root {
        Repo::from_root(Path::new(root)).with_key_opt(repo_key)
    } else if let Some(key) = repo_key {
        Repo::from_key(key)
    } else {
        Repo::default()
    };
    ReviewTarget::from_repo_args(repo, input.base_args.clone(), input.staged)
        .with_rev_key_opt(rev_key)
}

pub fn process_input(input: Input, args: &Args) -> PipelineOutput {
    let explicit_notes_path = args.notes_file.as_ref().or(input.notes_file.as_ref());
    let notes_path = match (explicit_notes_path, input.repo_root.as_deref()) {
        (Some(p), _) => Some(PathBuf::from(p)),
        (None, Some(_)) => Some(default_notes_path()),
        (None, None) => None,
    };

    let target = target_for_input(&input, args);
    let target_repo_key = target.repo_key().map(ToString::to_string);
    let target_rev_key = target.rev_key().map(ToString::to_string);

    if let Some(ref path) = notes_path
        && let Some(repo_k) = target_repo_key.as_deref()
    {
        let rev_k = target_rev_key.as_deref().unwrap_or("");
        if let Some("addNote") = input.action.as_deref()
            && let Some(n) = input.note.clone()
        {
            let mut db = load_notes(path);
            add_note(&mut db, repo_k, rev_k, n);
            let _ = save_notes(path, &db);
        }

        if let Some("removeNotes") = input.action.as_deref()
            && let (Some(f), Some(st), Some(en)) = (input.file.as_deref(), input.start, input.end)
        {
            let mut db = load_notes(path);
            remove_notes_in_range(&mut db, repo_k, rev_k, f, st, en);
            let _ = save_notes(path, &db);
        }

        if let Some("setNoteStatus") = input.action.as_deref()
            && let (Some(id), Some(st)) = (input.note_id.as_deref(), input.status)
        {
            let mut db = load_notes(path);
            let _ = update_note_in_repo(&mut db, repo_k, id, |n| {
                n.status = st;
            });
            let _ = save_notes(path, &db);
        }

        if let Some("resolveNote") = input.action.as_deref()
            && let Some(id) = input.note_id.as_deref()
        {
            let mut db = load_notes(path);
            let _ = update_note_in_repo(&mut db, repo_k, id, |n| {
                n.status = NoteStatus::Resolved;
            });
            let _ = save_notes(path, &db);
        }

        if let Some("reopenNote") = input.action.as_deref()
            && let Some(id) = input.note_id.as_deref()
        {
            let mut db = load_notes(path);
            let _ = update_note_in_repo(&mut db, repo_k, id, |n| {
                n.status = NoteStatus::Open;
            });
            let _ = save_notes(path, &db);
        }

        if let Some("replyNote") = input.action.as_deref()
            && let (Some(id), Some(reply)) = (input.note_id.as_deref(), input.reply_text.as_deref())
        {
            let mut db = load_notes(path);
            let _ = update_note_in_repo(&mut db, repo_k, id, |n| {
                n.text = reply_to_note_text(&n.text, reply);
                if let Some(s) = input.status {
                    n.status = s;
                }
            });
            let _ = save_notes(path, &db);
        }
    }

    let loaded_db = notes_path.as_ref().map(|p| load_notes(p));
    let visual = input.context.unwrap_or(args.context);
    let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
    let budget = input.cluster_budget.unwrap_or(args.cluster_budget);
    let bw_min = input
        .cluster_bandwidth_min
        .unwrap_or(args.cluster_bandwidth_min);
    let bw_slack = input
        .cluster_bandwidth_slack
        .unwrap_or(args.cluster_bandwidth_slack);
    let show_resolved = input.show_resolved.unwrap_or(args.resolved || args.all);
    let opts = PipelineOptions::default()
        .with_notes(
            loaded_db.clone(),
            target_repo_key.clone(),
            target_rev_key.clone(),
        )
        .with_show_resolved(show_resolved)
        .with_visual_context(visual)
        .with_expand_lines(args.expand_lines)
        .with_token_product_cap(cap)
        .with_cluster_product_cap(budget)
        .with_cluster_bandwidth_min(bw_min)
        .with_cluster_bandwidth_slack(bw_slack);

    if let Some(ref repo_root) = input.repo_root
        && let Ok(git_res) = load_git_repo_with_notes(
            Path::new(repo_root),
            &input.base_args,
            &input.paths,
            input.staged,
            ENGINE_CONTEXT,
            loaded_db.as_ref(),
            target_repo_key.as_deref(),
            target_rev_key.as_deref(),
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
