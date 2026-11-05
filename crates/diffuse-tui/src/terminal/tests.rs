use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

#[test]
fn test_ratatui_test_backend_draw() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let size = f.area();
            assert_eq!(size, Rect::new(0, 0, 40, 10));
            let p = Paragraph::new("diffuse review").block(Block::default().borders(Borders::ALL));
            f.render_widget(p, size);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 1)].symbol(), "d");
}

use std::{io, time::Duration};

use crossterm::event::Event;

use super::{PollOutcome, classify_poll};

#[test]
fn test_classify_poll_passes_events_through() {
    let outcome = classify_poll(
        Duration::from_millis(100),
        Duration::from_micros(50),
        Ok(Some(Event::Resize(80, 24))),
    );
    assert!(
        matches!(outcome, PollOutcome::Event(Event::Resize(80, 24))),
        "an arriving event is reported verbatim: {outcome:?}"
    );
}

#[test]
fn test_classify_poll_treats_full_timeout_as_idle() {
    let outcome = classify_poll(
        Duration::from_millis(100),
        Duration::from_millis(100),
        Ok(None),
    );
    assert!(
        matches!(outcome, PollOutcome::Idle),
        "a poll that waited out its timeout is a healthy idle tick: {outcome:?}"
    );
}

#[test]
fn test_classify_poll_rejects_early_empty_return() {
    // A hung-up descriptor never blocks: poll reports "no event" immediately
    // instead of waiting out the timeout, which would spin the render loop.
    let outcome = classify_poll(
        Duration::from_millis(100),
        Duration::from_micros(200),
        Ok(None),
    );
    assert!(
        matches!(outcome, PollOutcome::Disconnected),
        "an immediate empty return means the event source is gone: {outcome:?}"
    );
}

#[test]
fn test_classify_poll_ignores_zero_timeout_drain() {
    // The drain loop polls with a zero timeout, where an immediate empty
    // return is the expected result rather than a disconnect signal.
    let outcome = classify_poll(Duration::ZERO, Duration::ZERO, Ok(None));
    assert!(
        matches!(outcome, PollOutcome::Idle),
        "a zero-timeout drain is never a disconnect: {outcome:?}"
    );
}

#[test]
fn test_classify_poll_maps_error_to_disconnect() {
    let outcome = classify_poll(
        Duration::from_millis(100),
        Duration::from_millis(1),
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pty gone")),
    );
    assert!(
        matches!(outcome, PollOutcome::Disconnected),
        "an I/O error ends the session instead of being ignored: {outcome:?}"
    );
}

#[test]
fn test_classify_poll_retries_on_interrupt() {
    let outcome = classify_poll(
        Duration::from_millis(100),
        Duration::from_millis(1),
        Err(io::Error::new(io::ErrorKind::Interrupted, "sigwinch")),
    );
    assert!(
        matches!(outcome, PollOutcome::Idle),
        "an interrupted wait is not a disconnect: {outcome:?}"
    );
}
