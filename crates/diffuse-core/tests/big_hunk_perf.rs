//! TEMPORARY probe: time mod-pairing on a synthetic whole-file replacement
//! hunk built from the golden render.txt (≈7174 lines per side, every line
//! perturbed so pairing engages across the full corridor).

use std::{collections::HashMap, time::Instant};

use diffuse_core::{PipelineOptions, pipeline::run_pipeline_full};

fn load_render_txt() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/diffuse-2026-08/render.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

#[test]
#[ignore = "manual perf probe: minutes-long pairing workload"]
fn test_probe_big_replace_pairing_time() {
    let old_lines = load_render_txt();
    let n = old_lines.len();
    eprintln!("old lines: {n}");
    // Perturb every line slightly (palette-code style tweak) so nearly every
    // del/add pair shares most tokens: worst case for the pairing corridor.
    let new_lines: Vec<String> = old_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i % 3 == 0 {
                format!("{l}~")
            } else {
                l.clone()
            }
        })
        .collect();

    let mut diff = format!(
        "diff --git a/render.txt b/render.txt\n--- a/render.txt\n+++ b/render.txt\n@@ -1,{n} +1,{n} @@\n"
    );
    for l in &old_lines {
        diff.push('-');
        diff.push_str(l);
        diff.push('\n');
    }
    for l in &new_lines {
        diff.push('+');
        diff.push_str(l);
        diff.push('\n');
    }
    let mut new_content = HashMap::new();
    new_content.insert("render.txt".to_string(), new_lines.join("\n") + "\n");
    let mut old_content = HashMap::new();
    old_content.insert("render.txt".to_string(), old_lines.join("\n") + "\n");

    let t = Instant::now();
    let out = run_pipeline_full(
        &diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let elapsed = t.elapsed();
    let rows: usize = out.files.iter().map(|f| f.rows.len()).sum();
    eprintln!("paired {rows} rows in {elapsed:?}");
    assert!(!out.files.is_empty());
}
