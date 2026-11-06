//! Notes — draft commit/delete and callout generation.

pub mod callout;
pub mod draft;
pub mod mutations;
pub mod sync;
pub mod target;

#[cfg(test)]
mod tests;

pub(crate) use callout::NoteRowFocus;
#[allow(unused_imports)]
pub use sync::NotesChangeSummary;
pub use target::VisualTarget;
