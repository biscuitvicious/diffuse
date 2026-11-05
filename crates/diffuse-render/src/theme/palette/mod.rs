//! Theme palette definitions and color mapping specifications.
//!
//! The scoped cursor-tier palette ([`scoped::ThemePalette`]) is the only
//! palette: one [`CursorPalette`] per cursor tier plus UI chrome, indexed
//! `palette[cursor][kind]`. The legacy flat structures it replaced were
//! removed at the phase 7 consumer migration and frozen into the
//! equivalence suite's [`baseline`](super::baseline) literals.

pub mod scoped;

pub use scoped::{CursorPalette, ThemePalette, UiPalette};
