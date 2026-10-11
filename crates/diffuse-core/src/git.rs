//! Git plumbing, ref resolution, and repo status loader for diffuse.
//!
//! Port of TypeScript's `resolveRefs`, `loadReview`, and `rustBackendLoad`.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    process::Command,
    string::String,
    sync::Arc,
    thread,
};

/// Fixed unified-diff context width used when generating diffs from git (`-U`).
///
/// This is the *engine* band: the DP/shift engine needs enough context around
/// changes to slide hunks and anchor against syntax. It is separate from the
/// *visual* context the user controls via `-U/--context` — the pipeline's last
/// step normalizes the engine's context down (or up) to the requested visual
/// width.
pub const ENGINE_CONTEXT: usize = 4;

/// Default *visual* context lines (`-U/--context` default). Displayed context,
/// applied by the pipeline's final normalization step.
pub const DEFAULT_CONTEXT_LINES: usize = 4;

/// Default number of lines to expand up and down when activating a hidden gap
/// separator.
pub const DEFAULT_EXPAND_LINES: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPair {
    pub worktree_new: bool,
    pub new_ref: Option<String>,
    pub old_ref: Option<String>,
}

/// Run a git command in the target directory and return stdout if successful.
pub fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// Check if the directory is inside a git repository or worktree.
pub fn is_inside_work_tree(cwd: &Path) -> bool {
    if gix::discover(cwd).is_ok() {
        return true;
    }
    run_git(cwd, &["rev-parse", "--is-inside-work-tree"]).is_some()
}

