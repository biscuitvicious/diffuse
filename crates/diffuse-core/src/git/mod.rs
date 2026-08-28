//! Git plumbing, ref resolution, and repo status loader for diffuse.
//!
//! Port of TypeScript's `resolveRefs`, `loadReview`, and `rustBackendLoad`.

use std::{collections::HashMap, fs, path::Path, process::Command};

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
pub fn resolve_refs(cwd: &Path, base_args: &[String], cached: bool) -> RefPair {
    let refs: Vec<&str> = base_args
        .iter()
        .map(std::string::String::as_str)
        .filter(|t| *t != "--cached" && !t.starts_with('-'))
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
        worktree_new: true,
        new_ref: None,
        old_ref: if cached {
            Some("HEAD".to_string())
        } else {
            None
        },
    }
}

#[derive(Debug, Clone)]
pub struct GitFileItem {
    pub path: String,
    pub status: String,
    pub new_content: Option<String>,
    pub old_content: Option<String>,
    pub no_content: bool,
}

pub struct LoadGitResult {
    pub diff: String,
    pub new_content: HashMap<String, String>,
    pub old_content: HashMap<String, String>,
    pub branch: String,
    pub untracked_skipped: Vec<String>,
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

    let writer = std::thread::spawn(move || {
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

    // If any specs were not found in the gix ODB (e.g. unreferenced index blobs),
    // fill in missing ones via batch loader
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
pub fn stream_git_repo_to_producer(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    cached: bool,
    context: usize,
    producer: crate::streaming::StreamingProducer,
    meta_tx: &std::sync::mpsc::Sender<Result<crate::streaming::GitRepoMeta, String>>,
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

    let mut untracked_skipped = Vec::new();
    let refs = resolve_refs(repo_root, base_args, cached);

    let mut diff_cmd = vec![
        "diff".to_string(),
        format!("-U{context}"),
        "-M".to_string(),
        "--no-color".to_string(),
    ];
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
    let mut tree_cache: std::collections::HashMap<String, Option<gix::Tree<'_>>> =
        std::collections::HashMap::new();

    let mut get_blob = |ref_str: &str, path: &str| -> Option<String> {
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
                    && let Ok(text) = std::str::from_utf8(&obj.data)
                {
                    return Some(text.to_string());
                }
                return None;
            }
        }
        let spec = format!("{ref_str}:{path}");
        let blobs = load_git_blobs_batch(repo_root, std::slice::from_ref(&spec));
        blobs.get(&spec).cloned()
    };

    let mut emit_chunk = |chunk: &str, idx: &mut usize| {
        if chunk.trim().is_empty() {
            return;
        }
        let parsed_files = crate::parse::parse_diff(chunk);
        for f in parsed_files {
            let mut new_c = None;
            let mut old_c = None;

            if f.status != crate::parse::FileStatus::Deleted {
                if refs.worktree_new {
                    let new_abs = repo_root.join(&f.path);
                    if let Ok(c) = fs::read_to_string(&new_abs) {
                        new_c = Some(c);
                    }
                } else if let Some(new_ref) = &refs.new_ref {
                    new_c = get_blob(new_ref, &f.path);
                }
            }

            let has_dels = f.hunks.iter().any(|h| {
                h.rows
                    .iter()
                    .any(|r| matches!(r, crate::parse::Row::Del(_)))
            });
            let needs_old =
                f.status == crate::parse::FileStatus::Deleted || has_dels || new_c.is_none();
            if needs_old && let Some(c) = get_blob(old_tree_ref, &f.path) {
                if new_c.is_none() && f.status != crate::parse::FileStatus::Deleted {
                    new_c = Some(c.clone());
                }
                old_c = Some(c);
            }

            let fallback = if old_c.is_none() && f.status != crate::parse::FileStatus::Added {
                Some(chunk.to_string())
            } else {
                None
            };

            producer.push(crate::streaming::FileJob {
                idx: *idx,
                file: f,
                new_content: new_c,
                old_content: old_c,
                diff_fallback: fallback,
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

    // Untracked files
    if !cached {
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
                for mut f in parsed_files {
                    f.status = crate::parse::FileStatus::Untracked;
                    producer.push(crate::streaming::FileJob {
                        idx: file_idx,
                        file: f,
                        new_content: Some(content.clone()),
                        old_content: None,
                        diff_fallback: Some(synthetic_diff.clone()),
                    });
                    file_idx += 1;
                }
            }
        }
    }

    let _ = meta_tx.send(Ok(crate::streaming::GitRepoMeta {
        branch,
        untracked_skipped,
    }));
    producer.finish();
}

pub fn load_git_repo(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    cached: bool,
    context: usize,
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
    diff_cmd.extend(base_args.iter().cloned());
    diff_cmd.push("--".to_string());
    diff_cmd.extend(paths.iter().cloned());

    let diff_args: Vec<&str> = diff_cmd.iter().map(std::string::String::as_str).collect();
    let mut diff = run_git(repo_root, &diff_args).unwrap_or_default();

    let mut name_cmd = vec![
        "diff".to_string(),
        "-M".to_string(),
        "--name-status".to_string(),
    ];
    name_cmd.extend(base_args.iter().cloned());
    name_cmd.push("--".to_string());
    name_cmd.extend(paths.iter().cloned());
    let name_args: Vec<&str> = name_cmd.iter().map(std::string::String::as_str).collect();
    let name_status = run_git(repo_root, &name_args).unwrap_or_default();

    let mut num_cmd = vec![
        "diff".to_string(),
        "-M".to_string(),
        "--numstat".to_string(),
    ];
    num_cmd.extend(base_args.iter().cloned());
    num_cmd.push("--".to_string());
    num_cmd.extend(paths.iter().cloned());
    let num_args: Vec<&str> = num_cmd.iter().map(std::string::String::as_str).collect();
    let numstat = run_git(repo_root, &num_args).unwrap_or_default();

    let mut del_of: HashMap<String, usize> = HashMap::new();
    for l in numstat.lines() {
        let parts: Vec<&str> = l.split('\t').collect();
        if parts.len() == 3
            && let Ok(dels) = parts[1].parse::<usize>()
        {
            del_of.insert(parts[2].to_string(), dels);
        }
    }

    let refs = resolve_refs(repo_root, base_args, cached);
    let mut files: Vec<GitFileItem> = Vec::new();

    for l in name_status.lines() {
        let parts: Vec<&str> = l.split('\t').collect();
        if parts.is_empty() || parts[0].is_empty() {
            continue;
        }
        let st = parts[0];
        let mut status = "modified";
        let mut path = parts.get(1).copied().unwrap_or("").to_string();
        if st.starts_with('A') {
            status = "added";
        } else if st.starts_with('D') {
            status = "deleted";
        } else if st.starts_with('R') {
            status = "renamed";
            path = parts.get(2).copied().unwrap_or("").to_string();
        }
        files.push(GitFileItem {
            path,
            status: status.to_string(),
            new_content: None,
            old_content: None,
            no_content: false,
        });
    }

    let mut untracked_skipped: Vec<String> = Vec::new();
    if !cached {
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
                    new_content: Some(content),
                    old_content: None,
                    no_content: false,
                });
            }
        }
    }

    let mut specs_to_fetch = Vec::new();

    for f in &mut files {
        if f.status == "deleted" || f.status == "untracked" {
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
        if f.status == "untracked" {
            continue;
        }
        let new_empty = f.new_content.as_ref().is_none_or(String::is_empty);
        let needs_old =
            f.status == "deleted" || del_of.get(&f.path).copied().unwrap_or(0) > 0 || new_empty;
        if needs_old {
            specs_to_fetch.push(format!("{old_tree_ref}:{}", f.path));
        }
    }

    let mut batch_blobs = load_git_blobs_native(repo_root, &specs_to_fetch);

    for f in &mut files {
        if f.new_content.is_none()
            && let Some(new_ref) = &refs.new_ref
        {
            let spec = format!("{new_ref}:{}", f.path);
            f.new_content = batch_blobs.remove(&spec);
        }

        if f.status == "untracked" {
            continue;
        }
        let new_empty = f.new_content.as_ref().is_none_or(String::is_empty);
        let needs_old =
            f.status == "deleted" || del_of.get(&f.path).copied().unwrap_or(0) > 0 || new_empty;
        if needs_old {
            let spec = format!("{old_tree_ref}:{}", f.path);
            if let Some(c) = batch_blobs.remove(&spec) {
                if new_empty && f.status != "deleted" {
                    f.new_content = Some(c.clone());
                }
                f.old_content = Some(c);
            }
        }
        f.no_content = f.new_content.as_ref().is_none_or(String::is_empty)
            && f.old_content.as_ref().is_none_or(String::is_empty);
    }

    let mut new_content: HashMap<String, String> = HashMap::with_capacity(files.len());
    let mut old_content: HashMap<String, String> = HashMap::new();
    for f in files {
        if let Some(nc) = f.new_content {
            new_content.insert(f.path.clone(), nc);
        }
        if let Some(oc) = f.old_content {
            old_content.insert(f.path, oc);
        }
    }

    Ok(LoadGitResult {
        diff,
        new_content,
        old_content,
        branch,
        untracked_skipped,
    })
}

#[cfg(test)]
mod tests;
