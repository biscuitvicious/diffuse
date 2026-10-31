//! Asynchronous streaming worker pool and in-order reassembly sequencer.
//!
//! Provides priority job dispatching across worker threads (prioritizing early
//! files on screen) and in-order sequential stream reassembly for consumer TUI
//! or line-by-line CLI pagers.

use std::{
    cmp,
    collections::{BTreeMap, BinaryHeap, HashMap},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, channel},
    },
    thread::{JoinHandle, available_parallelism, spawn},
    time::Duration,
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
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Reverse order: smaller idx => Ordering::Greater
        other.0.idx.cmp(&self.0.idx)
    }
}

impl PartialOrd for PrioritizedJob {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
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

    /// Check if the queue is marked closed.
    pub fn is_closed(&self) -> bool {
        self.inner.lock().map(|s| s.is_closed).unwrap_or(true)
    }

    /// Pop the highest priority job without blocking, if available.
    pub fn try_pop(&self) -> Option<FileJob> {
        let mut state = self.inner.lock().ok()?;
        state.heap.pop().map(|j| j.0)
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
    queue: Arc<PriorityJobQueue>,
    processed_tx: Option<mpsc::Sender<ProcessedFile>>,
    opts: Arc<PipelineOptions>,
    _worker_handles: Vec<JoinHandle<()>>,
    _sequencer_handle: JoinHandle<()>,
}

impl StreamingConsumer {
    pub fn receiver(&self) -> &Receiver<OutputFile> {
        &self.rx
    }

    pub fn into_receiver(mut self) -> Receiver<OutputFile> {
        self.processed_tx.take();
        self.rx
    }
}

impl Iterator for StreamingConsumer {
    type Item = OutputFile;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.rx.try_recv() {
                Ok(file) => return Some(file),
                Err(mpsc::TryRecvError::Disconnected) => return None,
                Err(mpsc::TryRecvError::Empty) => {
                    if let Some(job) = self.queue.try_pop() {
                        let out_file = process_single_diff_file(
                            job.file,
                            job.new_content.as_deref().map(|s| s.as_str()),
                            job.old_content.as_deref().map(|s| s.as_str()),
                            job.diff_fallback.clone(),
                            &self.opts,
                        );
                        if let Some(ref tx) = self.processed_tx {
                            let _ = tx.send(ProcessedFile {
                                idx: job.idx,
                                file: out_file,
                            });
                        }
                    } else {
                        if self.queue.is_closed() {
                            self.processed_tx.take();
                            return self.rx.recv().ok();
                        }
                        match self.rx.recv_timeout(Duration::from_millis(10)) {
                            Ok(file) => return Some(file),
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
                        }
                    }
                }
            }
        }
    }
}

/// Returns the default worker count for the streaming pipeline: `(num_cpus *
/// 3 / 4).max(1)`.
pub fn default_worker_count() -> usize {
    let cpus = available_parallelism().map(|p| p.get()).unwrap_or(4);
    (cpus * 3 / 4).max(1)
}

/// Initializes the global rayon thread pool to `workers` threads (if not
/// already initialized).
pub fn init_thread_pool(workers: usize) {
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build_global();
}

/// Spawns an asynchronous streaming pipeline with `N = (num_cpus * 3 /
/// 4).max(1)` workers.
#[tracing::instrument(level = "trace", skip(opts), fields(workers = worker_count, visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn spawn_streaming_pipeline(
    worker_count: Option<usize>,
    opts: PipelineOptions,
) -> (StreamingProducer, StreamingConsumer) {
    let num_workers = worker_count
        .map(|c| c.max(1))
        .unwrap_or_else(default_worker_count);

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
                    job.diff_fallback.clone(),
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
        queue,
        processed_tx: Some(processed_tx),
        opts: opts_arc,
        _worker_handles: worker_handles,
        _sequencer_handle: sequencer_handle,
    };

    (producer, consumer)
}

/// Convenience helper to parse a diff and immediately spawn a streaming
/// pipeline using Arc<String> content maps to avoid allocations.
#[tracing::instrument(level = "trace", skip(diff, new_content, old_content, opts), fields(diff_bytes = diff.len(), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]
pub fn stream_diff_pipeline_arc(
    diff: Arc<String>,
    mut new_content: Option<HashMap<String, Arc<String>>>,
    mut old_content: Option<HashMap<String, Arc<String>>>,
    opts: PipelineOptions,
    worker_count: Option<usize>,
) -> StreamingConsumer {
    let (producer, consumer) = spawn_streaming_pipeline(worker_count, opts);

    let mut idx = 0;
    crate::parse::parse_diff_for_each(&diff, |f| {
        let new_c = new_content.as_mut().and_then(|nc| nc.remove(&f.path));
        let old_lookup = f.old_path.as_deref().unwrap_or(&f.path);
        let old_c = old_content
            .as_mut()
            .and_then(|oc| oc.remove(old_lookup).or_else(|| oc.remove(&f.path)));
        producer.push(FileJob {
            idx,
            file: f,
            new_content: new_c,
            old_content: old_c,
            diff_fallback: Some(Arc::clone(&diff)),
        });
        idx += 1;
    });
    producer.finish();

    consumer
}

/// Convenience helper to parse a diff and immediately spawn a streaming
/// pipeline.
pub fn stream_diff_pipeline(
    diff: impl Into<String>,
    new_content: Option<HashMap<String, String>>,
    old_content: Option<HashMap<String, String>>,
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
    pub commit_range: Option<String>,
}

/// Spawns an asynchronous git streaming pipeline:
/// 1. Immediately starts the worker pool and sequencer.
/// 2. Spawns a background thread to load the git repository diff and blob
///    contents.
/// 3. Returns a `StreamingConsumer` receiving ordered `OutputFile`s and a
///    receiver for `GitRepoMeta`.
#[tracing::instrument(level = "trace", skip(paths, opts), fields(repo_root = %repo_root.display(), base_args = ?base_args, path_count = paths.len(), staged, context, visual_context = opts.visual_context))]
pub fn spawn_git_stream_pipeline(
    repo_root: PathBuf,
    base_args: Vec<String>,
    paths: Vec<String>,
    staged: bool,
    context: usize,
    opts: PipelineOptions,
    worker_count: Option<usize>,
) -> (StreamingConsumer, Receiver<Result<GitRepoMeta, String>>) {
    let notes_db = opts.notes_db.clone();
    let target_repo_key = opts.target_repo_key.clone();
    let target_rev_key = opts.target_rev_key.clone();
    let (producer, consumer) = spawn_streaming_pipeline(worker_count, opts);
    let (meta_tx, meta_rx) = channel::<Result<GitRepoMeta, String>>();

    spawn(move || {
        crate::git::stream_git_repo_to_producer(
            &repo_root,
            &base_args,
            &paths,
            staged,
            context,
            producer,
            &meta_tx,
            notes_db.as_ref(),
            target_repo_key.as_deref(),
            target_rev_key.as_deref(),
        );
    });

    (consumer, meta_rx)
}

#[cfg(test)]
mod tests;