/// Get current repository branch name via native gix or git fallback.
pub fn get_current_branch(cwd: &Path) -> String {
    if let Ok(repo) = gix::open(cwd)
        && let Ok(Some(head)) = repo.head_name()
    {
        return head.shorten().to_string();
    }
    run_git(cwd, &["branch", "--show-current"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Resolve old/new git refs from base args (e.g. `main..feature`, `HEAD^!`,
/// `main...feature`).
pub fn resolve_refs(cwd: &Path, base_args: &[String], staged: bool) -> RefPair {
    let refs: Vec<&str> = base_args
        .iter()
        .map(String::as_str)
        .filter(|t| !t.starts_with('-'))
        .collect();

    if refs.len() == 1 {
        let r = refs[0];
        if let Some(c) = r.strip_suffix("^!") {
            return RefPair {
                worktree_new: false,
                new_ref: Some(c.to_string()),
                old_ref: Some(format!("{c}^")),
            };
        }
        if let Some((old_r, new_r)) = r.split_once("..")
            && !old_r.ends_with('.')
            && !new_r.starts_with('.')
        {
            return RefPair {
                worktree_new: false,
                new_ref: Some(new_r.to_string()),
                old_ref: Some(old_r.to_string()),
            };
        }
        if let Some((old_r, new_r)) = r.split_once("...") {
            let mb = run_git(cwd, &["merge-base", old_r, new_r])
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| old_r.to_string());
            return RefPair {
                worktree_new: false,
                new_ref: Some(new_r.to_string()),
                old_ref: Some(mb),
            };
        }
        if staged {
            return RefPair {
                worktree_new: false,
                new_ref: Some(String::new()),
                old_ref: Some(r.to_string()),
            };
        }
        return RefPair {
            worktree_new: true,
            new_ref: None,
            old_ref: Some(r.to_string()),
        };
    }

    if refs.len() == 2 {
        return RefPair {
            worktree_new: false,
            new_ref: Some(refs[1].to_string()),
            old_ref: Some(refs[0].to_string()),
        };
    }

    RefPair {
        worktree_new: !staged,
        new_ref: if staged { Some(String::new()) } else { None },
        old_ref: if staged {
            Some("HEAD".to_string())
        } else {
            None
        },
    }
}

fn rev_to_short_hash(cwd: &Path, rev: &str) -> Option<String> {
    if rev.is_empty() {
        return None;
    }
    run_git(cwd, &["rev-parse", "--short", &format!("{rev}^{{commit}}")])
        .map(|s| s.trim().to_string())
}

/// Resolves commit range short hashes from base args (e.g. `main..feature` ->
/// `1a2b3c4..5d6e7f8`). Returns `None` if the diff is not a commit-to-commit
/// range (e.g. diff against working tree).
pub fn resolve_commit_range(cwd: &Path, base_args: &[String]) -> Option<String> {
    let refs: Vec<&str> = base_args
        .iter()
        .map(String::as_str)
        .filter(|t| !t.starts_with('-'))
        .collect();

    if refs.len() == 1 {
        let r = refs[0];
        if let Some(c) = r.strip_suffix("^!") {
            let h_old = rev_to_short_hash(cwd, &format!("{c}^"))?;
            let h_new = rev_to_short_hash(cwd, c)?;
            return Some(format!("{h_old}..{h_new}"));
        }
        if let Some((old_r, new_r)) = r.split_once("...") {
            let left = if old_r.is_empty() { "HEAD" } else { old_r };
            let right = if new_r.is_empty() { "HEAD" } else { new_r };
            let h_left = rev_to_short_hash(cwd, left)?;
            let h_right = rev_to_short_hash(cwd, right)?;
            return Some(format!("{h_left}...{h_right}"));
        }
        if let Some((old_r, new_r)) = r.split_once("..")
            && !old_r.ends_with('.')
            && !new_r.starts_with('.')
        {
            let left = if old_r.is_empty() { "HEAD" } else { old_r };
            let right = if new_r.is_empty() { "HEAD" } else { new_r };
            let h_left = rev_to_short_hash(cwd, left)?;
            let h_right = rev_to_short_hash(cwd, right)?;
            return Some(format!("{h_left}..{h_right}"));
        }
        return None;
    }

    if refs.len() == 2 {
        let h1 = rev_to_short_hash(cwd, refs[0])?;
        let h2 = rev_to_short_hash(cwd, refs[1])?;
        return Some(format!("{h1}..{h2}"));
    }

    None
}

#[derive(Debug, Clone)]
pub struct GitFileItem {
    pub path: String,
    pub status: String,
    pub old_path: Option<String>,
    pub new_content: Option<String>,
    pub old_content: Option<String>,
    pub no_content: bool,
}

#[derive(Debug)]
pub struct LoadGitResult {
    pub diff: String,
    pub new_content: HashMap<String, String>,
    pub old_content: HashMap<String, String>,
    pub branch: String,
    pub untracked_skipped: Vec<String>,
    pub commit_range: Option<String>,
}

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::Stdio,
};

/// Batch-load file blob contents using a single `git cat-file --batch` process.
pub fn load_git_blobs_batch(repo_root: &Path, specs: &[String]) -> HashMap<String, String> {
    if specs.is_empty() {
        return HashMap::new();
    }

    let mut child = match Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    let mut stdin = child.stdin.take().expect("child stdin");
    let mut input_data = Vec::new();
    for spec in specs {
        input_data.extend_from_slice(spec.as_bytes());
        input_data.push(b'\n');
    }

    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&input_data);
        drop(stdin);
    });

    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    let mut results = HashMap::with_capacity(specs.len());

    for spec in specs {
        let mut header = String::new();
        if stdout.read_line(&mut header).is_err() || header.is_empty() {
            break;
        }
        let header_str = header.trim_end();
        if header_str.ends_with("missing") {
            continue;
        }
        let parts: Vec<&str> = header_str.split_whitespace().collect();
        if parts.len() == 3
            && let Ok(size) = parts[2].parse::<usize>()
        {
            let mut buf = vec![0u8; size];
            if stdout.read_exact(&mut buf).is_ok() {
                let mut newline = [0u8; 1];
                let _ = stdout.read_exact(&mut newline);
                if let Ok(text) = String::from_utf8(buf) {
                    results.insert(spec.clone(), text);
                }
            }
        }
    }

    let _ = writer.join();
    let _ = child.wait();
    results
}

