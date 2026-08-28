//! Terminal Session RAII wrapper, Crossterm events & raw mode initialization.

use std::{
    io::{self, Stdout, Write, stdout},
    time::Duration,
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
        size as terminal_size, supports_keyboard_enhancement,
    },
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};

/// RAII Guard that manages terminal raw mode, alternate screen buffer, and
/// mouse capture using Ratatui with a Crossterm backend.
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
    enhanced_keyboard: bool,
}

impl TerminalGuard {
    /// Enter raw mode, alternate screen buffer, and enable mouse capture.
    pub fn enter() -> io::Result<Self> {
        let mut stdout = stdout();
        enable_raw_mode()?;
        let enhanced_keyboard = supports_keyboard_enhancement().unwrap_or(false);
        if enhanced_keyboard {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
        }
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
            enhanced_keyboard,
        })
    }

    /// Suspend terminal raw mode and alternate screen (e.g. to run an external
    /// editor).
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            if self.enhanced_keyboard {
                let _ = execute!(self.terminal.backend_mut(), PopKeyboardEnhancementFlags);
            }
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
            if self.enhanced_keyboard {
                let _ = execute!(
                    self.terminal.backend_mut(),
                    PushKeyboardEnhancementFlags(
                        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    )
                );
            }
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

    /// Poll for terminal events with a timeout.
    pub fn poll_event(&self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
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
