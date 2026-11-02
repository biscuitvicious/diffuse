use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use diffuse_parse::FileStatus;

use super::{
    file::{FileDiffParams, generate_file_diff},
    line::WhitespaceMode,
};

/// Complete result of diffing a repository against a target.
#[derive(Debug, Clone, Default)]
pub struct RepoDiffResult {
    pub diff: String,
    pub files: Vec<diffuse_parse::DiffFile>,
    pub new_content: HashMap<String, String>,
    pub old_content: HashMap<String, String>,
    pub branch: Option<String>,
    pub untracked_skipped: Vec<String>,
}

/// Compute unified diff for a repository against a review target natively.
pub fn diff_repository(
    repo: &crate::Repository,
    target: &crate::ReviewTarget,
    context: u32,
    mode: WhitespaceMode,
) -> Result<RepoDiffResult, crate::GitError> {
    let statuses = repo.file_statuses(target)?;
    let mut files = Vec::new();
    let mut diff = String::new();
    let mut new_content_map = HashMap::new();
    let mut old_content_map = HashMap::new();

    let (old_ref, new_ref, worktree_new) = if let Some(ref r) = target.range {
        (Some(r.0.clone()), Some(r.1.clone()), false)
    } else if target.staged {
        (Some("HEAD".to_string()), None, false)
    } else {
        (None, None, true)
    };

    let repo_root = repo.work_dir().unwrap_or_else(|| repo.git_dir());

    for s in statuses {
        if !crate::status::pathspecs_match(&s.path, &target.pathspecs)
            && !s
                .old_path
                .as_deref()
                .is_some_and(|op| crate::status::pathspecs_match(op, &target.pathspecs))
        {
            continue;
        }

        let old_lookup = s.old_path.as_deref().unwrap_or(&s.path);
        let old_bytes: Option<Vec<u8>> = if s.status == FileStatus::Added {
            None
        } else if let Some(ref r) = old_ref {
            repo.load_blob(r, old_lookup).map(|b| b.data)
        } else {
            repo.load_index_blob(old_lookup).map(|b| b.data)
        };

        let new_bytes: Option<Vec<u8>> = if s.status == FileStatus::Deleted {
            None
        } else if worktree_new {
            if s.old_mode.as_deref() == Some("160000") || s.new_mode.as_deref() == Some("160000") {
                if let Ok(sub) = gix::open(repo_root.join(&s.path))
                    && let Ok(head) = sub.head_id()
                {
                    Some(format!("Subproject commit {head}\n").into_bytes())
                } else {
                    None
                }
            } else {
                fs::read(repo_root.join(&s.path)).ok()
            }
        } else if let Some(ref r) = new_ref {
            repo.load_blob(r, &s.path).map(|b| b.data)
        } else if target.staged {
            repo.load_index_blob(&s.path).map(|b| b.data)
        } else {
            fs::read(repo_root.join(&s.path)).ok()
        };

        let active_bytes = new_bytes.as_deref().or(old_bytes.as_deref()).unwrap_or(b"");
        let is_binary = repo.is_path_binary(&s.path, active_bytes)
            || old_bytes.as_ref().is_some_and(|b| b.contains(&0))
            || new_bytes.as_ref().is_some_and(|b| b.contains(&0));

        let res = generate_file_diff(FileDiffParams {
            path: &s.path,
            old_path: s.old_path.as_deref(),
            old_bytes: old_bytes.as_deref(),
            new_bytes: new_bytes.as_deref(),
            status: s.status,
            old_mode: s.old_mode.as_deref(),
            new_mode: s.new_mode.as_deref(),
            is_binary,
            context,
            mode,
        });

        let mode_changed = match (s.old_mode.as_deref(), s.new_mode.as_deref()) {
            (Some(o), Some(n)) => o != n,
            _ => false,
        };

        if res.file.hunks.is_empty()
            && old_bytes.as_deref() == new_bytes.as_deref()
            && (s.status == FileStatus::Modified || s.status == FileStatus::Unmodified)
            && !mode_changed
            && !is_binary
        {
            continue;
        }

        if !diff.is_empty() && !diff.ends_with('\n') {
            diff.push('\n');
        }
        diff.push_str(&res.patch);

        if !is_binary {
            if let Some(ob) = &old_bytes
                && let Ok(text) = std::str::from_utf8(ob)
            {
                old_content_map.insert(old_lookup.to_string(), text.to_string());
            }
            if let Some(nb) = &new_bytes
                && let Ok(text) = std::str::from_utf8(nb)
            {
                new_content_map.insert(s.path.clone(), text.to_string());
            }
        }

        files.push(res.file);
    }

    let mut untracked_skipped = Vec::new();
    if !target.staged && worktree_new {
        let untracked = repo.untracked_files(&target.pathspecs)?;
        untracked_skipped = untracked.skipped_binary;

        for f in untracked.files {
            let rel = f.path;
            let content_bytes = match fs::read(repo_root.join(&rel)) {
                Ok(b) => b,
                Err(_) => continue,
            };

            let is_binary = repo.is_path_binary(&rel, &content_bytes) || content_bytes.contains(&0);
            let res = generate_file_diff(FileDiffParams {
                path: &rel,
                old_path: None,
                old_bytes: None,
                new_bytes: Some(&content_bytes),
                status: FileStatus::Untracked,
                old_mode: None,
                new_mode: Some("100644"),
                is_binary,
                context,
                mode,
            });

            if !diff.is_empty() && !diff.ends_with('\n') {
                diff.push('\n');
            }
            diff.push_str(&res.patch);

            if !is_binary && let Ok(text) = String::from_utf8(content_bytes) {
                new_content_map.insert(rel, text);
            }

            files.push(res.file);
        }
    }

    let branch = repo.current_branch();

    Ok(RepoDiffResult {
        diff,
        files,
        new_content: new_content_map,
        old_content: old_content_map,
        branch,
        untracked_skipped,
    })
}