/// Load file blob contents directly using the native `gix` object database,
/// falling back to `load_git_blobs_batch` on error.
pub fn load_git_blobs_native(repo_root: &Path, specs: &[String]) -> HashMap<String, String> {
    if specs.len() > 1 {
        return load_git_blobs_batch(repo_root, specs);
    }
    let Ok(repo) = gix::open(repo_root) else {
        return load_git_blobs_batch(repo_root, specs);
    };

    let mut results = HashMap::with_capacity(specs.len());
    let mut tree_cache: HashMap<String, gix::Tree<'_>> = HashMap::new();

    for spec in specs {
        let Some((ref_str, path)) = spec.split_once(':') else {
            continue;
        };

        let tree_opt = if let Some(t) = tree_cache.get(ref_str) {
            Some(t.clone())
        } else {
            let res: Result<gix::Tree<'_>, Box<dyn std::error::Error>> = (|| {
                let id = repo.rev_parse_single(ref_str)?;
                let obj = repo.find_object(id)?;
                let commit = obj.peel_to_commit()?;
                let tree = commit.tree()?;
                Ok(tree)
            })();
            match res {
                Ok(tree) => {
                    tree_cache.insert(ref_str.to_string(), tree.clone());
                    Some(tree)
                }
                Err(_) => None,
            }
        };

        if let Some(tree) = tree_opt
            && let Ok(Some(entry)) = tree.lookup_entry_by_path(path)
            && let Ok(obj) = entry.object()
            && let Ok(text) = String::from_utf8(obj.data.to_vec())
        {
            results.insert(spec.clone(), text);
        }
    }

    // If any specs were not found in the gix ODB (e.g. unreferenced index
    // blobs), fill in missing ones via batch loader
    if results.len() < specs.len() {
        let missing: Vec<String> = specs
            .iter()
            .filter(|s| !results.contains_key(*s))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let fallback_blobs = load_git_blobs_batch(repo_root, &missing);
            results.extend(fallback_blobs);
        }
    }

    results
}

