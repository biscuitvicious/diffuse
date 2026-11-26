//! Cluster alignment configuration and thread-local scratch arena.

pub mod arena;
pub mod config;

pub use arena::{DpScratchArena, with_tls_arena};
pub use config::{
    DEFAULT_CLUSTER_BUDGET, DEFAULT_CLUSTER_PRODUCT_CAP, PATH_GAP_COST, cluster_budget,
    cluster_product_cap, set_cluster_budget, set_cluster_product_cap,
};
