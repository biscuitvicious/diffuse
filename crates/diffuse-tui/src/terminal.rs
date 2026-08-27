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
use ratatui::{Frame, Terminal, backend::CrosstermBackend};

/// RAII Guard that manages terminal raw mode, alternate screen buffer, and
/// mouse capture using Ratatui with a Crossterm backend.
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
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
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            active: true,
        })
    }

    /// Suspend terminal raw mode and alternate screen (e.g. to run an external
    /// editor).
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            execute!(
                self.terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture,
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
            execute!(
                self.terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture,
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
mod tests {
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
                let p =
                    Paragraph::new("diffuse review").block(Block::default().borders(Borders::ALL));
                f.render_widget(p, size);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 1)].symbol(), "d");
    }
}
