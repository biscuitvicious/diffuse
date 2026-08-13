//! Git plumbing, ref resolution, and repo status loader for diffuse.
//!
//! Port of TypeScript's `resolveRefs`, `loadReview`, and `rustBackendLoad`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

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

/// Resolve old/new git refs from base args (e.g. `main..feature`, `HEAD^!`, `main...feature`).
pub fn resolve_refs(cwd: &Path, base_args: &[String], cached: bool) -> RefPair {
    let refs: Vec<&str> = base_args
        .iter()
        .map(|s| s.as_str())
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
    pub new_content: Vec<String>,
    pub old_content: Vec<String>,
    pub no_content: bool,
}

pub struct LoadGitResult {
    pub diff_text: String,
    pub contents: HashMap<String, String>,
    pub branch: String,
    pub untracked_skipped: Vec<String>,
}

pub fn load_git_repo(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    cached: bool,
    context: usize,
) -> Result<LoadGitResult, String> {
    let branch = run_git(repo_root, &["branch", "--show-current"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mut diff_cmd = vec![
        "diff".to_string(),
        format!("-U{context}"),
        "-M".to_string(),
        "--no-color".to_string(),
    ];
    diff_cmd.extend(base_args.iter().cloned());
    diff_cmd.push("--".to_string());
    diff_cmd.extend(paths.iter().cloned());

    let diff_args: Vec<&str> = diff_cmd.iter().map(|s| s.as_str()).collect();
    let diff_text = run_git(repo_root, &diff_args).unwrap_or_default();

    let mut name_cmd = vec![
        "diff".to_string(),
        "-M".to_string(),
        "--name-status".to_string(),
    ];
    name_cmd.extend(base_args.iter().cloned());
    name_cmd.push("--".to_string());
    name_cmd.extend(paths.iter().cloned());
    let name_args: Vec<&str> = name_cmd.iter().map(|s| s.as_str()).collect();
    let name_status = run_git(repo_root, &name_args).unwrap_or_default();

    let mut num_cmd = vec![
        "diff".to_string(),
        "-M".to_string(),
        "--numstat".to_string(),
    ];
    num_cmd.extend(base_args.iter().cloned());
    num_cmd.push("--".to_string());
    num_cmd.extend(paths.iter().cloned());
    let num_args: Vec<&str> = num_cmd.iter().map(|s| s.as_str()).collect();
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
            new_content: Vec::new(),
            old_content: Vec::new(),
            no_content: false,
        });
    }

    let mut untracked_skipped: Vec<String> = Vec::new();
    if !cached && paths.is_empty() {
        let ut_opt = run_git(repo_root, &["ls-files", "--others", "--exclude-standard"]);
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
                let lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
                files.push(GitFileItem {
                    path: rel.to_string(),
                    status: "untracked".to_string(),
                    new_content: lines,
                    old_content: Vec::new(),
                    no_content: false,
                });
            }
        }
    }

    for f in &mut files {
        if f.status == "deleted" || f.status == "untracked" {
            continue;
        }
        if refs.worktree_new {
            let new_abs = repo_root.join(&f.path);
            if let Ok(c) = fs::read_to_string(&new_abs) {
                f.new_content = c.split('\n').map(|s| s.to_string()).collect();
            }
        } else if let Some(new_ref) = &refs.new_ref {
            let spec = format!("{new_ref}:{}", f.path);
            if let Some(c) = run_git(repo_root, &["show", &spec]) {
                f.new_content = c.split('\n').map(|s| s.to_string()).collect();
            }
        }
    }

    let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
    for f in &mut files {
        if f.status == "untracked" {
            continue;
        }
        let new_empty = f.new_content.is_empty();
        let needs_old =
            f.status == "deleted" || del_of.get(&f.path).copied().unwrap_or(0) > 0 || new_empty;
        if !needs_old {
            continue;
        }
        let spec = format!("{old_tree_ref}:{}", f.path);
        if let Some(c) = run_git(repo_root, &["show", &spec]) {
            f.old_content = c.split('\n').map(|s| s.to_string()).collect();
            if new_empty && f.status != "deleted" {
                f.new_content = f.old_content.clone();
            }
        }
        f.no_content = f.new_content.is_empty() && f.old_content.is_empty();
    }

    let mut contents: HashMap<String, String> = HashMap::new();
    for f in &files {
        contents.insert(f.path.clone(), f.new_content.join("\n"));
    }

    Ok(LoadGitResult {
        diff_text,
        contents,
        branch,
        untracked_skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_refs_two_dot() {
        let refs = resolve_refs(Path::new("."), &["main..feature".into()], false);
        assert_eq!(refs.worktree_new, false);
        assert_eq!(refs.new_ref.as_deref(), Some("feature"));
        assert_eq!(refs.old_ref.as_deref(), Some("main"));
    }

    #[test]
    fn test_resolve_refs_single_ref() {
        let refs = resolve_refs(Path::new("."), &["main".into()], false);
        assert_eq!(refs.worktree_new, true);
        assert_eq!(refs.new_ref, None);
        assert_eq!(refs.old_ref.as_deref(), Some("main"));
    }

    #[test]
    fn test_resolve_refs_caret_exclamation() {
        let refs = resolve_refs(Path::new("."), &["abc1234^!".into()], false);
        assert_eq!(refs.worktree_new, false);
        assert_eq!(refs.new_ref.as_deref(), Some("abc1234"));
        assert_eq!(refs.old_ref.as_deref(), Some("abc1234^"));
    }
}
