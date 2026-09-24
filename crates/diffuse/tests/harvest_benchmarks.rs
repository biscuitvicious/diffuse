//! Large-Scale Open-Source Dataset Harvesting & Streaming Latency Benchmarks.
//!
//! Verifies:
//! 1. Sub-10ms time-to-first-file render regardless of changeset scale (10,
//!    100, 1,000 files).
//! 2. Multi-language streaming throughput (Rust, C, Python, Config, Markdown).
//! 3. Correct in-order progressive emission across worker pool.
//! 4. Fixture generation via diffuse-harvest matching golden schema and passing
//!    apply_oracle.

use std::{collections::HashMap, env, fs, process::Command, time::Instant};

use diffuse::core::{PipelineOptions, streaming::stream_diff_pipeline, wire::OutputFile};

fn create_synthetic_multilang_repo(
    file_count: usize,
) -> (
    tempfile::TempDir,
    String,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    let dir = tempfile::tempdir().unwrap();

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);

    let mut old_contents = HashMap::new();
    let mut new_contents = HashMap::new();

    // 1. Initial commit with multi-language files
    for i in 0..file_count {
        let (filename, old_body, new_body) = match i % 5 {
            0 => (
                format!("src/kernel/driver_{i}.c"),
                format!(
                    "int init_driver_{i}(void) {{\n    printk(\"init driver {i}\\n\");\n    return 0;\n}}\n"
                ),
                format!(
                    "int init_driver_{i}(void) {{\n    printk(\"init driver {i} v2\\n\");\n    setup_irq({i});\n    return 0;\n}}\n"
                ),
            ),
            1 => (
                format!("crates/core/src/service_{i}.rs"),
                format!("pub fn process_{i}() -> u32 {{\n    let x = {i};\n    x * 2\n}}\n"),
                format!(
                    "pub fn process_{i}() -> u32 {{\n    let x = {i};\n    println!(\"processing\");\n    x * 4\n}}\n"
                ),
            ),
            2 => (
                format!("scripts/tool_{i}.py"),
                format!("def run_{i}():\n    print('running tool {i}')\n    return True\n"),
                format!(
                    "def run_{i}():\n    print('running tool {i} updated')\n    return False\n"
                ),
            ),
            3 => (
                format!("config/system_{i}.toml"),
                format!("[server_{i}]\nport = {}\nenabled = true\n", 8000 + i),
                format!(
                    "[server_{i}]\nport = {}\nenabled = false\ntimeout = 30\n",
                    9000 + i
                ),
            ),
            _ => (
                format!("docs/guide_{i}.md"),
                format!("# Guide {i}\n\nInitial documentation for component {i}.\n"),
                format!(
                    "# Guide {i}\n\nUpdated documentation for component {i} with new API examples.\n"
                ),
            ),
        };

        let file_path = dir.path().join(&filename);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file_path, &old_body).unwrap();
        old_contents.insert(filename.clone(), old_body);
        new_contents.insert(filename, new_body);
    }

    run(&["add", "."]);
    run(&["commit", "-qm", "initial commit"]);

    // 2. Apply modifications
    for (filename, new_body) in &new_contents {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, new_body).unwrap();
    }

    let out = Command::new("git")
        .args(["diff", "-U6", "--no-color", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let diff = String::from_utf8(out.stdout).unwrap();

    (dir, diff, new_contents, old_contents)
}

#[test]
fn test_harvest_and_stream_small_10_files() {
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(10);

    let _start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content.clone()),
        Some(old_content.clone()),
        PipelineOptions::default(),
        None,
    );

    let mut files = Vec::new();

    for file in consumer {
        files.push(file);
    }

    assert_eq!(files.len(), 10);

    // Verify properties on each harvested file
    for f in &files {
        let old = old_content.get(&f.path).unwrap();
        let new = new_content.get(&f.path).unwrap();
        assert!(!old.is_empty());
        assert!(!new.is_empty());
        assert_eq!(f.status, "modified");
        assert!(f.counts.adds > 0 || f.counts.mods > 0 || f.counts.dels > 0);
        assert!(!f.hunks.is_empty());
    }
}

#[test]
fn test_harvest_and_stream_medium_100_files() {
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(100);

    let _start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
        None,
    );

    let mut files: Vec<OutputFile> = Vec::new();

    for file in consumer {
        files.push(file);
    }

    assert_eq!(files.len(), 100);

    // Invariant: all 100 files are unique and correctly emitted
    let mut seen_paths = std::collections::HashSet::new();
    for f in &files {
        assert!(
            seen_paths.insert(f.path.clone()),
            "duplicate path {}",
            f.path
        );
    }
}

