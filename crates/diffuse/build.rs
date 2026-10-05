//! Build script for the `diffuse` crate.
//!
//! Declares `rerun-if-changed` hints for every workspace library crate that
//! the `diffuse` binary links against, so that `cargo build` (and therefore
//! `cargo test`) always re-links — and thus re-places — `target/debug/diffuse`
//! whenever any library source file changes.
//!
//! Without this, Cargo's incremental build may determine that `src/main.rs`
//! itself has not changed and skip the final link step, leaving the
//! `target/debug/diffuse` hard-link pointing at a stale artifact.  Integration
//! tests that spawn the binary via `env!("CARGO_BIN_EXE_diffuse")` would then
//! run the old binary against freshly-regenerated golden fixtures, causing
//! spurious mismatches.

fn main() {
    // Re-run this build script (and re-link the binary) whenever any source
    // file in the workspace library crates changes.
    for dir in &[
        "../../crates/diffuse-algo/src",
        "../../crates/diffuse-core/src",
        "../../crates/diffuse-render/src",
        "../../crates/diffuse-syntax/src",
        "../../crates/diffuse-tui/src",
    ] {
        println!("cargo:rerun-if-changed={dir}");
    }
}
