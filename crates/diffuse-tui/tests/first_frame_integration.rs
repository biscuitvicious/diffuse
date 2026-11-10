//! Startup latency coverage for the interactive loop.
//!
//! The loop renders through a [`Session`]; this test supplies a session backed
//! by an in-memory Ratatui buffer so the first presented frame can be inspected
//! and timed without a terminal.

use std::{
    cell::Cell,
    io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use diffuse_tui::{
    AppState, PollOutcome, Session,
    core::{
        DEFAULT_EXPAND_LINES, PipelineOptions, Repo, ReviewTarget, pipeline::run_pipeline_full,
        wire::PipelineOutput,
    },
    render::DiffuseTheme,
    run_session,
};
use ratatui::{Terminal, backend::TestBackend};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

/// Ceiling for presenting the first frame. The stall this test guards against
/// was measured in seconds, so a quarter second leaves ample margin.
const FIRST_FRAME_BUDGET: Duration = Duration::from_millis(250);

fn sample_output() -> PipelineOutput {
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n fn hello() {\n-    println!(\"old\");\n+    println!(\"new\");\n }\n";
    run_pipeline_full(diff, None, None, PipelineOptions::default())
}

/// A session that renders into an in-memory buffer, records every frame, and
/// ends the loop after `polls_left` idle polls.
struct BufferedSession {
    terminal: Terminal<TestBackend>,
    polls_left: Cell<usize>,
    drawn: usize,
    first_frame: Option<(Instant, String)>,
    frames: Vec<String>,
}

impl BufferedSession {
    fn new(polls_left: usize) -> Self {
        let backend = TestBackend::new(WIDTH, HEIGHT);
        Self {
            terminal: Terminal::new(backend).expect("in-memory terminal must construct"),
            polls_left: Cell::new(polls_left),
            drawn: 0,
            first_frame: None,
            frames: Vec::new(),
        }
    }

    /// Flatten the rendered buffer into text with one line per terminal row.
    fn screen_text(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }
}

impl Session for BufferedSession {
    fn poll_outcome(&self, _timeout: Duration) -> PollOutcome {
        let remaining = self.polls_left.get();
        if remaining == 0 {
            return PollOutcome::Disconnected;
        }
        self.polls_left.set(remaining - 1);
        PollOutcome::Idle
    }

    fn size(&self) -> (u16, u16) {
        (WIDTH, HEIGHT)
    }

    fn draw_frame(&mut self, state: &mut AppState) -> io::Result<()> {
        self.terminal
            .draw(|frame| state.render_ratatui_frame(frame))
            .map_err(io::Error::other)?;
        self.drawn += 1;
        let screen = self.screen_text();
        if self.first_frame.is_none() {
            self.first_frame = Some((Instant::now(), screen.clone()));
        }
        self.frames.push(screen);
        Ok(())
    }

    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A streamed file large enough that building its view is not free.
fn large_streamed_file() -> diffuse_tui::core::wire::OutputFile {
    let mut diff = String::from(
        "diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -1,200 +1,200 @@\n",
    );
    for i in 0..200 {
        diff.push_str(&format!("-let old_{i} = {i};\n+let new_{i} = {i};\n"));
    }
    let mut output = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    output.files.remove(0)
}

#[test]
fn first_frame_is_presented_promptly() {
    let mut session = BufferedSession::new(0);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));

    let started = Instant::now();
    run_session(
        &mut session,
        sample_output(),
        None,
        None,
        target,
        PathBuf::from("/nonexistent/notes.json"),
        DEFAULT_EXPAND_LINES,
        DiffuseTheme::dark(),
        false,
        false,
        false,
        None,
        None,
        false,
    )
    .expect("the loop returns after the session disconnects");
    let elapsed = started.elapsed();

    let (presented_at, screen) = session
        .first_frame
        .expect("the loop must present a frame before polling");
    assert!(
        presented_at >= started,
        "the first frame must be presented after the loop starts"
    );
    assert_eq!(
        session.drawn, 1,
        "the loop must present one frame before the disconnect ends it"
    );
    assert!(
        elapsed < FIRST_FRAME_BUDGET,
        "first frame took {elapsed:?}, budget is {FIRST_FRAME_BUDGET:?}"
    );
    assert!(
        screen.contains("lib.rs"),
        "first frame must show the changed file:\n{screen}"
    );
    assert!(
        screen.contains("diffuse"),
        "first frame must show the review title:\n{screen}"
    );
}

/// The empty-state notice must wait for the stream to finish: before a single
/// file has been read, an empty file list says nothing about whether the diff
/// is empty, so claiming so would flash a wrong verdict at startup.
#[test]
fn empty_state_notice_waits_for_the_stream() {
    let mut session = BufferedSession::new(2);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
    let (streaming_tx, streaming_rx) = mpsc::channel::<diffuse_tui::core::wire::OutputFile>();
    drop(streaming_tx);

    run_session(
        &mut session,
        PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        Some(streaming_rx),
        None,
        target,
        PathBuf::from("/nonexistent/notes.json"),
        DEFAULT_EXPAND_LINES,
        DiffuseTheme::dark(),
        false,
        false,
        false,
        None,
        None,
        false,
    )
    .expect("the loop returns after the session disconnects");

    assert!(
        !session.frames[0].contains("No diff to display."),
        "the first frame must not call the undelivered diff empty:\n{}",
        session.frames[0]
    );
    assert!(
        session.frames[0].contains("streaming..."),
        "the first frame must show that the stream is still running:\n{}",
        session.frames[0]
    );
    let last = session.frames.last().expect("the loop must present frames");
    assert!(
        last.contains("No diff to display."),
        "once the stream ends, an empty diff must be reported:\n{last}"
    );
}

/// Files that are already streaming at startup must not delay the first frame:
/// the review is presented first and repainted once the files are ingested.
#[test]
fn first_frame_precedes_streamed_files() {
    let mut session = BufferedSession::new(1);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
    let (streaming_tx, streaming_rx) = mpsc::channel();
    streaming_tx
        .send(large_streamed_file())
        .expect("queue the streamed file before the loop starts");
    drop(streaming_tx);

    run_session(
        &mut session,
        PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        Some(streaming_rx),
        None,
        target,
        PathBuf::from("/nonexistent/notes.json"),
        DEFAULT_EXPAND_LINES,
        DiffuseTheme::dark(),
        false,
        false,
        false,
        None,
        None,
        false,
    )
    .expect("the loop returns after the session disconnects");

    assert!(
        !session.frames[0].contains("big.rs"),
        "the first frame must precede the streamed files:\n{}",
        session.frames[0]
    );
    assert!(
        session.frames.len() >= 2,
        "the loop must repaint after ingesting the streamed file, frames: {}",
        session.frames.len()
    );
    assert!(
        session.frames.iter().any(|frame| frame.contains("big.rs")),
        "the streamed file must appear once ingested"
    );
}

#[test]
fn test_run_session_with_expand_mods_expands_mod_rows() {
    let mut session = BufferedSession::new(1);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
    let (streaming_tx, streaming_rx) = mpsc::channel();
    let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    let a = 1;\n+    let a = 10;\n }\n";
    let file = run_pipeline_full(diff, None, None, PipelineOptions::default())
        .files
        .into_iter()
        .next()
        .unwrap();
    streaming_tx.send(file).unwrap();
    drop(streaming_tx);

    run_session(
        &mut session,
        PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        Some(streaming_rx),
        None,
        target,
        PathBuf::from("/nonexistent/notes.json"),
        DEFAULT_EXPAND_LINES,
        DiffuseTheme::dark(),
        false,
        false,
        false,
        Some(true),
        None,
        false,
    )
    .expect("the loop returns");

    let last_frame = session.frames.last().unwrap();
    eprintln!("LAST FRAME:\n{last_frame}");
    assert!(last_frame.contains("┌-"));
    assert!(last_frame.contains("└+"));
}
