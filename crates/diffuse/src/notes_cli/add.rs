//! Handler for adding and relocating review notes on diff rows.

use std::{
    collections::HashMap,
    io::{self, IsTerminal, Read},
    path::Path,
    process,
};

use diffuse::{
    git::{ENGINE_CONTEXT, is_inside_work_tree, load_git_repo},
    notes::{DiffCoord, LineHash, NoteLocation, add_note, generate_note_id, now_millis},
    prelude::*,
};

use super::span::{
    check_span_bounds, missing_file_exit, parse_line_span, provisional_ctx_row, relocate_span,
};
use crate::{Args, NotesArgs};

/// A reviewed diff with its hydrated file snapshots, whether loaded from a
/// git work tree or from an explicit `--diff` file plus snapshot dirs.
struct FileReview {
    diff: String,
    new_content: HashMap<String, String>,
    old_content: HashMap<String, String>,
}

/// Load an explicit `--diff` file with its `--old-dir` / `--new-dir`
/// snapshots for `file_path`. Snapshot files absent from the given dirs stay
/// absent (exactly like a file missing from a git diff); the raw diff file
/// itself must read and parse, and a corrupt hunk header is refused with the
/// input-error status, matching non-interactive diff streaming.
fn load_file_review(
    diff_path: &str,
    file_path: &str,
    old_dir: Option<&Path>,
    new_dir: Option<&Path>,
) -> FileReview {
    let diff = std::fs::read_to_string(diff_path).unwrap_or_else(|e| {
        eprintln!("diffuse: cannot load diff file '{diff_path}': {e}");
        process::exit(1);
    });
    // The old-side snapshot is looked up by the diff's old path so renames
    // resolve; when the diff does not cover the file, fall back to the new
    // path so the absence surfaces downstream as "not part of the diff".
    let old_lookup = diffuse::core::parse::parse_diff(&diff)
        .unwrap_or_else(|e| {
            eprintln!("diffuse: corrupt diff: {e}");
            process::exit(2);
        })
        .into_iter()
        .find(|f| f.path == file_path || f.old_path.as_deref() == Some(file_path))
        .and_then(|f| f.old_path.or(Some(f.path)))
        .unwrap_or_else(|| file_path.to_string());
    let mut new_content = HashMap::new();
    if let Some(dir) = new_dir
        && let Ok(c) = std::fs::read_to_string(dir.join(file_path))
    {
        new_content.insert(file_path.to_string(), c);
    }
    let mut old_content = HashMap::new();
    if let Some(dir) = old_dir
        && let Ok(c) = std::fs::read_to_string(dir.join(&old_lookup))
    {
        old_content.insert(old_lookup.clone(), c.clone());
        old_content.insert(file_path.to_string(), c);
    }
    FileReview {
        diff,
        new_content,
        old_content,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_add(
    target: &Option<String>,
    old: &Option<String>,
    new: &Option<String>,
    message: &Option<String>,
    text: &Option<String>,
    status: &str,
    json: bool,
    rev: &Option<String>,
    notes_args: &NotesArgs,
    args: &Args,
    db: &mut NotesDB,
    notes_path: &Path,
    repo: Repo,
    repo_key: &str,
    current_dir: &Path,
) -> io::Result<()> {
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
    // A both-side anchor locates one set of rows: the spans must be equally long.
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

    let effective_revs = if let Some(r) = rev.as_ref().or(notes_args.rev.as_ref()) {
        vec![r.clone()]
    } else if !notes_args.revision.git_revs.is_empty() {
        notes_args.revision.git_revs.clone()
    } else {
        args.revision.git_revs.clone()
    };
    let add_target = ReviewTarget::from_repo_args(repo, effective_revs.clone(), args.staged)
        .with_rev_key_opt(args.rev_key.clone());
    let add_rev_key = add_target.rev_key().unwrap_or("").to_string();

    // 4. The note stores the diff rows it was created against. When the file is
    //    part of the reviewed diff, the pipeline runs with the provisional note
    //    registered (so gap synthesis includes its span) and the note is relocated
    //    onto the produced rows. Files outside the reviewed diff fall back to plain
    //    rows read from the working-tree content; the note then displays on the
    //    file banner.
    //
    //    Review input precedence: an explicit `--diff` file always wins (even
    //    inside a work tree, mirroring diff-stream routing), hydrated from
    //    `--old-dir` / `--new-dir` when given; otherwise the git work tree
    //    provides the reviewed diff; otherwise there is no reviewed diff.
    let reviewed = if let Some(diff_path) = args.diff.as_deref() {
        Some(load_file_review(
            diff_path,
            &file_path,
            args.old_dir.as_deref(),
            args.new_dir.as_deref(),
        ))
    } else if is_inside_work_tree(current_dir) {
        Some(
            load_git_repo(
                current_dir,
                &effective_revs,
                std::slice::from_ref(&file_path),
                args.staged,
                ENGINE_CONTEXT,
            )
            .map(|g| FileReview {
                diff: g.diff,
                new_content: g.new_content,
                old_content: g.old_content,
            })
            .unwrap_or_else(|e| {
                eprintln!("diffuse: cannot load diff: {e}");
                process::exit(1);
            }),
        )
    } else {
        None
    };
    if reviewed.is_none() && new_span.is_none() {
        eprintln!("diffuse: --old coordinates require the file to be part of a reviewed git diff");
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
            if args.diff.is_some() {
                eprintln!(
                    "diffuse: no old-side content for '{file_path}': pass --old-dir <DIR> holding the pre-image snapshot (and check the file is covered by --diff)"
                );
                process::exit(1);
            }
            missing_file_exit(&file_path);
        }
        (_, _, _) => {
            // Not part of the reviewed diff: fall back to the snapshot
            // dirs, then the working-tree content (new side only).
            let from_snapshot = args
                .new_dir
                .as_deref()
                .and_then(|dir| std::fs::read_to_string(dir.join(&file_path)).ok());
            match from_snapshot
                .or_else(|| std::fs::read_to_string(current_dir.join(&file_path)).ok())
            {
                Some(c) => (c, true),
                None => missing_file_exit(&file_path),
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
    let window: Vec<OutputRow> = if let Some(review) =
        reviewed_content.is_some().then_some(reviewed).flatten()
    {
        let mut prov_db = db.clone();
        let prov_rev = add_target.rev_key().unwrap_or("");
        add_note(&mut prov_db, repo_key, prov_rev, provisional_note);
        let opts = PipelineOptions::default().with_notes(
            Some(prov_db),
            Some(repo_key.to_string()),
            Some(prov_rev.to_string()),
        );
        let out = run_pipeline_full(
            &review.diff,
            Some(review.new_content),
            Some(review.old_content),
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
                coord: DiffCoord::from_row(&window[0]).expect("relocated row carries coordinates"),
            }
        } else {
            NoteLocation::Range {
                start: DiffCoord::from_row(window.first().expect("non-empty relocation window"))
                    .expect("relocated row carries coordinates"),
                end: DiffCoord::from_row(window.last().expect("non-empty relocation window"))
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
    add_note(db, repo_key, &add_rev_key, final_note.clone());
    save_notes(notes_path, db)?;

    if json {
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

    Ok(())
}
