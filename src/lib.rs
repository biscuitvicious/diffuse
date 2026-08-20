//! diffuse — the diff-engine token-stream backend.
//!
//! A renderer-agnostic port of the pi review extension's diff machinery:
//! parse a git diff + the per-file contents → mark moved lines → collapse
//! the FIFO del/add pairs into mods (with the stretch, reverse-stretch,
//! reflow, and brace-anchor rules) → emit the merged token runs that a
//! renderer (the pi extension's TUI today, a Rust TUI later) turns into
//! visual rows.
//!
//! The port is one-for-one from `review.ts` (Phase 1 of PLAN.md) — the
//! function names, control flow, and comments mirror the TypeScript so the
//! two can be diffed against each other. A "Rusty" redesign is explicitly
//! deferred to Phase 4.
//!
//! See PLAN.md for the rewrite roadmap and the port map.

#![warn(
    clippy::doc_markdown,
    clippy::redundant_closure_for_method_calls,
    clippy::match_same_arms
)]

pub mod cluster; // Context-delimited change clusters, syntax section partitioning
// and the cluster DP transformation engine
pub mod collapse; // collapseHunkRows: pairing, stretch, reverse-stretch, reflow, ordered walk
pub mod counts; // fileCounts
pub mod lcs; // tokenizeLine, lcsPairs, spansFrom, wordSpans
pub mod merge; // mergedRuns, delPairing, delSpansToShow, wholesalePair
pub mod moves; // markMoves, the block-move rule
pub mod parse; // parseDiff, hunks, rows
pub mod scope; // is_tail_before_closing, fn_of
pub mod shift; // diff identity transformations: content-neutral upward/downward hunk shifts
pub mod similarity; // lineSimilarity, lineSimilarityParts
pub mod syntax; // lang_profile, classify_tokens

pub mod apply; // apply_file_diff: the Phase 6 apply-oracle primitive
pub mod bin_pipeline;
pub mod git;
pub mod notes;
pub mod old_content;
pub mod render;
pub mod tui;
pub mod visible;
