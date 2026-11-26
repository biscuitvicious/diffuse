//! The Diffuse Core Engine Pipeline Orchestrator.
//!
//! Transforms unified diffs and source buffers into structured,
//! syntax-classified token runs and wire models through a lean orchestrator:
//!
//! $$\text{Parse} \longrightarrow \text{Intern} \longrightarrow
//! \text{Algorithmic Optimization} \longrightarrow \text{Wire/Render}$$
//!
//! 1. Diff parsing & content reconstruction (`src/parse/`,
//!    `src/old_content.rs`)
//! 2. Symbol interning & token indexing (`src/algo/interner.rs`,
//!    `src/similarity.rs`)
//! 3. Algorithmic sequence optimization & cluster transformations:
//!    - Bipartite move relocation (`src/algo/bipartite_matching.rs`,
//!      `src/moves.rs`)
//!    - Container Dyck balancing & phrase landmarks (`src/algo/dyck.rs`,
//!      `src/algo/landmarks.rs`, `src/token.rs`)
//!    - 2D multi-objective dynamic programming & collapse
//!      (`src/algo/block_dp.rs`, `src/cluster/`, `src/collapse.rs`)
//! 4. Fence syntax region detection & classification (`src/syntax/`)
//! 5. Wire contract projection & streaming (`src/wire/`, `src/streaming.rs`)

use std::{collections::HashMap, sync::Arc};

use crate::{
    notes::NotesDB,
    wire::{OutputFile, PipelineOutput},
};

pub mod classify;
pub mod context;
pub mod file;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use crate::{
    notes::ReviewNote,
    wire::{MovSide, OutputRun},
};
#[cfg(test)]
pub(crate) use classify::classify_ctx_line;
pub use classify::{
    build_hunk_output_row, colored_runs, colored_runs_with_next, context_line_runs, token_cow,
};
pub use context::{
    markdown_file_rows, note_runs, render_plain_common_context, synthesize_note_context_hunks,
    synthesize_plain_common_context_hunk,
};
#[cfg(test)]
pub(crate) use diffuse_syntax::{fence::FenceRegion, registry::lang_profile};
pub use file::{
    T_RENDER_HUNKS, T_RENDER_ROWS, T_STAGE, is_initially_collapsed, process_single_diff_file,
    render_file_from_source,
};

/// Options and configuration controlling the pipeline execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOptions {
    pub notes_db: Option<NotesDB>,
    pub target_repo_key: Option<String>,
    pub target_rev_key: Option<String>,
    pub visual_context: usize,
    pub expand_lines: usize,
    pub token_product_cap: usize,
    pub cluster_product_cap: usize,
    pub lazy_collapsed: bool,
    pub show_resolved: bool,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            notes_db: None,
            target_repo_key: None,
            target_rev_key: None,
            visual_context: crate::git::DEFAULT_CONTEXT_LINES,
            expand_lines: crate::git::DEFAULT_EXPAND_LINES,
            token_product_cap: crate::lcs::DEFAULT_TOKEN_PRODUCT_CAP,
            cluster_product_cap: crate::cluster::dp::DEFAULT_CLUSTER_PRODUCT_CAP,
            lazy_collapsed: false,
            show_resolved: false,
        }
    }
}

impl PipelineOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_notes(
        mut self,
        notes_db: Option<NotesDB>,
        repo_key: Option<String>,
        rev_key: Option<String>,
    ) -> Self {
        self.notes_db = notes_db;
        self.target_repo_key = repo_key;
        self.target_rev_key = rev_key;
        self
    }

    pub fn with_repo_key(mut self, repo_key: Option<String>) -> Self {
        self.target_repo_key = repo_key;
        self
    }

    pub fn with_rev_key(mut self, rev_key: Option<String>) -> Self {
        self.target_rev_key = rev_key;
        self
    }

    pub fn with_visual_context(mut self, visual_context: usize) -> Self {
        self.visual_context = visual_context;
        self
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
        self
    }

    pub fn with_token_product_cap(mut self, token_product_cap: usize) -> Self {
        self.token_product_cap = token_product_cap;
        self
    }

    pub fn with_cluster_product_cap(mut self, cluster_product_cap: usize) -> Self {
        self.cluster_product_cap = cluster_product_cap;
        self
    }

    pub fn with_lazy_collapsed(mut self, lazy_collapsed: bool) -> Self {
        self.lazy_collapsed = lazy_collapsed;
        self
    }

    pub fn with_show_resolved(mut self, show_resolved: bool) -> Self {
        self.show_resolved = show_resolved;
        self
    }
}

