//! Diffuse Terminal Rendering & ANSI Stylization.
//!
//! Provides token ANSI stylization, row segment formatting, coordinate gutter
//! rendering, column slicing, and full screen frame layout.

pub use diffuse_core as core;
pub use diffuse_syntax as syntax;

pub mod ansi;
pub mod badge;
pub mod counts;
pub mod diff;
pub mod markdown;
pub mod notes;
pub mod row;
pub mod span;
pub mod theme;
pub mod width;

pub use ansi::*;
pub use badge::*;
pub use counts::*;
pub use diff::*;
pub use markdown::*;
pub use notes::*;
pub use row::*;
pub use span::*;
pub use theme::*;
pub use width::*;
