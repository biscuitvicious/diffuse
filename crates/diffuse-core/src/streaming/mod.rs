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
    parse::DiffFile,
    pipeline::{PipelineOptions, process_single_diff_file},
    wire::OutputFile,
};

/// A self-contained diff job representing a single file in a changeset.
#[derive(Debug, Clone)]
pub struct FileJob {
    pub idx: usize,
    pub file: DiffFile,
    pub new_content: Option<Arc<String>>,
    pub old_content: Option<Arc<String>>,
    pub diff_fallback: Option<Arc<String>>,
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
/// Spawns an asynchronous streaming pipeline with `N = (num_cpus / 2).max(1)`
/// workers.
pub fn spawn_streaming_pipeline(
    worker_count: Option<usize>,
    opts: PipelineOptions,
) -> (StreamingProducer, StreamingConsumer) {
    let num_workers =
        worker_count.unwrap_or_else(|| available_parallelism().map(|p| p.get()).unwrap_or(4));

    let queue = Arc::new(PriorityJobQueue::new());
    let (processed_tx, processed_rx) = channel::<ProcessedFile>();
    let (output_tx, output_rx) = channel::<OutputFile>();

    let opts_arc = Arc::new(opts);

    let mut worker_handles = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let q = Arc::clone(&queue);
        let p_tx = processed_tx.clone();
        let opts = Arc::clone(&opts_arc);

        let handle = spawn(move || {
            while let Some(job) = q.pop() {
                let out_file = process_single_diff_file(
                    job.file,
                    job.new_content.as_deref().map(|s| s.as_str()),
                    job.old_content.as_deref().map(|s| s.as_str()),
                    job.diff_fallback.as_deref().map(|s| s.as_str()),
                    &opts,
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
/// pipeline using Arc<String> content maps to avoid allocations.
pub fn stream_diff_pipeline_arc(
    diff: Arc<String>,
    mut new_content: Option<std::collections::HashMap<String, Arc<String>>>,
    mut old_content: Option<std::collections::HashMap<String, Arc<String>>>,
    opts: PipelineOptions,
    worker_count: Option<usize>,
) -> StreamingConsumer {
    let parsed = crate::parse::parse_diff(&diff);
    let (producer, consumer) = spawn_streaming_pipeline(worker_count, opts);

    for (idx, f) in parsed.into_iter().enumerate() {
        let new_c = new_content.as_mut().and_then(|nc| nc.remove(&f.path));
        let old_c = old_content.as_mut().and_then(|oc| oc.remove(&f.path));
        producer.push(FileJob {
            idx,
            file: f,
            new_content: new_c,
            old_content: old_c,
            diff_fallback: None,
        });
    }
    producer.finish();

    consumer
}

/// Convenience helper to parse a diff and immediately spawn a streaming
/// pipeline.
pub fn stream_diff_pipeline(
    diff: impl Into<String>,
    new_content: Option<std::collections::HashMap<String, String>>,
    old_content: Option<std::collections::HashMap<String, String>>,
    opts: PipelineOptions,
    worker_count: Option<usize>,
) -> StreamingConsumer {
    let diff_arc: Arc<String> = Arc::new(diff.into());
    let new_arc = new_content.map(|m| m.into_iter().map(|(k, v)| (k, Arc::new(v))).collect());
    let old_arc = old_content.map(|m| m.into_iter().map(|(k, v)| (k, Arc::new(v))).collect());
    stream_diff_pipeline_arc(diff_arc, new_arc, old_arc, opts, worker_count)
}

/// Metadata discovered asynchronously from the git repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRepoMeta {
    pub branch: String,
    pub untracked_skipped: Vec<String>,
}

/// Spawns an asynchronous git streaming pipeline:
/// 1. Immediately starts the worker pool and sequencer.
/// 2. Spawns a background thread to load the git repository diff and blob
///    contents.
/// 3. Returns a `StreamingConsumer` receiving ordered `OutputFile`s and a
///    receiver for `GitRepoMeta`.
pub fn spawn_git_stream_pipeline(
    repo_root: std::path::PathBuf,
    base_args: Vec<String>,
    paths: Vec<String>,
    cached: bool,
    context: usize,
    opts: PipelineOptions,
    worker_count: Option<usize>,
) -> (StreamingConsumer, Receiver<Result<GitRepoMeta, String>>) {
    let (producer, consumer) = spawn_streaming_pipeline(worker_count, opts);
    let (meta_tx, meta_rx) = channel::<Result<GitRepoMeta, String>>();

    std::thread::spawn(move || {
        crate::git::stream_git_repo_to_producer(
            &repo_root, &base_args, &paths, cached, context, producer, &meta_tx,
        );
    });

    (consumer, meta_rx)
}

#[cfg(test)]
mod tests;