/// Builder for diff pipeline inputs: diff text, optional new/old content
/// buffers, and pipeline options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineInput {
    pub diff: String,
    pub new_content: Option<HashMap<String, String>>,
    pub old_content: Option<HashMap<String, String>>,
    pub options: PipelineOptions,
}

impl PipelineInput {
    pub fn new(diff: impl Into<String>) -> Self {
        Self {
            diff: diff.into(),
            new_content: None,
            old_content: None,
            options: PipelineOptions::default(),
        }
    }

    pub fn with_new_content(mut self, new_content: HashMap<String, String>) -> Self {
        self.new_content = Some(new_content);
        self
    }

    pub fn with_old_content(mut self, old_content: HashMap<String, String>) -> Self {
        self.old_content = Some(old_content);
        self
    }

    pub fn with_options(mut self, options: PipelineOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_notes(
        mut self,
        db: Option<NotesDB>,
        repo_key: Option<String>,
        rev_key: Option<String>,
    ) -> Self {
        self.options.notes_db = db;
        self.options.target_repo_key = repo_key;
        self.options.target_rev_key = rev_key;
        self
    }

    pub fn with_visual_context(mut self, visual_context: usize) -> Self {
        self.options.visual_context = visual_context;
        self
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.options.expand_lines = expand_lines;
        self
    }

    /// Execute the full pipeline over the input.
    pub fn run(self) -> PipelineOutput {
        run_pipeline_full(self.diff, self.new_content, self.old_content, self.options)
    }
}

/// Run the pipeline over a diff + optional new-file contents; produce the
/// wire-contract output.
pub fn run_pipeline(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
) -> PipelineOutput {
    run_pipeline_with_notes(diff, new_content, None, None, None)
}

/// Run the pipeline over a diff with no file contents; produce the
/// wire-contract output.
pub fn run_pipeline_diff(diff: impl Into<String>) -> PipelineOutput {
    run_pipeline_full(diff, None, None, PipelineOptions::default())
}

/// Run the pipeline over a diff + optional new-file contents + optional notes;
/// produce the wire-contract output.
pub fn run_pipeline_with_notes(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    notes_db: Option<NotesDB>,
    repo_key: Option<String>,
    rev_key: Option<String>,
) -> PipelineOutput {
    run_pipeline_full(
        diff,
        new_content,
        None,
        PipelineOptions::default().with_notes(notes_db, repo_key, rev_key),
    )
}

/// Run the pipeline over Arc<String> inputs without string duplication.
#[tracing::instrument(level = "trace", ret, skip(diff, new_content, old_content, opts), fields(diff_bytes = diff.len(), new_file_count = new_content.as_ref().map(|m| m.len()), old_file_count = old_content.as_ref().map(|m| m.len()), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn run_pipeline_arc(
    diff: Arc<String>,
    new_content: Option<HashMap<String, Arc<String>>>,
    old_content: Option<HashMap<String, Arc<String>>>,
    opts: PipelineOptions,
) -> PipelineOutput {
    let out_files =
        crate::streaming::stream_diff_pipeline_arc(diff, new_content, old_content, opts, None)
            .collect();

    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

/// Run the pipeline over a diff + optional new-file contents + optional
/// old-file contents + pipeline options; produce the wire-contract output.
#[tracing::instrument(level = "trace", ret, skip(diff, new_content, old_content, opts), fields(new_file_count = new_content.as_ref().map(|m| m.len()), old_file_count = old_content.as_ref().map(|m| m.len()), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn run_pipeline_full(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    old_content: Option<HashMap<String, String>>,
    opts: PipelineOptions,
) -> PipelineOutput {
    crate::lcs::set_token_product_cap(opts.token_product_cap);
    crate::cluster::dp::set_cluster_product_cap(opts.cluster_product_cap);
    let out_files: Vec<OutputFile> =
        crate::streaming::stream_diff_pipeline(diff, new_content, old_content, opts, None)
            .collect();

    tracing::debug!(file_count = out_files.len(), "Completed diff pipeline");

    PipelineOutput {
        files: out_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    }
}

pub(crate) fn split_content_lines(s: &str) -> Vec<&str> {
    let stripped = s.strip_suffix('\r').unwrap_or(s);
    let stripped = stripped.strip_suffix('\n').unwrap_or(stripped);
    let stripped = stripped.strip_suffix('\r').unwrap_or(stripped);
    stripped
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}
