//! Terminal Session RAII wrapper, Crossterm events & raw mode initialization.

use std::{
    io::{self, Stdout, Write, stdout},
    panic,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        size as terminal_size,
    },
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};

/// RAII Guard that manages terminal raw mode, alternate screen buffer, and
/// mouse capture using Ratatui with a Crossterm backend.
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install a process-wide panic hook that restores the terminal (leaving
/// alternate screen, disabling raw mode, and showing cursor) before printing
/// the panic information and aborting the process.
pub fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // Reset terminal immediately before printing panic info
        let mut stdout = stdout();
        let _ = execute!(
            stdout,
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            cursor::Show
        );
        let _ = disable_raw_mode();
        let _ = stdout.flush();

        eprintln!("\r\n========================================================");
        eprintln!("diffuse encountered an unexpected fatal error / panic:");
        eprintln!("========================================================");
        default_hook(panic_info);
        eprintln!("========================================================\r\n");

        // Force immediate exit of the entire process so background worker/rayon
        // panics abort the process cleanly instead of leaving the TUI running.
        std::process::exit(101);
    }));
}

/// The shortest poll timeout whose early return is diagnostic: below it, an
/// empty return is normal and carries no signal.
const EARLY_RETURN_GRACE: Duration = Duration::from_millis(10);

/// Result of polling the terminal for input.
#[derive(Debug)]
pub enum PollOutcome {
    /// A terminal event is ready.
    Event(Event),
    /// The poll waited out its timeout with no event; the terminal is healthy.
    Idle,
    /// The terminal can no longer deliver input, so the session must end.
    Disconnected,
}

/// Classify a terminal poll.
///
/// A healthy event source blocks for the full timeout when no input arrives,
/// so an `Ok(None)` that returns almost immediately means the source behind
/// stdin is gone: the controlling terminal was closed, or a multiplexer pane
/// hung up. Reporting that as [`PollOutcome::Idle`] would spin the render loop
/// at full CPU forever, because a hung-up descriptor never blocks and never
/// surfaces an error.
#[must_use]
pub fn classify_poll(
    requested: Duration,
    elapsed: Duration,
    result: io::Result<Option<Event>>,
) -> PollOutcome {
    match result {
        Ok(Some(event)) => PollOutcome::Event(event),
        Ok(None) => {
            if requested >= EARLY_RETURN_GRACE && elapsed * 2 < requested {
                PollOutcome::Disconnected
            } else {
                PollOutcome::Idle
            }
        }
        // A signal interrupted the wait; the terminal is still usable.
        Err(err) if err.kind() == io::ErrorKind::Interrupted => PollOutcome::Idle,
        Err(_) => PollOutcome::Disconnected,
    }
}

impl TerminalGuard {
    /// Enter raw mode, alternate screen buffer, and enable mouse capture.
    pub fn enter() -> io::Result<Self> {
        install_panic_hook();
        let mut stdout = stdout();
        enable_raw_mode()?;
        // Announce the flags without asking first: a terminal that does not
        // implement the kitty keyboard protocol ignores the sequence, while
        // querying support blocks startup for seconds when a terminal does
        // not answer the query.
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            cursor::Hide
        )?;
        stdout.flush()?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(Self {
            terminal,
            active: true,
        })
    }

    /// Suspend terminal raw mode and alternate screen (e.g. to run an external
    /// editor).
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            let _ = execute!(self.terminal.backend_mut(), PopKeyboardEnhancementFlags);
            execute!(
                self.terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture,
                DisableBracketedPaste,
                cursor::Show
            )?;
            disable_raw_mode()?;
            self.terminal.backend_mut().flush()?;
            self.active = false;
        }
        Ok(())
    }

    /// Resume terminal raw mode and alternate screen after suspension.
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            enable_raw_mode()?;
            let _ = execute!(
                self.terminal.backend_mut(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
            execute!(
                self.terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture,
                EnableBracketedPaste,
                cursor::Hide
            )?;
            self.terminal.backend_mut().flush()?;
            self.terminal.clear()?;
            self.active = true;
        }
        Ok(())
    }

    /// Query the current terminal dimensions (columns, rows).
    pub fn size(&self) -> (u16, u16) {
        self.terminal
            .size()
            .map(|s| (s.width, s.height))
            .unwrap_or_else(|_| terminal_size().unwrap_or((80, 24)))
    }

    /// Poll for a terminal event, measuring how long the wait took so that a
    /// dead event source is reported as [`PollOutcome::Disconnected`].
    pub fn poll_outcome(&self, timeout: Duration) -> PollOutcome {
        let started = Instant::now();
        let result = match event::poll(timeout) {
            Ok(true) => event::read().map(Some),
            Ok(false) => Ok(None),
            Err(err) => Err(err),
        };
        classify_poll(timeout, started.elapsed(), result)
    }

    /// Draw a frame using Ratatui.
    pub fn draw<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
    }

    /// Write raw ANSI bytes directly to the terminal stdout.
    pub fn write_frame(&mut self, frame: &str) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), cursor::MoveTo(0, 0))?;
        self.terminal.backend_mut().write_all(frame.as_bytes())?;
        self.terminal.backend_mut().flush()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}

#[cfg(test)]
mod tests;
