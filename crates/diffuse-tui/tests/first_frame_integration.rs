//! Startup latency coverage for the interactive loop.
//!
//! The loop renders through a [`Session`]; this test supplies a session backed
//! by an in-memory Ratatui buffer so the first presented frame can be inspected
//! and timed without a terminal.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use diffuse_tui::{
    AppState, PollOutcome, Session,
    core::{
        DEFAULT_EXPAND_LINES, PipelineOptions, Repo, ReviewTarget, pipeline::run_pipeline_full,
        wire::PipelineOutput,
    },
    render::DiffuseTheme,
    run_session,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

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
    terminal: RefCell<Terminal<TestBackend>>,
    polls_left: Cell<usize>,
    events: RefCell<VecDeque<Option<Event>>>,
    drawn: usize,
    first_frame: Option<(Instant, String)>,
    frames: Vec<String>,
    width: Cell<u16>,
    height: Cell<u16>,
}

impl BufferedSession {
    fn new(polls_left: usize) -> Self {
        Self::with_dimensions(WIDTH, HEIGHT, polls_left)
    }

    fn with_dimensions(width: u16, height: u16, polls_left: usize) -> Self {
        let backend = TestBackend::new(width, height);
        Self {
            terminal: RefCell::new(
                Terminal::new(backend).expect("in-memory terminal must construct"),
            ),
            polls_left: Cell::new(polls_left),
            events: RefCell::new(VecDeque::new()),
            drawn: 0,
            first_frame: None,
            frames: Vec::new(),
            width: Cell::new(width),
            height: Cell::new(height),
        }
    }

    fn push_event(&self, event: Event) {
        self.events.borrow_mut().push_back(Some(event));
    }

    fn push_idle(&self) {
        self.events.borrow_mut().push_back(None);
    }

    /// Flatten the rendered buffer into text with one line per terminal row.
    fn screen_text(&self) -> String {
        let term = self.terminal.borrow();
        let buffer = term.backend().buffer();
        let mut text = String::new();
        let (w, h) = (self.width.get(), self.height.get());
        for y in 0..h {
            for x in 0..w {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }
}

impl Session for BufferedSession {
    fn poll_outcome(&self, _timeout: Duration) -> PollOutcome {
        if let Some(maybe_event) = self.events.borrow_mut().pop_front() {
            if let Some(event) = maybe_event {
                if let Event::Resize(w, h) = event {
                    self.width.set(w);
                    self.height.set(h);
                    let mut term = self.terminal.borrow_mut();
                    term.backend_mut().resize(w, h);
                    let _ = term.resize(Rect::new(0, 0, w, h));
                }
                return PollOutcome::Event(event);
            }
            return PollOutcome::Idle;
        }
        let remaining = self.polls_left.get();
        if remaining == 0 {
            return PollOutcome::Disconnected;
        }
        self.polls_left.set(remaining - 1);
        PollOutcome::Idle
    }

    fn size(&self) -> (u16, u16) {
        (self.width.get(), self.height.get())
    }

    fn draw_frame(&mut self, state: &mut AppState) -> io::Result<()> {
        self.terminal
            .borrow_mut()
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
        None,
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
        None,
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
        None,
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
        None,
    )
    .expect("the loop returns");

    let last_frame = session.frames.last().unwrap();
    eprintln!("LAST FRAME:\n{last_frame}");
    assert!(last_frame.contains("┌-"));
    assert!(last_frame.contains("└+"));
}

#[test]
fn test_run_session_auto_defaults_side_by_side_on_wide_terminal() {
    let mut session = BufferedSession::with_dimensions(180, 24, 0);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    let screen = &session.first_frame.unwrap().1;
    assert!(
        screen.contains('│'),
        "180-col terminal must auto-default to side-by-side with divider '│':\n{screen}"
    );
}

#[test]
fn test_run_session_defaults_to_unified_on_narrow_terminal() {
    let mut session = BufferedSession::with_dimensions(120, 24, 0);
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    let screen = &session.first_frame.unwrap().1;
    assert!(
        !screen.contains('│'),
        "120-col terminal must default to unified diff (no divider):\n{screen}"
    );
}

#[test]
fn test_run_session_auto_adapts_to_resize() {
    let session = BufferedSession::with_dimensions(180, 24, 2);
    session.push_event(Event::Resize(120, 24));
    session.push_idle();
    session.push_event(Event::Resize(190, 24));
    session.push_idle();

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 3);
    assert!(
        session.frames[0].contains('│'),
        "Frame 1 at 180 cols must be split mode:\n{}",
        session.frames[0]
    );
    assert!(
        !session.frames[1].contains('│'),
        "Frame 2 at 120 cols must adapt to unified mode:\n{}",
        session.frames[1]
    );
    assert!(
        session.frames[2].contains('│'),
        "Frame 3 at 190 cols must adapt to split mode:\n{}",
        session.frames[2]
    );
}

#[test]
fn test_run_session_explicit_cli_side_by_side_does_not_adapt() {
    let session = BufferedSession::with_dimensions(180, 24, 1);
    session.push_event(Event::Resize(120, 24));

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        Some(true),
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 2);
    assert!(session.frames[0].contains('│'));
    assert!(
        session.frames[1].contains('│'),
        "Frame 2 at 120 cols must remain split when explicit:\n{}",
        session.frames[1]
    );
}