///// Progressive streaming producer for git diff and blob retrieval.
/// Directly emits `FileJob`s to `StreamingProducer` as each file's diff chunk
/// is encountered.
#[expect(clippy::too_many_arguments)]
#[tracing::instrument(level = "trace", skip(paths, producer, meta_tx, notes_db), fields(repo_root = %repo_root.display(), base_args = ?base_args, path_count = paths.len(), staged, context))]
pub fn stream_git_repo_to_producer(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
    producer: crate::streaming::StreamingProducer,
    meta_tx: &std::sync::mpsc::Sender<Result<crate::streaming::GitRepoMeta, String>>,
    notes_db: Option<&crate::notes::NotesDB>,
    repo: Option<&str>,
) {
    if !is_inside_work_tree(repo_root) {
        let _ = meta_tx.send(Err(format!(
            "not a git repository (or any of the parent directories): {}",
            repo_root.display()
        )));
        producer.finish();
        return;
    }

    let branch = get_current_branch(repo_root);
    tracing::info!(branch = %branch, "Streaming git repository diff");

    let mut untracked_skipped = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let refs = resolve_refs(repo_root, base_args, staged);

    let mut diff_cmd = vec![
        "diff".to_string(),
        format!("-U{context}"),
        "-M".to_string(),
        "--no-color".to_string(),
    ];
    if staged {
        diff_cmd.push("--staged".to_string());
    }
    diff_cmd.extend(base_args.iter().cloned());
    diff_cmd.push("--".to_string());
    diff_cmd.extend(paths.iter().cloned());

    let mut child = match Command::new("git")
        .args(&diff_cmd)
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = meta_tx.send(Err(format!("failed to spawn git diff: {e}")));
            producer.finish();
            return;
        }
    };

    let stdout = BufReader::new(child.stdout.take().expect("git diff stdout"));
    let mut current_chunk = String::new();
    let mut file_idx = 0;

    let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");

    let repo_gix = gix::open(repo_root).ok();
    let mut tree_cache: HashMap<String, Option<gix::Tree<'_>>> = HashMap::new();

    let mut get_blob_data = |ref_str: &str, path: &str| -> (Option<String>, Option<usize>) {
        if let Some(ref repo) = repo_gix {
            let tree_opt = tree_cache.entry(ref_str.to_string()).or_insert_with(|| {
                let id = repo.rev_parse_single(ref_str).ok()?;
                let obj = repo.find_object(id).ok()?;
                let commit = obj.peel_to_commit().ok()?;
                commit.tree().ok()
            });
            if let Some(tree) = tree_opt.as_ref() {
                if let Ok(Some(entry)) = tree.lookup_entry_by_path(path)
                    && let Ok(obj) = entry.object()
                {
                    let size = obj.data.len();
                    let text = std::str::from_utf8(&obj.data).ok().map(|s| s.to_string());
                    return (text, Some(size));
                }
                return (None, None);
            }
        }
        let spec = format!("{ref_str}:{path}");
        let blobs = load_git_blobs_batch(repo_root, std::slice::from_ref(&spec));
        let text = blobs.get(&spec).cloned();
        let size = text.as_ref().map(|t| t.len()).or_else(|| {
            run_git(repo_root, &["cat-file", "-s", &spec])
                .and_then(|s| s.trim().parse::<usize>().ok())
        });
        (text, size)
    };

    let mut emit_chunk = |chunk: &str, idx: &mut usize| {
        if chunk.trim().is_empty() {
            return;
        }
        let parsed_files = crate::parse::parse_diff(chunk);
        for mut f in parsed_files {
            seen_paths.insert(f.path.clone());
            if let Some(ref op) = f.old_path {
                seen_paths.insert(op.clone());
            }
            let mut new_c = None;
            let mut old_c = None;

            if f.status != crate::parse::FileStatus::Deleted {
                if refs.worktree_new {
                    let new_abs = repo_root.join(&f.path);
                    if let Ok(c) = fs::read_to_string(&new_abs) {
                        f.new_bytes = Some(c.len());
                        new_c = Some(c);
                    } else if let Ok(meta) = fs::metadata(&new_abs) {
                        f.new_bytes = Some(meta.len() as usize);
                    }
                } else if let Some(new_ref) = &refs.new_ref {
                    let (c, size) = get_blob_data(new_ref, &f.path);
                    new_c = c;
                    if size.is_some() {
                        f.new_bytes = size;
                    }
                }
            } else {
                f.new_bytes = None;
            }

            let old_lookup_path = f.old_path.as_deref().unwrap_or(&f.path);
            if f.status != crate::parse::FileStatus::Added
                && f.status != crate::parse::FileStatus::Untracked
            {
                let (c, size) = get_blob_data(old_tree_ref, old_lookup_path);
                if new_c.is_none() && f.status != crate::parse::FileStatus::Deleted {
                    new_c = c.clone();
                }
                old_c = c;
                if size.is_some() {
                    f.old_bytes = size;
                }
            } else {
                f.old_bytes = None;
            }

            let fallback = if old_c.is_none() && f.status != crate::parse::FileStatus::Added {
                Some(chunk.to_string())
            } else {
                None
            };

            producer.push(crate::streaming::FileJob {
                idx: *idx,
                file: f,
                new_content: new_c.map(Into::into),
                old_content: old_c.map(Into::into),
                diff_fallback: fallback.map(Into::into),
            });
            *idx += 1;
        }
    };

    let mut line_buf = String::with_capacity(512);
    let mut reader = stdout;
    while let Ok(n) = reader.read_line(&mut line_buf) {
        if n == 0 {
            break;
        }
        if line_buf.starts_with("diff --git ") && !current_chunk.is_empty() {
            emit_chunk(&current_chunk, &mut file_idx);
            current_chunk.clear();
        }
        current_chunk.push_str(&line_buf);
        line_buf.clear();
    }

    if !current_chunk.is_empty() {
        emit_chunk(&current_chunk, &mut file_idx);
    }

    let _ = child.wait();

    // Untracked files — only relevant when the new side is the working tree.
    // A fixed commit range (worktree_new == false) diffs two historical refs;
    // untracked working-tree files must never appear in those results.
    if !staged && refs.worktree_new {
        let mut ls_args = vec!["ls-files", "--others", "--exclude-standard"];
        if !paths.is_empty() {
            ls_args.push("--");
            ls_args.extend(paths.iter().map(String::as_str));
        }
        if let Some(ut) = run_git(repo_root, &ls_args) {
            for rel in ut.lines().filter(|s| !s.is_empty()) {
                let abs = repo_root.join(rel);
                let content = match fs::read_to_string(&abs) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if content.contains('\0') {
                    untracked_skipped.push(rel.to_string());
                    continue;
                }
                let line_count = content.lines().count();
                let mut synthetic_diff = format!(
                    "diff --git a/{rel} b/{rel}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{line_count} @@\n"
                );
                for line in content.lines() {
                    synthetic_diff.push('+');
                    synthetic_diff.push_str(line);
                    synthetic_diff.push('\n');
                }
                let parsed_files = crate::parse::parse_diff(&synthetic_diff);
                let synthetic_arc = Arc::new(synthetic_diff);
                for mut f in parsed_files {
                    seen_paths.insert(f.path.clone());
                    f.status = crate::parse::FileStatus::Untracked;
                    producer.push(crate::streaming::FileJob {
                        idx: file_idx,
                        file: f,
                        new_content: Some(content.clone().into()),
                        old_content: None,
                        diff_fallback: Some(Arc::clone(&synthetic_arc)),
                    });
                    file_idx += 1;
                }
            }
        }
    }

    // Unmodified files carrying review notes.
    if let Some(db) = notes_db {
        let note_files = crate::notes::note_files_for_repo(db, repo);
        for file_path in note_files {
            if seen_paths.contains(&file_path) {
                continue;
            }
            if !paths.is_empty()
                && !paths.iter().any(|p| {
                    let clean = p.strip_suffix('/').unwrap_or(p);
                    file_path == clean || file_path.starts_with(&format!("{clean}/"))
                })
            {
                continue;
            }

            let (content, size) = if refs.worktree_new {
                let abs = repo_root.join(&file_path);
                if let Ok(c) = fs::read_to_string(&abs) {
                    let len = c.len();
                    (Some(c), Some(len))
                } else {
                    (None, None)
                }
            } else if let Some(ref new_ref) = refs.new_ref {
                get_blob_data(new_ref, &file_path)
            } else {
                get_blob_data(old_tree_ref, &file_path)
            };

            if let Some(c) = content {
                let file_lines = crate::pipeline::split_content_lines(&c);
                let file_notes = crate::notes::query_notes_for_file(db, repo, &file_path);
                if !file_notes
                    .iter()
                    .any(|n| crate::notes::note_anchors_in_file(n, &file_lines))
                {
                    continue;
                }
                let mut f = crate::parse::DiffFile::new(&file_path);
                f.status = crate::parse::FileStatus::Unmodified;
                f.new_bytes = size;
                f.old_bytes = size;
                let c_arc = Arc::new(c);
                producer.push(crate::streaming::FileJob {
                    idx: file_idx,
                    file: f,
                    new_content: Some(Arc::clone(&c_arc)),
                    old_content: Some(c_arc),
                    diff_fallback: None,
                });
                seen_paths.insert(file_path);
                file_idx += 1;
            }
        }
    }

    let commit_range = resolve_commit_range(repo_root, base_args);
    let _ = meta_tx.send(Ok(crate::streaming::GitRepoMeta {
        branch,
        untracked_skipped,
        commit_range,
    }));
    producer.finish();
}

