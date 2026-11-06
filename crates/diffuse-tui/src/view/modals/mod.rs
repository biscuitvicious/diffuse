//! Interactive modal views, dialogs, and fuzzy finder popups.

pub mod finder;
pub mod geometry;
pub mod help;
pub mod note;
pub mod path;

#[cfg(test)]
mod tests;

pub use finder::render_finder_modal_widget;
pub use geometry::{centered_rect, finder_modal_area, help_modal_area, standard_modal_area};
pub use help::{HELP_ENTRIES, render_help_modal_widget, wrap_words};
pub use note::render_note_modal_widget;