/// Compute unified diff between two files or two directory trees directly on
/// disk.
pub fn diff_paths(
    old_path: &Path,
    new_path: &Path,
    context: u32,
    mode: WhitespaceMode,
) -> std::io::Result<RepoDiffResult> {
    if !old_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path not found: {}", old_path.display()),
        ));
    }
    if !new_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path not found: {}", new_path.display()),
        ));
    }

    if old_path.is_file() && new_path.is_file() {
        let old_bytes = fs::read(old_path)?;
        let new_bytes = fs::read(new_path)?;
        let is_binary = old_bytes.contains(&0) || new_bytes.contains(&0);
        let path_str = new_path.to_string_lossy();
        let old_path_str = old_path.to_string_lossy();

        let res = generate_file_diff(FileDiffParams {
            path: &path_str,
            old_path: Some(&old_path_str),
            old_bytes: Some(&old_bytes),
            new_bytes: Some(&new_bytes),
            status: FileStatus::Modified,
            old_mode: None,
            new_mode: None,
            is_binary,
            context,
            mode,
        });

        let mut new_content_map = HashMap::new();
        let mut old_content_map = HashMap::new();
        if !is_binary {
            if let Ok(text) = String::from_utf8(new_bytes) {
                new_content_map.insert(path_str.to_string(), text);
            }
            if let Ok(text) = String::from_utf8(old_bytes) {
                old_content_map.insert(old_path_str.to_string(), text);
            }
        }

        return Ok(RepoDiffResult {
            diff: res.patch,
            files: vec![res.file],
            new_content: new_content_map,
            old_content: old_content_map,
            branch: None,
            untracked_skipped: Vec::new(),
        });
    }

    if old_path.is_dir() && new_path.is_dir() {
        fn collect_rel_files(
            root: &Path,
            cur: &Path,
            acc: &mut Vec<PathBuf>,
        ) -> std::io::Result<()> {
            for entry in fs::read_dir(cur)? {
                let entry = entry?;
                let path = entry.path();
                let file_name = entry.file_name();
                if file_name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    collect_rel_files(root, &path, acc)?;
                } else if path.is_file()
                    && let Ok(rel) = path.strip_prefix(root)
                {
                    acc.push(rel.to_path_buf());
                }
            }
            Ok(())
        }

        let mut old_files = Vec::new();
        let mut new_files = Vec::new();
        collect_rel_files(old_path, old_path, &mut old_files)?;
        collect_rel_files(new_path, new_path, &mut new_files)?;

        let mut all_paths: BTreeSet<PathBuf> = BTreeSet::new();
        all_paths.extend(old_files);
        all_paths.extend(new_files);

        let mut files = Vec::new();
        let mut diff = String::new();
        let mut new_content_map = HashMap::new();
        let mut old_content_map = HashMap::new();

        for rel in all_paths {
            let rel_str = rel.to_string_lossy().to_string();
            let p_old = old_path.join(&rel);
            let p_new = new_path.join(&rel);

            let has_old = p_old.is_file();
            let has_new = p_new.is_file();

            let old_bytes = if has_old {
                Some(fs::read(&p_old)?)
            } else {
                None
            };
            let new_bytes = if has_new {
                Some(fs::read(&p_new)?)
            } else {
                None
            };

            let (status, is_binary) = match (&old_bytes, &new_bytes) {
                (Some(o), Some(n)) => {
                    if o == n {
                        // Unmodified, skip from diff
                        continue;
                    }
                    (FileStatus::Modified, o.contains(&0) || n.contains(&0))
                }
                (None, Some(n)) => (FileStatus::Added, n.contains(&0)),
                (Some(o), None) => (FileStatus::Deleted, o.contains(&0)),
                (None, None) => continue,
            };

            let res = generate_file_diff(FileDiffParams {
                path: &rel_str,
                old_path: None,
                old_bytes: old_bytes.as_deref(),
                new_bytes: new_bytes.as_deref(),
                status,
                old_mode: None,
                new_mode: None,
                is_binary,
                context,
                mode,
            });

            if !diff.is_empty() && !diff.ends_with('\n') {
                diff.push('\n');
            }
            diff.push_str(&res.patch);

            if !is_binary {
                if let Some(nb) = new_bytes
                    && let Ok(text) = String::from_utf8(nb)
                {
                    new_content_map.insert(rel_str.clone(), text);
                }
                if let Some(ob) = old_bytes
                    && let Ok(text) = String::from_utf8(ob)
                {
                    old_content_map.insert(rel_str, text);
                }
            }

            files.push(res.file);
        }

        return Ok(RepoDiffResult {
            diff,
            files,
            new_content: new_content_map,
            old_content: old_content_map,
            branch: None,
            untracked_skipped: Vec::new(),
        });
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "cannot diff file with directory: '{}' and '{}'",
            old_path.display(),
            new_path.display()
        ),
    ))
}
