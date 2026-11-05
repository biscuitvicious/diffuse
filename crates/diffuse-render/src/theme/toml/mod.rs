//! Declarative TOML theme engine.
//!
//! Composes the schema model (deserialization), the cascade compiler
//! (inheritance + cursor/kind resolution), and the public parser entry
//! point into a flat, cursor-tiered [`ParsedTheme`].

pub mod cascade;
pub mod model;
pub mod parse;

pub use parse::{ParsedTheme, ThemeParseError, parse_builtin, parse_theme_str};
