//! Tests for the DP pairing bridge (fidelity loop).

use super::*;

/// Over the mirage-2025-08 corpus (the human-verified wire), the DP pairing
/// must reproduce the legacy pipeline's canonical output byte-for-byte.
/// This is the switchover's fidelity gate; failures name the deviation.
#[test]
fn dp_engine_mirage_wire_fidelity() {
    use serde::Deserialize;
    use std::{collections::HashMap, fs};

    #[derive(Deserialize)]
    struct Fixture {
        diff: String,
        contents: HashMap<String, String>,
    }

    let raw = fs::read_to_string(format!(
        "{}/tests/golden/mirage-2025-08.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let fx: Fixture = serde_json::from_str(&raw).unwrap();

    set_dp_engine(true);
    let dp_out = crate::bin_pipeline::run_pipeline(&fx.diff, &fx.contents);
    set_dp_engine(false);

    let target_raw = fs::read_to_string(format!(
        "{}/tests/golden/mirage-2025-08.dp.wire.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let target_val: serde_json::Value = serde_json::from_str(&target_raw).unwrap();
    let dp_val: serde_json::Value = serde_json::to_value(&dp_out).unwrap();

    if dp_val != target_val {
        let _dp_json = serde_json::to_string_pretty(&dp_out).unwrap();
        let _target_json = serde_json::to_string_pretty(&target_val).unwrap();
        panic!("DP engine deviates from mirage-2025-08.dp.wire.json");
    }
}
