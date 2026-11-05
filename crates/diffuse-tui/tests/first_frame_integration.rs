//! Startup latency coverage for the interactive loop.
//!
//! The loop renders through a [`Session`]; this test supplies a session backed
//! by an in-memory Ratatui buffer so the first presented frame can be inspected
//! and timed without a terminal.

use std::{
    io,
    path::PathBuf,
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

/// A session that renders into an in-memory buffer, then ends the loop.
struct BufferedSession {
    terminal: Terminal<TestBackend>,
    drawn: usize,
    first_frame: Option<(Instant, String)>,
}

impl BufferedSession {
    fn new() -> Self {
        let backend = TestBackend::new(WIDTH, HEIGHT);
        Self {
            terminal: Terminal::new(backend).expect("in-memory terminal must construct"),
            drawn: 0,
            first_frame: None,
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
        // Ends the loop once the first frame has been presented.
        PollOutcome::Disconnected
    }

    fn size(&self) -> (u16, u16) {
        (WIDTH, HEIGHT)
    }

    fn draw_frame(&mut self, state: &mut AppState) -> io::Result<()> {
        self.terminal
            .draw(|frame| state.render_ratatui_frame(frame))
            .map_err(io::Error::other)?;
        self.drawn += 1;
        if self.first_frame.is_none() {
            self.first_frame = Some((Instant::now(), self.screen_text()));
        }
        Ok(())
    }

    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn first_frame_is_presented_promptly() {
    let mut session = BufferedSession::new();
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