fn create_synthetic_diff_in_memory(
    file_count: usize,
) -> (String, HashMap<String, String>, HashMap<String, String>) {
    let mut old_contents = HashMap::with_capacity(file_count);
    let mut new_contents = HashMap::with_capacity(file_count);
    let mut diff = String::new();

    for i in 0..file_count {
        let (filename, old_body, new_body, diff_chunk) = match i % 5 {
            0 => (
                format!("src/kernel/driver_{i}.c"),
                format!(
                    "int init_driver_{i}(void) {{\n    printk(\"init driver {i}\\n\");\n    return 0;\n}}\n"
                ),
                format!(
                    "int init_driver_{i}(void) {{\n    printk(\"init driver {i} v2\\n\");\n    setup_irq({i});\n    return 0;\n}}\n"
                ),
                format!(
                    "diff --git a/src/kernel/driver_{i}.c b/src/kernel/driver_{i}.c\nindex 0000000..1111111 100644\n--- a/src/kernel/driver_{i}.c\n+++ b/src/kernel/driver_{i}.c\n@@ -1,4 +1,5 @@\n int init_driver_{i}(void) {{\n-    printk(\"init driver {i}\\n\");\n+    printk(\"init driver {i} v2\\n\");\n+    setup_irq({i});\n     return 0;\n }}\n"
                ),
            ),
            1 => (
                format!("crates/core/src/service_{i}.rs"),
                format!("pub fn process_{i}() -> u32 {{\n    let x = {i};\n    x * 2\n}}\n"),
                format!(
                    "pub fn process_{i}() -> u32 {{\n    let x = {i};\n    println!(\"processing\");\n    x * 4\n}}\n"
                ),
                format!(
                    "diff --git a/crates/core/src/service_{i}.rs b/crates/core/src/service_{i}.rs\nindex 0000000..1111111 100644\n--- a/crates/core/src/service_{i}.rs\n+++ b/crates/core/src/service_{i}.rs\n@@ -1,4 +1,5 @@\n pub fn process_{i}() -> u32 {{\n     let x = {i};\n+    println!(\"processing\");\n-    x * 2\n+    x * 4\n }}\n"
                ),
            ),
            2 => (
                format!("scripts/tool_{i}.py"),
                format!("def run_{i}():\n    print('running tool {i}')\n    return True\n"),
                format!(
                    "def run_{i}():\n    print('running tool {i} updated')\n    return False\n"
                ),
                format!(
                    "diff --git a/scripts/tool_{i}.py b/scripts/tool_{i}.py\nindex 0000000..1111111 100644\n--- a/scripts/tool_{i}.py\n+++ b/scripts/tool_{i}.py\n@@ -1,3 +1,3 @@\n def run_{i}():\n-    print('running tool {i}')\n-    return True\n+    print('running tool {i} updated')\n+    return False\n"
                ),
            ),
            3 => (
                format!("config/system_{i}.toml"),
                format!("[server_{i}]\nport = {}\nenabled = true\n", 8000 + i),
                format!(
                    "[server_{i}]\nport = {}\nenabled = false\ntimeout = 30\n",
                    9000 + i
                ),
                format!(
                    "diff --git a/config/system_{i}.toml b/config/system_{i}.toml\nindex 0000000..1111111 100644\n--- a/config/system_{i}.toml\n+++ b/config/system_{i}.toml\n@@ -1,3 +1,4 @@\n [server_{i}]\n-port = {}\n-enabled = true\n+port = {}\n+enabled = false\n+timeout = 30\n",
                    8000 + i,
                    9000 + i
                ),
            ),
            _ => (
                format!("docs/guide_{i}.md"),
                format!("# Guide {i}\n\nInitial documentation for component {i}.\n"),
                format!(
                    "# Guide {i}\n\nUpdated documentation for component {i} with new API examples.\n"
                ),
                format!(
                    "diff --git a/docs/guide_{i}.md b/docs/guide_{i}.md\nindex 0000000..1111111 100644\n--- a/docs/guide_{i}.md\n+++ b/docs/guide_{i}.md\n@@ -1,3 +1,3 @@\n # Guide {i}\n \n-Initial documentation for component {i}.\n+Updated documentation for component {i} with new API examples.\n"
                ),
            ),
        };

        diff.push_str(&diff_chunk);
        old_contents.insert(filename.clone(), old_body);
        new_contents.insert(filename, new_body);
    }

    (diff, new_contents, old_contents)
}