#[tracing::instrument(level = "trace", ret, err, skip(paths, notes_db), fields(repo_root = %repo_root.display(), base_args = ?base_args, path_count = paths.len(), staged, context))]
pub fn load_git_repo_with_notes(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
    notes_db: Option<&crate::notes::NotesDB>,
    repo: Option<&str>,
) -> Result<LoadGitResult, String> {
    if !is_inside_work_tree(repo_root) {
        return Err(format!(
            "not a git repository (or any of the parent directories): {}",
            repo_root.display()
        ));
    }

    let branch = get_current_branch(repo_root);

    let mut diff_cmd = vec![
        "diff".to_string(),
        format!("-U{context}"),
        "-M".to_string(),
        "--no-color".to_string(),
    ];
    if staged {
        diff_cmd.push("--staged".to_string());
    }
    diff_cmd.extend(base_args.iter().cloned());
    diff_cmd.push("--".to_string());
    diff_cmd.extend(paths.iter().cloned());

    let diff_args: Vec<&str> = diff_cmd.iter().map(String::as_str).collect();
    let mut diff = run_git(repo_root, &diff_args).unwrap_or_default();

    let mut name_cmd = vec![
        "diff".to_string(),
        "-M".to_string(),
        "--name-status".to_string(),
    ];
    if staged {
        name_cmd.push("--staged".to_string());
    }
    name_cmd.extend(base_args.iter().cloned());
    name_cmd.push("--".to_string());
    name_cmd.extend(paths.iter().cloned());
    let name_args: Vec<&str> = name_cmd.iter().map(String::as_str).collect();
    let name_status = run_git(repo_root, &name_args).unwrap_or_default();

    let refs = resolve_refs(repo_root, base_args, staged);
    let mut files: Vec<GitFileItem> = Vec::new();

    for l in name_status.lines() {
        let parts: Vec<&str> = l.split('\t').collect();
        if parts.is_empty() || parts[0].is_empty() {
            continue;
        }
        let st = parts[0];
        let mut status = "modified";
        let mut path = parts.get(1).copied().unwrap_or("").to_string();
        let mut old_path = None;
        if st.starts_with('A') {
            status = "added";
        } else if st.starts_with('D') {
            status = "deleted";
        } else if st.starts_with('R') {
            status = "renamed";
            old_path = Some(path);
            path = parts.get(2).copied().unwrap_or("").to_string();
        }
        files.push(GitFileItem {
            path,
            status: status.to_string(),
            old_path,
            new_content: None,
            old_content: None,
            no_content: false,
        });
    }

    let mut untracked_skipped: Vec<String> = Vec::new();
    // Untracked files — only relevant when the new side is the working tree.
    // A fixed commit range (worktree_new == false) diffs two historical refs;
    // untracked working-tree files must never appear in those results.
    if !staged && refs.worktree_new {
        let mut ls_args = vec!["ls-files", "--others", "--exclude-standard"];
        if !paths.is_empty() {
            ls_args.push("--");
            ls_args.extend(paths.iter().map(String::as_str));
        }
        let ut_opt = run_git(repo_root, &ls_args);
        if let Some(ut) = ut_opt {
            for rel in ut.lines().filter(|s| !s.is_empty()) {
                let abs = repo_root.join(rel);
                let content = match fs::read_to_string(&abs) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if content.contains('\0') {
                    untracked_skipped.push(rel.to_string());
                    continue;
                }
                let line_count = content.lines().count();
                if !diff.is_empty() && !diff.ends_with('\n') {
                    diff.push('\n');
                }
                diff.push_str(&format!(
                    "diff --git a/{rel} b/{rel}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{line_count} @@\n"
                ));
                for line in content.lines() {
                    diff.push('+');
                    diff.push_str(line);
                    diff.push('\n');
                }
                files.push(GitFileItem {
                    path: rel.to_string(),
                    status: "untracked".to_string(),
                    old_path: None,
                    new_content: Some(content),
                    old_content: None,
                    no_content: false,
                });
            }
        }
    }

    let mut seen_paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();

    // Unmodified files carrying review notes.
    if let Some(db) = notes_db {
        let note_files = crate::notes::note_files_for_repo(db, repo);
        for file_path in note_files {
            if seen_paths.contains(&file_path) {
                continue;
            }
            if !paths.is_empty()
                && !paths.iter().any(|p| {
                    let clean = p.strip_suffix('/').unwrap_or(p);
                    file_path == clean || file_path.starts_with(&format!("{clean}/"))
                })
            {
                continue;
            }

            let content = if refs.worktree_new {
                let abs = repo_root.join(&file_path);
                fs::read_to_string(&abs).ok()
            } else if let Some(ref new_ref) = refs.new_ref {
                load_git_blob(repo_root, new_ref, &file_path).0
            } else {
                let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
                load_git_blob(repo_root, old_tree_ref, &file_path).0
            };

            if let Some(c) = content {
                if !diff.is_empty() && !diff.ends_with('\n') {
                    diff.push('\n');
                }
                diff.push_str(&format!(
                    "diff --git a/{file_path} b/{file_path}\nunmodified file mode 100644\n--- a/{file_path}\n+++ b/{file_path}\n"
                ));
                files.push(GitFileItem {
                    path: file_path.clone(),
                    status: "unmodified".to_string(),
                    old_path: None,
                    new_content: Some(c.clone()),
                    old_content: Some(c),
                    no_content: false,
                });
                seen_paths.insert(file_path);
            }
        }
    }

    let mut specs_to_fetch = Vec::new();

    for f in &mut files {
        if f.status == "deleted" || f.status == "untracked" || f.status == "unmodified" {
            continue;
        }
        if refs.worktree_new {
            let new_abs = repo_root.join(&f.path);
            if let Ok(c) = fs::read_to_string(&new_abs) {
                f.new_content = Some(c);
            }
        } else if let Some(new_ref) = &refs.new_ref {
            specs_to_fetch.push(format!("{new_ref}:{}", f.path));
        }
    }

    let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
    for f in &files {
        if f.status == "untracked" || f.status == "added" || f.status == "unmodified" {
            continue;
        }
        let old_lookup_path = f.old_path.as_deref().unwrap_or(&f.path);
        specs_to_fetch.push(format!("{old_tree_ref}:{old_lookup_path}"));
    }

    let mut batch_blobs = load_git_blobs_native(repo_root, &specs_to_fetch);

    for f in &mut files {
        if f.new_content.is_none()
            && let Some(new_ref) = &refs.new_ref
        {
            let spec = format!("{new_ref}:{}", f.path);
            f.new_content = batch_blobs.remove(&spec);
        }

        if f.status == "untracked" || f.status == "added" || f.status == "unmodified" {
            f.no_content = f.new_content.is_none() && f.old_content.is_none();
            continue;
        }
        let old_lookup_path = f.old_path.as_deref().unwrap_or(&f.path);
        let spec = format!("{old_tree_ref}:{old_lookup_path}");
        if let Some(c) = batch_blobs.remove(&spec) {
            if f.new_content.is_none() && f.status != "deleted" {
                f.new_content = Some(c.clone());
            }
            f.old_content = Some(c);
        }
        f.no_content = f.new_content.is_none() && f.old_content.is_none();
    }

    let mut new_content: HashMap<String, String> = HashMap::with_capacity(files.len());
    let mut old_content: HashMap<String, String> = HashMap::new();
    for f in files {
        if let Some(nc) = f.new_content {
            new_content.insert(f.path.clone(), nc);
        }
        if let Some(oc) = f.old_content {
            if let Some(ref op) = f.old_path {
                old_content.insert(op.clone(), oc.clone());
            }
            old_content.insert(f.path, oc);
        }
    }

    let commit_range = resolve_commit_range(repo_root, base_args);

    Ok(LoadGitResult {
        diff,
        new_content,
        old_content,
        branch,
        untracked_skipped,
        commit_range,
    })
}

