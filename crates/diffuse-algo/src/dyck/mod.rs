//! Dyck Language Delimiter Balancer and Sequence Geometry.
//!
//! Academic foundation:
//! - Walther von Dyck (1882). "Gruppentheoretische Studien". Mathematische
//!   Annalen, 20(1), 1–44.
//! - Noam Chomsky (1956). "Three Models for the Description of Language". IRE
//!   Transactions on Information Theory, 2(3), 113–124.
//!
//! # Strict Architectural Separation
//! Operates over generic streams `Iterator<Item = T>` and slices `&[T]`.
//! Zero diffuse-domain dependencies. Stack allocation via [`SmallStack`] avoids
//! heap allocation for nesting depths up to 32.

pub mod classifier;
pub mod containers;
pub mod iter;
pub mod partners;
pub mod stack;

#[cfg(test)]
mod tests;

pub use classifier::{DelimiterCategory, DelimiterClassifier, UniversalDelimiterClassifier};
pub use containers::{
    has_trailing_delimiter_expansion, terminal_dyck_stack, track_enclosing_containers,
    track_enclosing_containers_into, track_nesting_depths_into, track_nesting_depths_with_prev,
    track_surrounding_containers, track_surrounding_containers_into,
};
pub use iter::{DiffuseDyckExt, DyckBalanced};
pub use partners::{
    compute_delimiter_partners, compute_delimiter_partners_into, trim_balanced_boundaries,
};
pub use stack::SmallStack;
