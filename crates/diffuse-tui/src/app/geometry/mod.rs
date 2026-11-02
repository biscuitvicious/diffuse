//! Geometry — pure width-dependent layout calculations.

pub mod divider;
pub mod scrollbar;
pub mod vis;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use scrollbar::{ScrollbarCell, ScrollbarSubcellThumb, lower_block_glyph};
pub(crate) use vis::CachedGeometry;
