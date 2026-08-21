//! Asynchronous streaming worker pool and in-order reassembly sequencer.
//!
//! Provides priority job dispatching across worker threads (prioritizing early
//! files on screen) and in-order sequential stream reassembly for consumer TUI
//! or line-by-line CLI pagers.

use std::{
    collections::{BTreeMap, BinaryHeap},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{Receiver, channel},
    },
    thread::{JoinHandle, available_parallelism, spawn},
};

use crate::{
    notes::NotesDB, parse::DiffFile, pipeline::process_single_diff_file, wire::OutputFile,
};

/// A self-contained diff job representing a single file in a changeset.
#[derive(Debug, Clone)]
pub struct FileJob {
    pub idx: usize,
    pub file: DiffFile,
    pub new_content: Option<String>,
    pub old_content: Option<String>,
    pub diff_fallback: Option<String>,
}

/// Priority wrapper for `FileJob`.
///
/// In `BinaryHeap` (a max-heap), we invert index comparison so that jobs with
/// smaller indices (`idx = 0, 1, 2...`) have the highest priority.
#[derive(Debug)]
struct PrioritizedJob(FileJob);

impl PartialEq for PrioritizedJob {
    fn eq(&self, other: &Self) -> bool {
        self.0.idx == other.0.idx
    }
}
impl Eq for PrioritizedJob {}

impl Ord for PrioritizedJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order: smaller idx => Ordering::Greater
        other.0.idx.cmp(&self.0.idx)
    }
}

impl PartialOrd for PrioritizedJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A processed diff file with its sequence index.
#[derive(Debug)]
pub struct ProcessedFile {
    pub idx: usize,
    pub file: OutputFile,
}

/// Inner state of the priority job queue.
struct QueueState {
    heap: BinaryHeap<PrioritizedJob>,
    is_closed: bool,
}

/// Thread-safe priority job queue for worker threads.
pub struct PriorityJobQueue {
    inner: Mutex<QueueState>,
    cond: Condvar,
}

impl PriorityJobQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(QueueState {
                heap: BinaryHeap::new(),
                is_closed: false,
            }),
            cond: Condvar::new(),
        }
    }

    /// Push a job into the priority queue and wake an available worker.
    pub fn push(&self, job: FileJob) {
        let mut state = self.inner.lock().unwrap();
        if state.is_closed {
            return;
        }
        state.heap.push(PrioritizedJob(job));
        self.cond.notify_one();
    }

    /// Mark the queue as closed (no more jobs will be added).
    pub fn close(&self) {
        let mut state = self.inner.lock().unwrap();
        state.is_closed = true;
        self.cond.notify_all();
    }

    /// Pop the highest priority (lowest index) job, blocking until available.
    pub fn pop(&self) -> Option<FileJob> {
        let mut state = self.inner.lock().unwrap();
        loop {
            if let Some(job) = state.heap.pop() {
                return Some(job.0);
            }
            if state.is_closed {
                return None;
            }
            state = self.cond.wait(state).unwrap();
        }
    }
}

impl Default for PriorityJobQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Reassembles out-of-order completed files into strict in-order sequential
/// output.
#[derive(Debug, Default)]
pub struct FileSequencer {
    next_expected_idx: usize,
    buffer: BTreeMap<usize, OutputFile>,
}

impl FileSequencer {
    pub fn new() -> Self {
        Self {
            next_expected_idx: 0,
            buffer: BTreeMap::new(),
        }
    }

    /// Insert a processed file and drain any contiguous sequential files ready.
    pub fn push(&mut self, item: ProcessedFile) -> Vec<OutputFile> {
        self.buffer.insert(item.idx, item.file);
        let mut ready = Vec::new();
        while let Some(file) = self.buffer.remove(&self.next_expected_idx) {
            ready.push(file);
            self.next_expected_idx += 1;
        }
        ready
    }

    /// Check if the reassembly buffer is completely drained.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the next expected sequence index.
    pub fn next_expected(&self) -> usize {
        self.next_expected_idx
    }
}

/// Streaming producer to submit file jobs to the worker pool.
pub struct StreamingProducer {
    queue: Arc<PriorityJobQueue>,
}

impl StreamingProducer {
    pub fn push(&self, job: FileJob) {
        self.queue.push(job);
    }

    pub fn finish(self) {
        self.queue.close();
    }
}

/// Streaming consumer receiving sequentially ordered `OutputFile`s.
pub struct StreamingConsumer {
    rx: Receiver<OutputFile>,
    _worker_handles: Vec<JoinHandle<()>>,
    _sequencer_handle: JoinHandle<()>,
}

impl StreamingConsumer {
    pub fn receiver(&self) -> &Receiver<OutputFile> {
        &self.rx
    }

    pub fn into_receiver(self) -> Receiver<OutputFile> {
        self.rx
    }
}

impl Iterator for StreamingConsumer {
    type Item = OutputFile;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()
    }
}

