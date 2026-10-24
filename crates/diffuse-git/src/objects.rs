//! Native Gitoxide object database, blob retrieval, and tree traversal.

use std::collections::HashMap;

use gix::{
    bstr::ByteSlice,
    diff::blob::{Platform, ResourceKind, platform::resource::Data},
    object::tree::EntryKind,
};

use crate::types::GitBlob;

/// Check whether a resource is binary using Gitoxide's diff pipeline.
///
/// This evaluates all metadata and content criteria via Gitoxide:
/// 1. `.gitattributes` (`-diff`, `binary`, or `diff=<driver>`).
/// 2. Diff driver configuration (`diff.<driver>.binary`).
/// 3. Resource entry mode (e.g. symlink vs blob).
/// 4. Buffer content (canonical 8,000-byte null check via Gitoxide).
pub fn is_blob_binary(
    platform: &mut Platform,
    id: gix::ObjectId,
    mode: EntryKind,
    rela_path: &str,
    repo: &gix::Repository,
) -> bool {
    let b_path = rela_path.as_bytes().as_bstr();
    if platform
        .set_resource(id, mode, b_path, ResourceKind::OldOrSource, repo)
        .is_ok()
        && let Some(res) = platform.resource(ResourceKind::OldOrSource) {
            return matches!(res.data, Data::Binary { .. });
        }
    false
}

fn index_mode_to_entry_kind(mode: gix::index::entry::Mode) -> EntryKind {
    if mode == gix::index::entry::Mode::FILE_EXECUTABLE {
        EntryKind::BlobExecutable
    } else if mode == gix::index::entry::Mode::SYMLINK {
        EntryKind::Link
    } else {
        EntryKind::Blob
    }
}

fn peel_to_tree<'a>(repo: &'a gix::Repository, id: gix::ObjectId) -> Option<gix::Tree<'a>> {
    let obj = repo.find_object(id).ok()?;
    match obj.kind {
        gix::object::Kind::Commit => obj.into_commit().tree().ok(),
        gix::object::Kind::Tree => Some(obj.into_tree()),
        _ => {
            let peeled = obj.peel_to_kind(gix::object::Kind::Tree).ok()?;
            Some(peeled.into_tree())
        }
    }
}

/// Load a single blob from a repository given a reference specifier and path.
///
/// If `ref_str` is empty or `":"`, the path is resolved against the staged
/// index. Otherwise, `ref_str` is resolved as a commit or tree.
pub fn load_blob(repo: &gix::Repository, ref_str: &str, path: &str) -> Option<GitBlob> {
    let clean_ref = ref_str.trim();
    if clean_ref.is_empty() || clean_ref == ":" || clean_ref == ":0" {
        return load_blob_from_index(repo, path);
    }

    let clean_path = path.trim_start_matches("./");

    let id = repo.rev_parse_single(clean_ref).ok()?;
    let tree = peel_to_tree(repo, id.detach())?;

    let entry = tree.lookup_entry_by_path(clean_path).ok()??;
    let obj = entry.object().ok()?;
    if obj.kind == gix::object::Kind::Blob {
        let mut platform = repo.diff_resource_cache_for_tree_diff().ok();
        let is_binary = if let Some(ref mut p) = platform {
            is_blob_binary(p, entry.object_id(), entry.mode().into(), clean_path, repo)
        } else {
            obj.data.iter().take(8000).any(|&b| b == 0)
        };
        Some(GitBlob::new(
            entry.object_id().to_string(),
            obj.data.to_vec(),
            is_binary,
        ))
    } else {
        None
    }
}

/// Load a blob directly from the index (staged file).
pub fn load_blob_from_index(repo: &gix::Repository, path: &str) -> Option<GitBlob> {
    let clean_path = path.trim_start_matches("./");
    let index = repo.index().ok()?;
    let entry = index.entry_by_path(clean_path.as_bytes().as_bstr())?;
    let obj = repo.find_object(entry.id).ok()?;
    if obj.kind == gix::object::Kind::Blob {
        let mut platform = repo.diff_resource_cache_for_tree_diff().ok();
        let mode = index_mode_to_entry_kind(entry.mode);
        let is_binary = if let Some(ref mut p) = platform {
            is_blob_binary(p, entry.id, mode, clean_path, repo)
        } else {
            obj.data.iter().take(8000).any(|&b| b == 0)
        };
        Some(GitBlob::new(
            entry.id.to_string(),
            obj.data.to_vec(),
            is_binary,
        ))
    } else {
        None
    }
}