#[inline]
pub fn load_git_repo(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
) -> Result<LoadGitResult, String> {
    load_git_repo_with_notes(repo_root, base_args, paths, staged, context, None, None)
}

/// Load a git blob text and byte size.
pub fn load_git_blob(
    repo_root: &Path,
    ref_str: &str,
    path: &str,
) -> (Option<String>, Option<usize>) {
    let spec = format!("{ref_str}:{path}");
    let blobs = load_git_blobs_batch(repo_root, std::slice::from_ref(&spec));
    let text = blobs.get(&spec).cloned();
    let size = text.as_ref().map(|t| t.len()).or_else(|| {
        run_git(repo_root, &["cat-file", "-s", &spec]).and_then(|s| s.trim().parse::<usize>().ok())
    });
    (text, size)
}

/// Load and process the diff for a single file on demand from a git repository.
#[tracing::instrument(
    level = "debug",
    skip(repo_root, base_args, opts),
    fields(path, status)
)]
pub fn load_single_file_diff(
    repo_root: &Path,
    base_args: &[String],
    path: &str,
    old_path: Option<&str>,
    status: &str,
    context: usize,
    opts: &crate::pipeline::PipelineOptions,
) -> Option<crate::wire::OutputFile> {
    if status == "untracked" {
        let abs = repo_root.join(path);
        let content = fs::read_to_string(&abs).ok()?;
        let line_count = content.lines().count();
        let mut synthetic_diff = format!(
            "diff --git a/{path} b/{path}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{line_count} @@\n"
        );
        for line in content.lines() {
            synthetic_diff.push('+');
            synthetic_diff.push_str(line);
            synthetic_diff.push('\n');
        }
        let mut parsed = crate::parse::parse_diff(&synthetic_diff);
        let mut f = parsed.pop()?;
        f.status = crate::parse::FileStatus::Untracked;
        f.new_bytes = Some(content.len());
        let mut rendered = crate::pipeline::process_single_diff_file(
            f,
            Some(&content),
            None,
            Some(Arc::new(synthetic_diff)),
            opts,
        );
        rendered.raw_source = Some(crate::wire::RawFileSource::Git {
            path: path.to_string(),
            old_path: old_path.map(String::from),
            status: status.to_string(),
        });
        return Some(rendered);
    }

    let refs = resolve_refs(repo_root, base_args, false);
    let mut cmd = Command::new("git");
    cmd.arg("diff")
        .arg(format!("-U{context}"))
        .arg("-M")
        .arg("--no-color")
        .args(base_args)
        .arg("--")
        .arg(path);
    if let Some(old) = old_path
        && old != path
    {
        cmd.arg(old);
    }
    cmd.current_dir(repo_root);
    let output = cmd.output().ok()?;
    let diff_str = String::from_utf8_lossy(&output.stdout).to_string();
    let mut parsed = crate::parse::parse_diff(&diff_str);
    let mut f = if let Some(p) = parsed.pop() {
        p
    } else {
        let st = match status {
            "renamed" => crate::parse::FileStatus::Renamed,
            "mode changed" => crate::parse::FileStatus::ModeChanged,
            "deleted" => crate::parse::FileStatus::Deleted,
            "added" => crate::parse::FileStatus::Added,
            "unmodified" => crate::parse::FileStatus::Unmodified,
            _ => crate::parse::FileStatus::Modified,
        };
        crate::parse::DiffFile {
            path: path.to_string(),
            old_path: old_path.map(String::from),
            old_mode: None,
            new_mode: None,
            status: st,
            is_binary: false,
            old_bytes: None,
            new_bytes: None,
            new_line_count: 0,
            old_line_count: 0,
            new_content: Vec::new(),
            old_content: Vec::new(),
            hunks: Vec::new(),
            raw_hunks: Vec::new(),
            no_content: false,
        }
    };

    let mut new_c = None;
    if f.status != crate::parse::FileStatus::Deleted {
        if refs.worktree_new {
            let new_abs = repo_root.join(&f.path);
            if let Ok(c) = fs::read_to_string(&new_abs) {
                f.new_bytes = Some(c.len());
                new_c = Some(c);
            }
        } else if let Some(ref new_ref) = refs.new_ref {
            let (c, sz) = load_git_blob(repo_root, new_ref, &f.path);
            new_c = c;
            if sz.is_some() {
                f.new_bytes = sz;
            }
        }
    }

    let mut old_c = None;
    let old_lookup = f.old_path.as_deref().unwrap_or(&f.path);
    let old_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
    if f.status != crate::parse::FileStatus::Added
        && f.status != crate::parse::FileStatus::Untracked
    {
        let (c, sz) = load_git_blob(repo_root, old_ref, old_lookup);
        old_c = c;
        if sz.is_some() {
            f.old_bytes = sz;
        }
    }

    let fallback = if old_c.is_none() && f.status != crate::parse::FileStatus::Added {
        Some(Arc::new(diff_str))
    } else {
        None
    };

    let mut rendered = crate::pipeline::process_single_diff_file(
        f,
        new_c.as_deref(),
        old_c.as_deref(),
        fallback,
        opts,
    );
    rendered.raw_source = Some(crate::wire::RawFileSource::Git {
        path: path.to_string(),
        old_path: old_path.map(String::from),
        status: status.to_string(),
    });
    Some(rendered)
}

#[cfg(test)]
mod tests;
