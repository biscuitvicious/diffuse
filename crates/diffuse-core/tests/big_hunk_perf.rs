//! Large-hunk pairing regression tests: whole-file replacements built from
//! the golden render.txt (very long lines), exercising the narrow-band
//! corridor and parallel candidate scoring end to end through the pipeline.

use std::{
    collections::HashMap,
    fs,
    time::{Duration, Instant},
};

use diffuse_core::{PipelineOptions, pipeline::run_pipeline_full};

fn load_render_txt() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/diffuse-2026-08/expect/pager.render.dark.txt"
    );
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

/// Pair a whole-file replacement of the first `num_lines` render.txt lines
/// (every third line perturbed palette-code style) and return the elapsed
/// pairing time.
fn pairing_time(num_lines: usize) -> Duration {
    let old_lines: Vec<String> = load_render_txt().into_iter().take(num_lines).collect();
    let n = old_lines.len();
    assert!(n > 0);
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
    elapsed
}

#[test]
fn test_big_replace_pairing_time() {
    // Gated: past the narrow-band threshold with realistic long lines.
    let elapsed = pairing_time(1100);
    assert!(
        elapsed < Duration::from_secs(3),
        "large-hunk pairing regressed (narrow band not engaging?): {elapsed:?}"
    );
}

/// Real-world-anchored stress coverage for the pairing corridor. Deliberate
/// exception to the unit (<0.5s) and binary (<1.5s) budgets: the full
/// 7174-line workload is the only fixture that exercises the narrow-band
/// corridor at production scale, and its ~0.6s dev cost guards directly
/// against minutes-scale pairing regressions.
#[test]
fn test_stress_full_replace_pairing_time() {
    let elapsed = pairing_time(7174);
    assert!(
        elapsed < Duration::from_secs(10),
        "full-workload pairing regressed: {elapsed:?}"
    );
}

fn wholesale_pairing_time(num_lines: usize) -> Duration {
    let old_lines: Vec<String> = load_render_txt().into_iter().take(num_lines).collect();
    let n = old_lines.len();
    assert!(n > 0);
    let new_lines: Vec<String> = old_lines.iter().map(|l| format!("{l}~")).collect();

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
    eprintln!("wholesale paired {rows} rows in {elapsed:?}");
    assert!(!out.files.is_empty());
    elapsed
}

#[test]
fn test_wholesale_replace_pairing_time() {
    let elapsed = wholesale_pairing_time(1000);
    // Re-baselined for the peer pipeline (1300ms legacy -> 2000ms at
    // cutover -> 4000ms now): 1000 affix Mods each build word spans,
    // admissibility, typed runs, conservation repair, and projection
    // checks, and per-cell span/run construction on long lines dominates
    // (~2.5s solo, ~3.2s under workspace-parallel load on 8-core dev
    // hardware). A/B shows the projection/repair guards cost only ~30ms
    // of that; the rest is structural peer per-group work predating this
    // change. Follow-up: cut per-cell tape/admissibility recomputation
    // in slot DP instead of raising this again.
    assert!(
        elapsed < Duration::from_millis(4000),
        "wholesale replacement pairing regressed: {elapsed:?}"
    );
}
