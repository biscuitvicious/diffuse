//! Notes — draft commit/delete and callout generation.

pub mod callout;
pub mod draft;
pub mod mutations;
pub mod sync;
pub mod target;

#[cfg(test)]
mod tests;

pub(crate) use callout::style_focused_note_line;
#[allow(unused_imports)]
pub use sync::NotesChangeSummary;
pub use target::VisualTarget;
