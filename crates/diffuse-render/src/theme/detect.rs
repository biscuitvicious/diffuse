//! Dynamic terminal mode and background luminance detection subsystem.

use std::{
    env,
    io::{self, IsTerminal, Read, Write},
    str,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMode {
    Dark,
    Light,
}

/// Parse `COLORFGBG` environment variable into a [`TerminalMode`].
///
/// Follows standard xterm conventions: `"fg;bg"` or `"fg;extra;bg"`.
/// ANSI color numbers 0..=6 and 8 indicate dark backgrounds.
/// ANSI color numbers 7 and 11..=15 indicate light backgrounds.
#[must_use]
pub fn parse_colorfgbg(val: &str) -> Option<TerminalMode> {
    let parts: Vec<&str> = val.split(';').collect();
    let bg_str = parts.last()?.trim();
    let bg = bg_str.parse::<u8>().ok()?;
    match bg {
        0..=6 | 8 => Some(TerminalMode::Dark),
        7 | 11..=15 => Some(TerminalMode::Light),
        232..=243 => Some(TerminalMode::Dark),
        244..=255 => Some(TerminalMode::Light),
        _ => None,
    }
}

/// Parse `TERM_BACKGROUND` environment variable into a [`TerminalMode`].
#[must_use]
pub fn parse_term_background(val: &str) -> Option<TerminalMode> {
    match val.trim().to_lowercase().as_str() {
        "light" | "white" => Some(TerminalMode::Light),
        "dark" | "black" => Some(TerminalMode::Dark),
        _ => None,
    }
}

fn parse_hex_component(hex: &str) -> Option<u16> {
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let val = u16::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        1 => Some(val * 0x1111),
        2 => Some(val * 0x0101),
        3 => Some((val << 4) | (val >> 8)),
        4 => Some(val),
        _ => None,
    }
}

/// Parse an OSC 11 terminal response string (e.g.
/// `\x1b]11;rgb:rrrr/gggg/bbbb\x1b\\` or `\x1b]11;rgb:rr/gg/bb\x07`) into
/// 16-bit RGB components.
#[must_use]
pub fn parse_osc11_response(resp: &str) -> Option<(u16, u16, u16)> {
    let rgb_idx = resp.find("rgb:")?;
    let rest = &resp[rgb_idx + 4..];
    let end_idx = rest.find(['\x1b', '\x07']).unwrap_or(rest.len());
    let spec = &rest[..end_idx];
    let parts: Vec<&str> = spec.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parse_hex_component(parts[0])?;
    let g = parse_hex_component(parts[1])?;
    let b = parse_hex_component(parts[2])?;
    Some((r, g, b))
}

/// Calculate perceived relative luminance using ITU-R BT.709 formula
/// (`Y = 0.2126R + 0.7152G + 0.0722B`) over normalized 16-bit RGB values.
#[must_use]
pub fn relative_luminance(r: u16, g: u16, b: u16) -> f64 {
    let r_norm = r as f64 / 65535.0;
    let g_norm = g as f64 / 65535.0;
    let b_norm = b as f64 / 65535.0;
    0.2126 * r_norm + 0.7152 * g_norm + 0.0722 * b_norm
}

/// Classify 16-bit RGB background into [`TerminalMode::Light`] or
/// [`TerminalMode::Dark`].
#[must_use]
pub fn classify_luminance(r: u16, g: u16, b: u16) -> TerminalMode {
    if relative_luminance(r, g, b) > 0.5 {
        TerminalMode::Light
    } else {
        TerminalMode::Dark
    }
}

#[cfg(unix)]
struct RawModeGuard;

#[cfg(unix)]
impl RawModeGuard {
    fn acquire() -> Option<Self> {
        crossterm::terminal::enable_raw_mode().ok().map(|()| Self)
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

#[cfg(unix)]
fn poll_stdin(timeout_ms: i32) -> bool {
    let mut fds = [libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    }];
    let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };
    ret > 0 && (fds[0].revents & libc::POLLIN) != 0
}

#[cfg(unix)]
fn query_terminal_mode_osc11() -> Option<TerminalMode> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }

    let _guard = RawModeGuard::acquire()?;

    let mut stdout = io::stdout().lock();
    stdout.write_all(b"\x1b]11;?\x1b\\").ok()?;
    stdout.flush().ok()?;

    let start = Instant::now();
    let timeout = Duration::from_millis(30);
    let mut buf = Vec::with_capacity(64);
    let mut byte = [0u8; 1];

    while start.elapsed() < timeout {
        let remaining_ms = timeout.saturating_sub(start.elapsed()).as_millis() as i32;
        if remaining_ms <= 0 || !poll_stdin(remaining_ms) {
            break;
        }
        if io::stdin().read_exact(&mut byte).is_ok() {
            buf.push(byte[0]);
            if byte[0] == 0x07
                || (buf.len() >= 2 && buf[buf.len() - 2] == 0x1b && buf[buf.len() - 1] == b'\\')
            {
                break;
            }
        } else {
            break;
        }
    }

    let text = str::from_utf8(&buf).ok()?;
    let (r, g, b) = parse_osc11_response(text)?;
    Some(classify_luminance(r, g, b))
}

#[cfg(not(unix))]
fn query_terminal_mode_osc11() -> Option<TerminalMode> {
    None
}

/// Detect terminal background color mode across prioritized evaluation tiers:
/// 1. `DIFFUSE_THEME` environment override
/// 2. Non-TTY / dumb terminal bypass
/// 3. `TERM_BACKGROUND` and `COLORFGBG` environment indicators
///
/// Never writes to the terminal: safe in one-shot (pager/pipe/redirect)
/// modes. The OSC 11 dynamic query lives in
/// [`detect_terminal_mode_interactive`] only.
#[must_use]
pub fn detect_terminal_mode() -> Option<TerminalMode> {
    if let Ok(val) = env::var("DIFFUSE_THEME") {
        match val.trim().to_lowercase().as_str() {
            "light" | "white" => return Some(TerminalMode::Light),
            "dark" | "black" | "default" => return Some(TerminalMode::Dark),
            "auto" => {}
            _ => {}
        }
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return None;
    }
    if env::var("TERM").as_deref() == Ok("dumb") {
        return None;
    }

    if let Ok(val) = env::var("TERM_BACKGROUND")
        && let Some(mode) = parse_term_background(&val)
    {
        return Some(mode);
    }
    if let Ok(val) = env::var("COLORFGBG")
        && let Some(mode) = parse_colorfgbg(&val)
    {
        return Some(mode);
    }

    None
}

/// Full detection pipeline for interactive sessions: the environment tiers
/// of [`detect_terminal_mode`], then the OSC 11 dynamic query with a
/// non-blocking 30ms timeout.
///
/// Call ONLY when the process owns the terminal for its whole lifetime
/// (raw mode held, all stdin consumed — the interactive TUI). A one-shot
/// process that queries and exits can outrun a slow terminal response;
/// the late reply then arrives as shell input — echoed garbage plus
/// phantom typed text (`11;rgb:...`).
#[must_use]
pub fn detect_terminal_mode_interactive() -> Option<TerminalMode> {
    detect_terminal_mode().or_else(query_terminal_mode_osc11)
}
