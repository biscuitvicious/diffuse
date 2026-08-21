//! Git plumbing, ref resolution, and repo status loader for diffuse.
//!
//! Port of TypeScript's `resolveRefs`, `loadReview`, and `rustBackendLoad`.

use std::{collections::HashMap, fs, path::Path, process::Command};

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
    pub diff_text: String,
    pub contents: HashMap<String, String>,
    pub old_contents: HashMap<String, String>,
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
    if run_git(repo_root, &["rev-parse", "--is-inside-work-tree"]).is_none() {
        return Err(format!(
            "not a git repository (or any of the parent directories): {}",
            repo_root.display()
        ));
    }

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

    let diff_args: Vec<&str> = diff_cmd.iter().map(std::string::String::as_str).collect();
    let mut diff_text = run_git(repo_root, &diff_args).unwrap_or_default();

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
                if !diff_text.is_empty() && !diff_text.ends_with('\n') {
                    diff_text.push('\n');
                }
                diff_text.push_str(&format!(
                    "diff --git a/{rel} b/{rel}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{line_count} @@\n"
                ));
                for line in content.lines() {
                    diff_text.push('+');
                    diff_text.push_str(line);
                    diff_text.push('\n');
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
            let spec = format!("{new_ref}:{}", f.path);
            if let Some(c) = run_git(repo_root, &["show", &spec]) {
                f.new_content = Some(c);
            }
        }
    }

    let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
    for f in &mut files {
        if f.status == "untracked" {
            continue;
        }
        let new_empty = f.new_content.as_ref().is_none_or(String::is_empty);
        let needs_old =
            f.status == "deleted" || del_of.get(&f.path).copied().unwrap_or(0) > 0 || new_empty;
        if !needs_old {
            continue;
        }
        let spec = format!("{old_tree_ref}:{}", f.path);
        if let Some(c) = run_git(repo_root, &["show", &spec]) {
            if new_empty && f.status != "deleted" {
                f.new_content = Some(c.clone());
            }
            f.old_content = Some(c);
        }
        f.no_content = f.new_content.as_ref().is_none_or(String::is_empty)
            && f.old_content.as_ref().is_none_or(String::is_empty);
    }

    let mut contents: HashMap<String, String> = HashMap::with_capacity(files.len());
    let mut old_contents: HashMap<String, String> = HashMap::new();
    for f in files {
        if let Some(nc) = f.new_content {
            contents.insert(f.path.clone(), nc);
        }
        if let Some(oc) = f.old_content {
            old_contents.insert(f.path, oc);
        }
    }

    Ok(LoadGitResult {
        diff_text,
        contents,
        old_contents,
        branch,
        untracked_skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_git_repo_non_repo_errors() {
        let dir = std::env::temp_dir().join(format!("diffuse-nonrepo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let result = load_git_repo(&dir, &[], &[], false, 6);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_err(),
            "a non-git directory must error instead of silently yielding an empty diff"
        );
    }

    #[test]
    fn test_resolve_refs_two_dot() {
        let refs = resolve_refs(Path::new("."), &["main..feature".into()], false);
        assert!(!refs.worktree_new);
        assert_eq!(refs.new_ref.as_deref(), Some("feature"));
        assert_eq!(refs.old_ref.as_deref(), Some("main"));
    }

    #[test]
    fn test_resolve_refs_single_ref() {
        let refs = resolve_refs(Path::new("."), &["main".into()], false);
        assert!(refs.worktree_new);
        assert_eq!(refs.new_ref, None);
        assert_eq!(refs.old_ref.as_deref(), Some("main"));
    }

    #[test]
    fn test_resolve_refs_caret_exclamation() {
        let refs = resolve_refs(Path::new("."), &["abc1234^!".into()], false);
        assert!(!refs.worktree_new);
        assert_eq!(refs.new_ref.as_deref(), Some("abc1234"));
        assert_eq!(refs.old_ref.as_deref(), Some("abc1234^"));
    }

    #[test]
    fn test_load_git_repo_includes_untracked_files_with_synthetic_diff() {
        let dir = std::env::temp_dir().join(format!("diffuse-untracked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success());
        };

        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "test"]);
        std::fs::write(dir.join("tracked.rs"), "fn tracked() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "init"]);

        // Create untracked file
        std::fs::write(dir.join("untracked.rs"), "fn untracked() {}\n").unwrap();

        let res = load_git_repo(&dir, &[], &[], false, 6).unwrap();
        assert!(
            res.diff_text.contains("untracked.rs"),
            "diff_text must contain synthetic diff for untracked.rs"
        );
        assert!(
            res.contents.contains_key("untracked.rs"),
            "contents must contain untracked.rs"
        );

        let out = crate::pipeline::run_pipeline_full(
            &res.diff_text,
            &res.contents,
            Some(&res.old_contents),
            None,
            None,
        );
        let untracked_file = out
            .files
            .iter()
            .find(|f| f.path == "untracked.rs")
            .expect("untracked.rs must be in pipeline output");
        assert_eq!(untracked_file.status, "untracked");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