#[test]
fn test_run_session_explicit_cli_unified_does_not_adapt() {
    let session = BufferedSession::with_dimensions(120, 24, 1);
    session.push_event(Event::Resize(190, 24));

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        Some(false),
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 2);
    assert!(!session.frames[0].contains('│'));
    assert!(
        !session.frames[1].contains('│'),
        "Frame 2 at 190 cols must remain unified when explicit:\n{}",
        session.frames[1]
    );
}

#[test]
fn test_run_session_explicit_shortcut_locks_mode_across_resize() {
    let session = BufferedSession::with_dimensions(180, 24, 2);
    session.push_event(Event::Key(KeyEvent::new(
        KeyCode::Char('\\'),
        KeyModifiers::NONE,
    )));
    session.push_idle();
    session.push_event(Event::Resize(200, 24));
    session.push_idle();

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 3);
    assert!(session.frames[0].contains('│'));
    assert!(!session.frames[1].contains('│'));
    assert!(
        !session.frames[2].contains('│'),
        "Frame 3 at 200 cols must remain unified after user toggled:\n{}",
        session.frames[2]
    );
}

#[test]
fn test_run_session_f3_shortcut_toggles_side_by_side() {
    let session = BufferedSession::with_dimensions(120, 24, 1);
    // 120 cols defaults to unified mode. Pressing F3 toggles to split mode.
    session.push_event(Event::Key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE)));

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 2);
    assert!(!session.frames[0].contains('│'));
    assert!(
        session.frames[1].contains('│'),
        "Frame 2 after F3 must be split mode:\n{}",
        session.frames[1]
    );
}

#[test]
fn test_run_session_b_shortcut_toggles_side_by_side() {
    let session = BufferedSession::with_dimensions(120, 24, 1);
    // 120 cols defaults to unified mode. Pressing 'b' toggles to split mode.
    session.push_event(Event::Key(KeyEvent::new(
        KeyCode::Char('b'),
        KeyModifiers::NONE,
    )));

    let mut session = session;
    let target = ReviewTarget::from_repo(Repo::from_key("diffuse"));
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
        None,
    )
    .expect("the loop returns");

    assert!(session.frames.len() >= 2);
    assert!(!session.frames[0].contains('│'));
    assert!(
        session.frames[1].contains('│'),
        "Frame 2 after 'b' must be split mode:\n{}",
        session.frames[1]
    );
}
