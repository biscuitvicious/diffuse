//! TUI viewport diff lines and unified frame rendering.

pub(crate) mod buffer;
pub(crate) mod ratatui;
pub(crate) mod viewport;

#[cfg(test)]
pub(crate) use buffer::escape_ansi_for_diagnostic;
