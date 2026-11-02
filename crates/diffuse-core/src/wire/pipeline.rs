//! Top-Level Pipeline Output & Byte Size Models.
//!
//! Encodes the top-level wire contract payload containing processed file diffs,
//! branch metadata, byte size formatting, and directory prefix analysis.

use std::fmt::{self, Debug, Display};

use serde::{Deserialize, Serialize};

use super::OutputFile;
use crate::{counts::FileCounts, parse::DiffFile};

/// The top-level wire contract payload containing processed file diffs, branch
/// name, and untracked file metadata.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untracked_skipped: Vec<String>,
}

impl PipelineOutput {
    pub fn reverse(mut self) -> Self {
        self.files = self.files.into_iter().map(OutputFile::reverse).collect();
        self
    }

    pub fn reconstruct(&self) -> Vec<DiffFile> {
        self.files.iter().map(OutputFile::reconstruct).collect()
    }

    /// Computes the aggregated line-level modification counts summed across
    /// all files in this output payload, including the total number of review
    /// notes.
    pub fn total_counts(&self) -> FileCounts {
        let mut total = FileCounts::default();
        for f in &self.files {
            total.adds += f.counts.adds;
            total.dels += f.counts.dels;
            total.mods += f.counts.mods;
            total.movs += f.counts.movs;
            total.notes += f.counts.notes;
            total.in_progress_notes += f.counts.in_progress_notes;
            total.resolved_notes += f.counts.resolved_notes;
        }
        total
    }

    /// Computes the longest common directory prefix shared across all files in
    /// the diff (both `path` and `old_path`), if there are 2 or more files.
    /// Returns `Some(prefix)` with a trailing slash (e.g.
    /// `"crates/diffuse-core/src/"`) or `None` if there is no shared
    /// directory prefix or fewer than 2 files.
    pub fn common_directory_prefix(&self) -> Option<String> {
        if self.files.len() < 2 {
            return None;
        }

        let mut all_paths: Vec<&str> = Vec::new();
        for f in &self.files {
            all_paths.push(&f.path);
            if let Some(ref old) = f.old_path {
                all_paths.push(old);
            }
        }

        let split_paths: Vec<Vec<&str>> =
            all_paths.iter().map(|p| p.split('/').collect()).collect();

        // If any path has only 1 component (top-level file), there is no common
        // directory prefix
        if split_paths.iter().any(|p| p.len() <= 1) {
            return None;
        }

        let mut common_dirs = Vec::new();
        let min_dirs_len = split_paths.iter().map(|p| p.len() - 1).min().unwrap_or(0);

        for dir_idx in 0..min_dirs_len {
            let seg = split_paths[0][dir_idx];
            if split_paths.iter().all(|p| p[dir_idx] == seg) {
                common_dirs.push(seg);
            } else {
                break;
            }
        }

        if common_dirs.is_empty() {
            None
        } else {
            let mut prefix = common_dirs.join("/");
            prefix.push('/');
            Some(prefix)
        }
    }

    /// Validates cross-pipeline invariants: all files are valid and total
    /// counts match.
    pub fn validate_invariants(&self) -> Result<(), String> {
        for f in &self.files {
            f.validate_invariants()?;
        }
        let total = self.total_counts();
        let mut expected = FileCounts::default();
        for f in &self.files {
            expected.adds += f.counts.adds;
            expected.dels += f.counts.dels;
            expected.mods += f.counts.mods;
            expected.movs += f.counts.movs;
            expected.notes += f.counts.notes;
            expected.in_progress_notes += f.counts.in_progress_notes;
            expected.resolved_notes += f.counts.resolved_notes;
        }
        if total != expected {
            return Err("PipelineOutput total_counts mismatch".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(pub usize);

impl Debug for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_byte_size(self.0))
    }
}

impl Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_byte_size(self.0))
    }
}

pub fn format_byte_size(size: usize) -> String {
    let units = ["B", "kB", "MB", "GB", "TB", "PB"];
    let (size, unit) = units
        .into_iter()
        .enumerate()
        .rev()
        .map(|(i, u)| (size as f64 / (1u64 << (i * 10)) as f64, u))
        .find(|&(size, _)| size >= 0.5)
        .unwrap_or((size as f64, "B"));

    // Decimals only below 5.0 adjusted units; at and above, integers.
    let formatted = if size < 5.0 {
        let mut trimmed = format!("{:.1}", size);
        // Trim trailing zeros after the decimal point
        if trimmed.contains('.') {
            trimmed = trimmed
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string();
        }
        trimmed
    } else {
        format!("{:.0}", size)
    };

    format!("{}{}", formatted, unit)
}