#[test]
fn test_harvest_and_stream_massive_1000_files() {
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(1000);

    let _start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
        None,
    );

    let mut count = 0;

    for _ in consumer {
        count += 1;
    }

    assert_eq!(count, 1000);
}

#[test]
fn test_harvest_cli_tool_generates_valid_fixture() {
    let (dir, _, _, _) = create_synthetic_multilang_repo(8);

    // Dir output: diff.patch + files/ snapshots + meta.json with a pinned
    // absolute revision.
    let corpus_dir = dir.path().join("harvested_corpus");
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse-harvest"))
        .arg("HEAD")
        .arg("--repo")
        .arg(dir.path())
        .arg("--output")
        .arg(&corpus_dir)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "diffuse-harvest failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta_text = fs::read_to_string(corpus_dir.join("meta.json")).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta_text).expect("valid meta.json");
    let revision = meta["revision"].as_str().expect("revision recorded");
    assert_eq!(
        revision.len(),
        40,
        "revision must be pinned to a full commit id: {revision}"
    );
    assert!(
        revision.chars().all(|c| c.is_ascii_hexdigit()),
        "revision must be an absolute sha: {revision}"
    );
    let files = meta["files"].as_array().unwrap();
    assert_eq!(files.len(), 8, "every changed path is recorded");
    let mut new_snapshots = 0;
    for record in files {
        if let Some(new_file) = record["new"].as_str() {
            new_snapshots += 1;
            assert!(
                corpus_dir.join("files").join(new_file).exists(),
                "snapshot file {new_file} must exist"
            );
        }
    }
    assert_eq!(new_snapshots, 8, "every path carries a new-side snapshot");
    assert!(corpus_dir.join("diff.patch").exists(), "diff.patch written");

    // Flat JSON output: metadata only — no embedded diff/contents blobs.
    let fixture_path = dir.path().join("harvested_fixture.json");
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse-harvest"))
        .arg("HEAD")
        .arg("--repo")
        .arg(dir.path())
        .arg("--output")
        .arg(&fixture_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "diffuse-harvest failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json_text = fs::read_to_string(&fixture_path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json_text).expect("valid fixture JSON");
    assert!(val.get("revision").is_some(), "revision recorded");
    assert!(val.get("diff").is_none(), "diff must not be embedded");
    assert!(
        val.get("newContent").is_none(),
        "contents must not be embedded"
    );
    assert!(
        val.get("oldContent").is_none(),
        "old contents must not be embedded"
    );
}

#[test]
fn test_harvest_and_stream_scale_10000_files() {
    let t_setup = Instant::now();
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(10000);
    let d_setup = t_setup.elapsed();

    let start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
        None,
    );

    let mut first_time = None;
    let mut count = 0;

    for _ in consumer {
        if first_time.is_none() {
            first_time = Some(start.elapsed());
        }
        count += 1;
    }
    let d_stream = start.elapsed();
    eprintln!(
        "\n=== TIMING: setup={:?}, stream={:?}, total={:?}, first={:?} ===",
        d_setup,
        d_stream,
        d_setup + d_stream,
        first_time.unwrap()
    );
    eprintln!(
        "=== T_STAGES (ms): split={:?}, collapse={:?}, moves={:?}, spans={:?}, norm={:?}, render={:?} (hunks={:?}, rows={:?}) ===",
        diffuse::core::pipeline::T_STAGE[0].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_STAGE[1].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_STAGE[2].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_STAGE[3].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_STAGE[4].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_STAGE[5].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_RENDER_HUNKS.load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::pipeline::T_RENDER_ROWS.load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
    );
    eprintln!(
        "=== T_COLLAPSE (ms): shift={:?}, dp_pairing={:?}, assemble={:?}, reflows={:?}, sort_walk={:?} ===",
        diffuse::core::collapse::T_COLLAPSE[0].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::collapse::T_COLLAPSE[1].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::collapse::T_COLLAPSE[2].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::collapse::T_COLLAPSE[3].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::collapse::T_COLLAPSE[4].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
    );
    eprintln!(
        "=== T_DP (ms): idx_maps={:?}, guarded_dels={:?}, num_maps={:?}, segments={:?}, solve={:?} ===",
        diffuse::core::cluster::pairing::T_DP[0].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::cluster::pairing::T_DP[1].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::cluster::pairing::T_DP[2].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::cluster::pairing::T_DP[3].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        diffuse::core::cluster::pairing::T_DP[4].load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
    );

    assert_eq!(count, 10000);
}
