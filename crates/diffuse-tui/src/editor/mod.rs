//! Text editor component and external editor launcher for diffuse TUI.

pub mod buffer;
pub mod external;
pub mod highlight;
pub mod history;
pub mod input;

#[cfg(test)]
mod tests;

pub use buffer::Editor;
pub use external::*;
pub use highlight::{HighlightingCallback, markdown_highlighter};
pub use input::EditorAction;
