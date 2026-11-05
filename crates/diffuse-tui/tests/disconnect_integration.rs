//! Integration coverage for terminal disconnection.
//!
//! A terminal whose peer has hung up delivers no input and does not honor the
//! poll timeout: every poll returns no event almost immediately. The run loop
//! must read that as a disconnect and return instead of spinning on a dead
//! descriptor.

use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use diffuse_tui::{
    AppState, PollOutcome, Session,
    core::{DEFAULT_EXPAND_LINES, Repo, ReviewTarget, wire::PipelineOutput},
    render::DiffuseTheme,
    run_session,
    terminal::classify_poll,
};

/// Polls after which the fake terminal stops mimicking a hang-up and reports a
/// disconnect. It only caps a regressed loop so the test fails instead of
/// hanging.
const POLL_LIMIT: usize = 10_000;

/// Polls a healthy loop needs at most to observe one disconnect: the initial
/// iteration plus a small margin for the empty-event drain.
const MAX_EXPECTED_POLLS: usize = 8;

/// A session backed by a terminal whose peer is gone.
struct HungUpSession {
    polls: Arc<AtomicUsize>,
}

impl Session for HungUpSession {
    fn poll_outcome(&self, timeout: Duration) -> PollOutcome {
        if self.polls.fetch_add(1, Ordering::SeqCst) >= POLL_LIMIT {
            return PollOutcome::Disconnected;
        }
        // A hung-up pty returns from `poll` immediately without an event.
        classify_poll(timeout, Duration::from_micros(1), Ok(None))
    }

    fn size(&self) -> (u16, u16) {
        (80, 24)
    }

    fn draw_frame(&mut self, _state: &mut AppState) -> io::Result<()> {
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
fn run_loop_returns_when_the_terminal_hangs_up() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut session = HungUpSession {
        polls: Arc::clone(&polls),
    };
    let output = PipelineOutput {
        files: Vec::new(),
        branch: "test".into(),
        untracked_skipped: Vec::new(),
    };
    let target = ReviewTarget::from_repo(Repo::from_key("test"));

    let started = Instant::now();
    run_session(
        &mut session,
        output,
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
    .expect("run_session returns after the terminal hangs up");
    let elapsed = started.elapsed();

    let observed = polls.load(Ordering::SeqCst);
    assert!(
        observed < MAX_EXPECTED_POLLS,
        "loop polled {observed} times before honoring the disconnect"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "loop took {elapsed:?} to exit"
    );
}
