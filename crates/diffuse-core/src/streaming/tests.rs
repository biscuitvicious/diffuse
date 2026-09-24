use super::*;
use crate::{counts::FileCounts, parse::parse_diff, wire::PipelineOutput};

#[test]
fn test_file_sequencer_in_order_reassembly() {
    let mut seq = FileSequencer::new();

    let make_file = |name: &str| OutputFile {
        path: name.to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 0,
        old_line_count: 0,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks: Vec::new(),
        raw_hunks: Vec::new(),
        counts: FileCounts {
            adds: 0,
            dels: 0,
            movs: 0,
            mods: 0,
            notes: 0,
        },
        fences: Vec::new(),
        notes: Vec::new(),
        content_runs: Vec::new(),
        rows: Vec::new(),
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };

    // Receive out of order: 2, 0, 1, 4, 3
    let r2 = seq.push(ProcessedFile {
        idx: 2,
        file: make_file("f2"),
    });
    assert!(r2.is_empty(), "cannot emit 2 before 0 and 1");

    let r0 = seq.push(ProcessedFile {
        idx: 0,
        file: make_file("f0"),
    });
    assert_eq!(r0.len(), 1, "emits 0 immediately");
    assert_eq!(r0[0].path, "f0");

    let r1 = seq.push(ProcessedFile {
        idx: 1,
        file: make_file("f1"),
    });
    assert_eq!(r1.len(), 2, "emits 1 and buffered 2");
    assert_eq!(r1[0].path, "f1");
    assert_eq!(r1[1].path, "f2");

    let r4 = seq.push(ProcessedFile {
        idx: 4,
        file: make_file("f4"),
    });
    assert!(r4.is_empty(), "cannot emit 4 before 3");

    let r3 = seq.push(ProcessedFile {
        idx: 3,
        file: make_file("f3"),
    });
    assert_eq!(r3.len(), 2, "emits 3 and buffered 4");
    assert_eq!(r3[0].path, "f3");
    assert_eq!(r3[1].path, "f4");
    assert!(seq.is_empty());
}

#[test]
fn test_priority_job_queue_order() {
    let q = PriorityJobQueue::new();

    let make_job = |idx: usize| FileJob {
        idx,
        file: DiffFile {
            path: format!("file_{idx}.rs"),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: crate::parse::FileStatus::Modified,
            hunks: Vec::new(),
            raw_hunks: Vec::new(),
            new_line_count: 0,
            old_line_count: 0,
            new_content: Vec::new(),
            old_content: Vec::new(),
            no_content: false,
            is_binary: false,
            old_bytes: None,
            new_bytes: None,
        },

        new_content: None,
        old_content: None,
        diff_fallback: None,
    };

    // Push out of order
    q.push(make_job(5));
    q.push(make_job(1));
    q.push(make_job(0));
    q.push(make_job(3));
    q.close();

    assert_eq!(q.pop().unwrap().idx, 0);
    assert_eq!(q.pop().unwrap().idx, 1);
    assert_eq!(q.pop().unwrap().idx, 3);
    assert_eq!(q.pop().unwrap().idx, 5);
    assert!(q.pop().is_none());
}

#[test]
fn test_streaming_pipeline_concurrent_reassembly() {
    let diff = r#"diff --git a/a.rs b/a.rs
index 000..111 100644
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,2 @@
-fn a() {}
+fn a_mod() {}
diff --git a/b.rs b/b.rs
index 222..333 100644
--- a/b.rs
+++ b/b.rs
@@ -1,2 +1,2 @@
-fn b() {}
+fn b_mod() {}
diff --git a/c.rs b/c.rs
index 444..555 100644
--- a/c.rs
+++ b/c.rs
@@ -1,2 +1,2 @@
-fn c() {}
+fn c_mod() {}
"#;
    let files = parse_diff(diff);
    let new_content: std::collections::HashMap<String, String> = [
        ("a.rs".to_string(), "fn a_mod() {}\n".to_string()),
        ("b.rs".to_string(), "fn b_mod() {}\n".to_string()),
        ("c.rs".to_string(), "fn c_mod() {}\n".to_string()),
    ]
    .into_iter()
    .collect();

    // Run sequential baseline
    let baseline = crate::pipeline::run_pipeline_full(
        diff,
        Some(new_content.clone()),
        None,
        PipelineOptions::default(),
    );

    // Run concurrent streaming pipeline
    let (producer, consumer) = spawn_streaming_pipeline(Some(4), PipelineOptions::default());

    for (idx, f) in files.into_iter().enumerate() {
        let nc = new_content.get(&f.path).map(|s| Arc::new(s.clone()));
        producer.push(FileJob {
            idx,
            file: f,
            new_content: nc,
            old_content: None,
            diff_fallback: Some(Arc::new(diff.to_string())),
        });
    }
    producer.finish();

    let streamed_files: Vec<OutputFile> = consumer.collect();
    assert_eq!(streamed_files.len(), 3);
    assert_eq!(streamed_files[0].path, "a.rs");
    assert_eq!(streamed_files[1].path, "b.rs");
    assert_eq!(streamed_files[2].path, "c.rs");

    let streamed_out = PipelineOutput {
        files: streamed_files,
        branch: String::new(),
        untracked_skipped: Vec::new(),
    };

    // Assert identical output to full pipeline
    assert_eq!(
        serde_json::to_string(&streamed_out).unwrap(),
        serde_json::to_string(&baseline).unwrap(),
        "streaming pipeline output must match run_pipeline_full byte-for-byte"
    );
}

#[test]
fn test_stream_diff_pipeline_helper() {
    let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,1 +1,1 @@\n-const X: i32 = 1;\n+const X: i32 = 2;\n";
    let mut new_content = std::collections::HashMap::new();
    new_content.insert("x.rs".to_string(), "const X: i32 = 2;\n".to_string());

    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
        Some(2),
    );
    let files: Vec<OutputFile> = consumer.collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "x.rs");
    assert_eq!(files[0].counts.mods, 1);
}

#[test]
fn test_default_worker_count() {
    let cpus = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4);
    let expected = (cpus * 3 / 4).max(1);
    assert_eq!(default_worker_count(), expected);
}
