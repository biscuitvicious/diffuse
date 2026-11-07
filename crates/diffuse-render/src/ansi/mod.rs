//! Centralized ANSI escape sequences, color models, and terminal styling
//! primitives.
//!
//! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.

pub mod color;
pub mod optimize;
pub mod sgr;
pub mod style;

#[cfg(test)]
mod tests;

pub use yansi::{Color, Condition, Paint, Style};

pub use color::{AnsiColor, ColorExt};
pub use optimize::{optimize_ansi, optimize_ansi_line, write_optimized_ansi_line};
pub use sgr::{
    BG_RESET, FG_RESET, SGR_RESET, STRIKE_OFF, STRIKE_ON, decode_sgr, encode_sgr, sgr_off,
    sgr_off_code, sgr_on, sgr_on_str, strip_ansi,
};
pub use style::AnsiStyle;
