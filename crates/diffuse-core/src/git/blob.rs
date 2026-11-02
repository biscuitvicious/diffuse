use std::{collections::HashMap, path::Path};

/// Batch-load file blob contents using native Gitoxide.
pub fn load_git_blobs_batch(repo_root: &Path, specs: &[String]) -> HashMap<String, String> {
    load_git_blobs_native(repo_root, specs)
}

/// Load file blob contents directly using the native `diffuse-git` object
/// database.
pub fn load_git_blobs_native(repo_root: &Path, specs: &[String]) -> HashMap<String, String> {
    if let Ok(repo) = diffuse_git::Repository::open(repo_root) {
        repo.load_blobs_text_batch(specs)
    } else {
        HashMap::new()
    }
}

/// Load a git blob text and byte size.
pub fn load_git_blob(
    repo_root: &Path,
    ref_str: &str,
    path: &str,
) -> (Option<String>, Option<usize>) {
    if let Ok(repo) = diffuse_git::Repository::open(repo_root)
        && let Some(blob) = repo.load_blob(ref_str, path)
    {
        let size = blob.size();
        let text = blob.as_str().map(ToString::to_string);
        return (text, Some(size));
    }
    (None, None)
}
