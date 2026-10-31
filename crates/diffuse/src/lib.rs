//! diffuse — High-performance diff engine, token-stream pipeline, ANSI terminal
//! renderer, and interactive TUI.
//!
//! # Unified Namespace Architecture
//!
//! This crate re-exports the workspace subcrates under a unified `diffuse::*`
//! namespace:
//!
//! - [`diffuse::algo`](algo): Language-agnostic algorithms, DP solvers,
//!   pushdown automata, and foundational data structures.
//! - [`diffuse::core`](core): Core AST diff parsing, DP clustering, sequence
//!   alignment, token classification, and wire formats.
//! - [`diffuse::render`](render): Terminal ANSI SGR styling, diff color themes,
//!   and formatted row generation.
//! - [`diffuse::syntax`](syntax): Language profiles, syntax classifier, and
//!   markdown code fence extraction.
//! - [`diffuse::tui`](tui): Interactive terminal review application, command
//!   dispatch, and viewport navigation.
//!
//! # Quick Start with Prelude
//!
//! ```rust
//! use diffuse::prelude::*;
//!
//! let diff = r#"diff --git a/main.rs b/main.rs
//! --- a/main.rs
//! +++ b/main.rs
//! @@ -1,1 +1,1 @@
//! -println!("hello");
//! +println!("hello world");
//! "#;
//!
//! let output = run_pipeline(diff, None);
//! assert_eq!(output.files.len(), 1);
//! let theme = default_theme();
//! let rendered = render_pipeline_output(&output, &theme, false);
//! assert!(!rendered.is_empty());
//! ```

pub use diffuse_algo as algo;
pub use diffuse_core as core;
pub use diffuse_render as render;
pub use diffuse_syntax as syntax;
pub use diffuse_tui as tui;

// First-class architectural modules lifted to the diffuse:: root
pub use diffuse_core::{git, logging, notes, streaming, wire};
pub use diffuse_render::{ansi, theme};

/// Initialize diffuse tracing and logging subscriber.
pub use diffuse_core::logging::init_tracing_subscriber as init_logging;

// Top-level direct re-exports for high-frequency engine primitives
pub use diffuse_algo::{DiffEngine, LcsEngine, MyersOnD, PrefixMatcher};
pub use diffuse_core::{
    DEFAULT_CLUSTER_BANDWIDTH_MIN, DEFAULT_CLUSTER_BANDWIDTH_SLACK, DEFAULT_CLUSTER_BUDGET,
    DEFAULT_CLUSTER_PRODUCT_CAP, DEFAULT_CONTEXT_LINES, DEFAULT_EXPAND_LINES,
    DEFAULT_TOKEN_PRODUCT_CAP, ENGINE_CONTEXT, LineHash, NoteStatus, NotesDB, PipelineInput,
    PipelineOptions, Repo, ReviewNote, ReviewTarget, ReviewTargetNotesExt, RevisionRange,
    notes::{add_note, default_notes_path, load_notes, save_notes},
    parse::{Hunk, Row, RowKind},
    pipeline::{run_pipeline, run_pipeline_full},
    wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput},
};
pub use diffuse_render::{
    DiffuseTheme, MarkdownOptions,
    ansi::{SGR_RESET, strip_ansi},
    default_theme, detect_terminal_width, render_file_diff, render_file_diff_with_width,
    render_file_markdown, render_markdown_output, render_pipeline_output,
    render_pipeline_output_with_width, render_raw_file_diff, render_raw_file_diff_with_width,
    render_raw_pipeline_output, render_raw_pipeline_output_with_width,
};
pub use diffuse_syntax::{TokenColor, profile::LangProfile, registry::lang_profile};
pub use diffuse_tui::{run_interactive_tui, run_tui};

/// Common imports and prelude traits for diffuse consumers.
pub mod prelude {
    pub use diffuse_algo::{DiffEngine, LcsEngine, MyersOnD, PrefixMatcher};
    pub use diffuse_core::{
        DEFAULT_CLUSTER_BANDWIDTH_MIN, DEFAULT_CLUSTER_BANDWIDTH_SLACK, DEFAULT_CLUSTER_BUDGET,
        DEFAULT_CLUSTER_PRODUCT_CAP, DEFAULT_CONTEXT_LINES, DEFAULT_EXPAND_LINES,
        DEFAULT_TOKEN_PRODUCT_CAP, ENGINE_CONTEXT, LineHash, NoteStatus, NotesDB, PipelineInput,
        PipelineOptions, Repo, ReviewNote, ReviewTarget, ReviewTargetNotesExt, RevisionRange,
        merge::RunKind,
        notes::{add_note, default_notes_path, load_notes, save_notes},
        parse::{Hunk, Row, RowKind},
        pipeline::{run_pipeline, run_pipeline_full},
        wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput},
    };
    pub use diffuse_render::{
        DiffuseTheme, MarkdownOptions,
        ansi::{SGR_RESET, strip_ansi},
        default_theme, detect_terminal_width, render_file_diff, render_file_diff_with_width,
        render_file_markdown, render_markdown_output, render_pipeline_output,
        render_pipeline_output_with_width, render_raw_file_diff, render_raw_file_diff_with_width,
        render_raw_pipeline_output, render_raw_pipeline_output_with_width, render_source_rows,
    };
    pub use diffuse_syntax::{TokenColor, profile::LangProfile, registry::lang_profile};
    pub use diffuse_tui::{run_interactive_tui, run_tui};
}

#[cfg(test)]
mod tests;
