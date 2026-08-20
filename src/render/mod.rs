//! Diffuse Terminal Rendering & ANSI Stylization.
//!
//! Provides token ANSI stylization, row segment formatting, coordinate gutter
//! rendering, column slicing, and full screen frame layout.

pub mod diff;
pub mod row;
pub mod theme;

pub use diff::*;
pub use row::*;
pub use theme::*;
