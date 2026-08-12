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

pub mod collapse; // collapseHunkRows: pairing, stretch, reverse-stretch, reflow, ordered walk
pub mod counts; // fileCounts
pub mod lcs; // tokenizeLine, lcsPairs, spansFrom, wordSpans
pub mod merge; // mergedRuns, delPairing, delSpansToShow, wholesalePair
pub mod moves; // markMoves, the block-move rule
pub mod parse; // parseDiff, hunks, rows
pub mod similarity; // lineSimilarity, lineSimilarityParts
pub mod syntax;
pub mod tail; // isTailBeforeClosing, fnOf // langProfile, classifyTokens

pub mod bin_pipeline;
pub mod old_content;
