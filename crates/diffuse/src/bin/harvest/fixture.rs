//! Fixture serialization, repository detection, and golden tagging.

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
    /// corpus files (`fixture/diff.patch`, `fixture/old/<path>`,
    /// `fixture/new/<path>`) — never inside the JSON.
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<NotesDB>,
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
