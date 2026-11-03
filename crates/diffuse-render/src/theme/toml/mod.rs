//! Declarative TOML theme engine.
//!
//! Composes the schema model (deserialization), the cascade compiler
//! (inheritance + scope/kind resolution), and the public parser entry
//! point into a flat, scoped [`ParsedTheme`].

pub mod cascade;
pub mod model;
pub mod parse;

pub use parse::{ParsedTheme, ThemeParseError, parse_theme_str};