/// Batch-load file blobs from the repository without spawning any subprocess.
pub fn load_blobs_batch(repo: &gix::Repository, specs: &[String]) -> HashMap<String, GitBlob> {
    let mut results = HashMap::with_capacity(specs.len());
    let mut grouped_by_ref: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut direct_ids: Vec<String> = Vec::new();

    for spec in specs {
        if let Some((ref_part, path_part)) = spec.split_once(':') {
            grouped_by_ref
                .entry(ref_part.to_string())
                .or_default()
                .push((spec.clone(), path_part.to_string()));
        } else {
            direct_ids.push(spec.clone());
        }
    }

    let mut platform = repo.diff_resource_cache_for_tree_diff().ok();

    // 1. Process direct object IDs
    for spec in direct_ids {
        if let Ok(id) = gix::ObjectId::from_hex(spec.as_bytes())
            && let Ok(obj) = repo.find_object(id)
            && obj.kind == gix::object::Kind::Blob
        {
            let is_binary = if let Some(ref mut p) = platform {
                let binary = is_blob_binary(p, id, EntryKind::Blob, &spec, repo);
                p.clear_resource_cache_keep_allocation();
                binary
            } else {
                obj.data.iter().take(8000).any(|&b| b == 0)
            };
            results.insert(
                spec.clone(),
                GitBlob::new(id.to_string(), obj.data.to_vec(), is_binary),
            );
        }
    }

    // 2. Process grouped tree/index lookups
    for (ref_str, entries) in grouped_by_ref {
        let clean_ref = ref_str.trim();
        if clean_ref.is_empty() || clean_ref == ":" || clean_ref == ":0" {
            if let Ok(index) = repo.index() {
                for (spec, path) in entries {
                    let clean_path = path.trim_start_matches("./");
                    if let Some(entry) = index.entry_by_path(clean_path.as_bytes().as_bstr())
                        && let Ok(obj) = repo.find_object(entry.id)
                        && obj.kind == gix::object::Kind::Blob
                    {
                        let mode = index_mode_to_entry_kind(entry.mode);
                        let is_binary = if let Some(ref mut p) = platform {
                            let binary = is_blob_binary(p, entry.id, mode, clean_path, repo);
                            p.clear_resource_cache_keep_allocation();
                            binary
                        } else {
                            obj.data.iter().take(8000).any(|&b| b == 0)
                        };
                        results.insert(
                            spec,
                            GitBlob::new(entry.id.to_string(), obj.data.to_vec(), is_binary),
                        );
                    }
                }
            }
            continue;
        }

        // Tree lookup
        let Ok(id) = repo.rev_parse_single(clean_ref) else {
            continue;
        };

        let Some(tree) = peel_to_tree(repo, id.detach()) else {
            continue;
        };

        for (spec, path) in entries {
            let clean_path = path.trim_start_matches("./");
            if let Ok(Some(entry)) = tree.lookup_entry_by_path(clean_path)
                && let Ok(obj) = entry.object()
                && obj.kind == gix::object::Kind::Blob
            {
                let is_binary = if let Some(ref mut p) = platform {
                    let binary =
                        is_blob_binary(p, entry.object_id(), entry.mode().into(), clean_path, repo);
                    p.clear_resource_cache_keep_allocation();
                    binary
                } else {
                    obj.data.iter().take(8000).any(|&b| b == 0)
                };
                results.insert(
                    spec,
                    GitBlob::new(entry.object_id().to_string(), obj.data.to_vec(), is_binary),
                );
            }
        }
    }

    results
}

/// Batch-load file blobs as UTF-8 strings. Non-UTF8 binary blobs are omitted.
pub fn load_blobs_text_batch(repo: &gix::Repository, specs: &[String]) -> HashMap<String, String> {
    let blobs = load_blobs_batch(repo, specs);
    let mut texts = HashMap::with_capacity(blobs.len());
    for (spec, blob) in blobs {
        if let Some(s) = blob.as_str() {
            texts.insert(spec, s.to_string());
        }
    }
    texts
}