/// Spawns an asynchronous streaming pipeline with `N = (num_cpus / 2).max(1)`
/// workers.
pub fn spawn_streaming_pipeline(
    worker_count: Option<usize>,
    notes_db: Option<NotesDB>,
    repo: Option<String>,
) -> (StreamingProducer, StreamingConsumer) {
    let num_workers = worker_count
        .unwrap_or_else(|| (available_parallelism().map(|p| p.get()).unwrap_or(2) / 2).max(1));

    let queue = Arc::new(PriorityJobQueue::new());
    let (processed_tx, processed_rx) = channel::<ProcessedFile>();
    let (output_tx, output_rx) = channel::<OutputFile>();

    let notes_arc = notes_db.map(Arc::new);
    let repo_arc = repo.map(Arc::new);

    let mut worker_handles = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let q = Arc::clone(&queue);
        let p_tx = processed_tx.clone();
        let n_opt = notes_arc.clone();
        let r_opt = repo_arc.clone();

        let handle = spawn(move || {
            while let Some(job) = q.pop() {
                let out_file = process_single_diff_file(
                    job.file,
                    job.new_content.as_deref(),
                    job.old_content.as_deref(),
                    job.diff_fallback.as_deref(),
                    n_opt.as_deref(),
                    r_opt.as_deref().map(std::ops::Deref::deref),
                );
                if p_tx
                    .send(ProcessedFile {
                        idx: job.idx,
                        file: out_file,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        worker_handles.push(handle);
    }
    drop(processed_tx); // Drop initial sender so processed_rx terminates when all workers finish

    let sequencer_handle = spawn(move || {
        let mut sequencer = FileSequencer::new();
        while let Ok(item) = processed_rx.recv() {
            let ready = sequencer.push(item);
            for file in ready {
                if output_tx.send(file).is_err() {
                    return;
                }
            }
        }
    });

    let producer = StreamingProducer {
        queue: Arc::clone(&queue),
    };

    let consumer = StreamingConsumer {
        rx: output_rx,
        _worker_handles: worker_handles,
        _sequencer_handle: sequencer_handle,
    };

    (producer, consumer)
}

/// Convenience helper to parse a diff and immediately spawn a streaming
/// pipeline.
pub fn stream_diff_pipeline(
    diff: &str,
    contents: &std::collections::HashMap<String, String>,
    old_contents: Option<&std::collections::HashMap<String, String>>,
    notes_db: Option<NotesDB>,
    repo: Option<String>,
    worker_count: Option<usize>,
) -> StreamingConsumer {
    let parsed = crate::parse::parse_diff(diff);
    let (producer, consumer) = spawn_streaming_pipeline(worker_count, notes_db, repo);

    let diff_fallback = diff.to_string();
    for (idx, f) in parsed.into_iter().enumerate() {
        let new_c = contents.get(&f.path).cloned();
        let old_c = old_contents.and_then(|oc| oc.get(&f.path).cloned());
        producer.push(FileJob {
            idx,
            file: f,
            new_content: new_c,
            old_content: old_c,
            diff_fallback: Some(diff_fallback.clone()),
        });
    }
    producer.finish();

    consumer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{counts::FileCounts, parse::parse_diff, wire::PipelineOutput};

    #[test]
    fn test_file_sequencer_in_order_reassembly() {
        let mut seq = FileSequencer::new();

        let make_file = |name: &str| OutputFile {
            path: name.to_string(),
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
            },
            fences: Vec::new(),
            notes: Vec::new(),
            content_runs: Vec::new(),
            rows: Vec::new(),
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
                status: crate::parse::FileStatus::Modified,
                hunks: Vec::new(),
                raw_hunks: Vec::new(),
                new_line_count: 0,
                old_line_count: 0,
                new_content: Vec::new(),
                old_content: Vec::new(),
                no_content: false,
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
        let contents = [
            ("a.rs".to_string(), "fn a_mod() {}\n".to_string()),
            ("b.rs".to_string(), "fn b_mod() {}\n".to_string()),
            ("c.rs".to_string(), "fn c_mod() {}\n".to_string()),
        ]
        .into_iter()
        .collect();

        // Run sequential baseline
        let baseline = crate::pipeline::run_pipeline_full(diff, &contents, None, None, None);

        // Run concurrent streaming pipeline
        let (producer, consumer) = spawn_streaming_pipeline(Some(4), None, None);

        for (idx, f) in files.into_iter().enumerate() {
            let nc = contents.get(&f.path).cloned();
            producer.push(FileJob {
                idx,
                file: f,
                new_content: nc,
                old_content: None,
                diff_fallback: Some(diff.to_string()),
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
        let diff = r#"diff --git a/x.rs b/x.rs
index 000..111 100644
--- a/x.rs
+++ b/x.rs
@@ -1,1 +1,1 @@
-const X: i32 = 1;
+const X: i32 = 2;
"#;
        let mut contents = std::collections::HashMap::new();
        contents.insert("x.rs".to_string(), "const X: i32 = 2;\n".to_string());

        let consumer = stream_diff_pipeline(diff, &contents, None, None, None, Some(2));
        let files: Vec<OutputFile> = consumer.collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "x.rs");
        assert_eq!(files[0].counts.mods, 1);
    }
}
