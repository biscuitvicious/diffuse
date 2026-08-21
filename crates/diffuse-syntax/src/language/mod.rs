//! Language profile definitions — one submodule per language family.
//!
//! Each submodule exposes:
//! - `KEYWORDS: &[&str]` — sorted slice for binary-search lookup (where
//!   applicable)
//! - `profile() -> LangProfile` — construct the language's static profile
//!
//! The registry (`super::registry`) uses these to build and cache profiles
//! keyed by file extension.

pub mod c_family;
pub mod config;
pub mod go;
pub mod markdown;
pub mod python;
pub mod rust;
pub mod shell;
pub mod web;
