//! Fixture serialization, snapshot naming, repository detection, and golden
//! tagging.

use std::{collections::HashMap, fs, path::Path, process};

use serde::Serialize;

use diffuse::core::notes::NotesDB;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureOutput {
    #[serde(skip_serializing)]
    pub diff: String,
    #[serde(skip_serializing)]
    pub new_content: HashMap<String, String>,
    #[serde(skip_serializing)]
    pub old_content: HashMap<String, String>,
    /// The revision (range) this fixture was harvested from, pinned to
    /// absolute short commit ids (`WORKTREE` records the working tree's
    /// `HEAD`). The diff and the old/new side contents travel as separate
    /// corpus files (`diff.patch`, `files/*.old.*`, `files/*.new.*`) — never
    /// inside the JSON.
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<NotesDB>,
}

/// Stable FNV-1a 64-bit hash — snapshot names must be reproducible across
/// runs, which the std hasher's per-process keys do not guarantee.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Filesystem-safe snapshot file name for a corpus path's old/new side:
/// `<slug>.<side>.<ext>` with the path's `/` folded to `-`. The side tag
/// and the file's extension always trail the name. A stem that would exceed
/// the filename budget keeps its TAIL — the file's own name — while the
/// dropped directory prefix is marked with a stable FNV-1a hash of the full
/// path so distinct long paths keep distinct names.
pub fn snapshot_filename(path: &str, side: &str) -> String {
    const MAX_STEM: usize = 128;
    let slug = path.replace('/', "-");
    let (stem, ext) = match slug.rfind('.') {
        Some(dot_idx) if dot_idx > 0 => (slug[..dot_idx].to_string(), &slug[dot_idx..]),
        _ => (slug, ""),
    };
    if stem.len() <= MAX_STEM {
        return format!("{stem}.{side}{ext}");
    }
    let mut keep_from = stem.len() - (MAX_STEM - 17);
    while !stem.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    format!(
        "{}-{:016x}.{side}{ext}",
        &stem[keep_from..],
        fnv1a(path.as_bytes())
    )
}

/// Detects whether `repo_root` is the diffuse codebase using Cargo metadata /
/// manifests.
pub fn is_diffuse_repo(repo_root: &Path) -> bool {
    let manifest = repo_root.join("Cargo.toml");
    if !manifest.is_file() {
        return false;
    }
    // Fast path: inspect root or cli manifest directly
    if let Ok(content) = fs::read_to_string(&manifest)
        && (content.contains("diffuse-core") || content.contains("name = \"diffuse\""))
    {
        return true;
    }
    let cli_manifest = repo_root.join("crates/diffuse/Cargo.toml");
    if let Ok(content) = fs::read_to_string(&cli_manifest)
        && content.contains("name = \"diffuse\"")
    {
        return true;
    }
    // Fallback: query cargo metadata
    if let Ok(out) = process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        && out.status.success()
        && let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout)
        && let Some(pkgs) = val["packages"].as_array()
    {
        return pkgs.iter().any(|p| p["name"] == "diffuse");
    }
    false
}

/// Resolves the golden fixture name for tagging: either explicit `--tag <NAME>`
/// or derived from `--output`.
pub fn resolve_golden_name(
    tag_arg: Option<&str>,
    output: Option<&Path>,
    in_diffuse: bool,
) -> Result<Option<String>, String> {
    if let Some(tag_val) = tag_arg {
        if !tag_val.is_empty() {
            return Ok(Some(tag_val.to_string()));
        }
        let name = output
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "--tag requires a tag name or an --output directory path from which to derive one"
                    .to_string()
            })?;
        return Ok(Some(name.to_string()));
    }
    if in_diffuse {
        return Ok(output
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()));
    }
    Ok(None)
}
