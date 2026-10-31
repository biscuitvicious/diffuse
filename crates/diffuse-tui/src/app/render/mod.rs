//! TUI viewport diff lines and dual frame renderer (ANSI and Ratatui).

pub(crate) mod ansi;
pub(crate) mod ratatui;
pub(crate) mod viewport;

#[cfg(test)]
pub(crate) use ansi::escape_ansi_for_diagnostic;
