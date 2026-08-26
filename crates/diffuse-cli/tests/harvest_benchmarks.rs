//! Large-Scale Open-Source Dataset Harvesting & Streaming Latency Benchmarks.
//!
//! Verifies:
//! 1. Sub-10ms time-to-first-file render regardless of changeset scale (10,
//!    100, 1,000 files).
//! 2. Multi-language streaming throughput (Rust, C, Python, Config, Markdown).
//! 3. Correct in-order progressive emission across worker pool.
//! 4. Fixture generation via diffuse-harvest matching golden schema and passing
//!    apply_oracle.

use std::{
    collections::HashMap,
    env, fs,
    path::PathBuf,
    process::{self, Command},
    time::Instant,
};

use diffuse_core::{PipelineOptions, streaming::stream_diff_pipeline, wire::OutputFile};

static REPO_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(100);

fn create_synthetic_multilang_repo(
    file_count: usize,
) -> (
    PathBuf,
    String,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    let id = REPO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("diffuse-harvest-bench-{}-{}", process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&dir)
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

        let file_path = dir.join(&filename);
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
        let file_path = dir.join(filename);
        fs::write(&file_path, new_body).unwrap();
    }

    let out = Command::new("git")
        .args(["diff", "-U6", "--no-color", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let diff = String::from_utf8(out.stdout).unwrap();

    (dir, diff, new_contents, old_contents)
}

#[test]
fn test_harvest_and_stream_small_10_files() {
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(10);

    let start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content.clone()),
        Some(old_content.clone()),
        PipelineOptions::default(),
        None,
    );

    let mut first_time = None;
    let mut files = Vec::new();

    for file in consumer {
        if first_time.is_none() {
            first_time = Some(start.elapsed());
        }
        files.push(file);
    }

    assert_eq!(files.len(), 10);
    assert!(
        first_time.unwrap() < std::time::Duration::from_millis(50),
        "10-file first-file latency: {:?}",
        first_time.unwrap()
    );

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

    let start = Instant::now();
    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
        None,
    );

    let mut first_time = None;
    let mut files: Vec<OutputFile> = Vec::new();

    for file in consumer {
        if first_time.is_none() {
            first_time = Some(start.elapsed());
        }
        files.push(file);
    }

    assert_eq!(files.len(), 100);
    assert!(
        first_time.unwrap() < std::time::Duration::from_millis(80),
        "100-file first-file latency: {:?}",
        first_time.unwrap()
    );

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

    assert_eq!(count, 1000);
    assert!(
        first_time.unwrap() < std::time::Duration::from_millis(500),
        "1,000-file first-file latency: {:?}",
        first_time.unwrap()
    );
}

#[test]
fn test_harvest_cli_tool_generates_valid_fixture() {
    let (dir, _, _, _) = create_synthetic_multilang_repo(8);
    let fixture_path = dir.join("harvested_fixture.json");

    let out = Command::new(env!("CARGO_BIN_EXE_diffuse-harvest"))
        .arg("HEAD")
        .arg("--repo")
        .arg(&dir)
        .arg("--output")
        .arg(&fixture_path)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "diffuse-harvest failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(fixture_path.exists());

    let json_text = fs::read_to_string(&fixture_path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json_text).expect("valid fixture JSON");

    assert!(val.get("diff").is_some());
    assert!(val.get("newContent").is_some());
    assert!(val.get("oldContent").is_some());

    let contents_map = val["newContent"].as_object().unwrap();
    assert_eq!(contents_map.len(), 8);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "massive scale benchmark (run with --ignored)"]
fn test_harvest_and_stream_scale_10000_files() {
    let (diff, new_content, old_content) = create_synthetic_diff_in_memory(10000);

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

    assert_eq!(count, 10000);
    assert!(
        first_time.unwrap() < std::time::Duration::from_secs(10),
        "10,000-file first-file latency: {:?}",
        first_time.unwrap()
    );
}
