//! Terminal Session RAII wrapper, Crossterm events & raw mode initialization.

use std::{
    io::{self, Stdout, Write, stdout},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        size as terminal_size,
    },
};

/// RAII Guard that manages terminal raw mode, alternate screen buffer, and
/// mouse capture.
pub struct TerminalGuard {
    stdout: Stdout,
    active: bool,
}

impl TerminalGuard {
    /// Enter raw mode, alternate screen buffer, and enable mouse capture.
    pub fn enter() -> io::Result<Self> {
        let mut stdout = stdout();
        enable_raw_mode()?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;
        stdout.flush()?;
        Ok(Self {
            stdout,
            active: true,
        })
    }

    /// Suspend terminal raw mode and alternate screen (e.g. to run an external
    /// editor).
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            execute!(
                self.stdout,
                LeaveAlternateScreen,
                DisableMouseCapture,
                cursor::Show
            )?;
            disable_raw_mode()?;
            self.stdout.flush()?;
            self.active = false;
        }
        Ok(())
    }

    /// Resume terminal raw mode and alternate screen after suspension.
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            enable_raw_mode()?;
            execute!(
                self.stdout,
                EnterAlternateScreen,
                EnableMouseCapture,
                cursor::Hide
            )?;
            self.stdout.flush()?;
            self.active = true;
        }
        Ok(())
    }

    /// Query the current terminal dimensions (columns, rows).
    pub fn size(&self) -> (u16, u16) {
        terminal_size().unwrap_or((80, 24))
    }

    /// Poll for terminal events with a timeout.
    pub fn poll_event(&self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }

    /// Write raw ANSI bytes directly to the terminal stdout.
    pub fn write_frame(&mut self, frame: &str) -> io::Result<()> {
        execute!(self.stdout, cursor::MoveTo(0, 0))?;
        self.stdout.write_all(frame.as_bytes())?;
        self.stdout.flush()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}
