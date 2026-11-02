//! Redundant ANSI SGR escape sequence elimination and optimization.

use std::{
    borrow::Cow,
    fmt::{self, Write},
};

/// A tracked SGR cell color: the `None` state of the holding `Option` is
/// the default. Basic SGR colors (`31`, `103`, …) compare equal to their
/// 256-palette twins (`38;5;1`, `48;5;11`, …) — same cell, different
/// spelling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SgrColor {
    Fixed(u8),
    Rgb(u8, u8, u8),
}

/// The tracked subset of SGR cell state relevant to one line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct SgrState {
    fg: Option<SgrColor>,
    bg: Option<SgrColor>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

impl SgrState {
    /// Apply one SGR parameter list to the state. Returns `false` when a
    /// parameter falls outside the model (private modes, colons, exotic
    /// codes) — the caller must emit the sequence and stop trusting its
    /// tracked state until a full reset.
    fn apply_params(&mut self, params: &[u32]) -> bool {
        let mut k = 0;
        // `ext_color(38|48)` consumes the mode + color words that follow.
        let ext_color = |k: &mut usize, colored: &mut Option<SgrColor>| -> bool {
            let mode = params.get(*k).copied();
            *k += 1;
            match mode {
                Some(5) => {
                    let n = params.get(*k).copied().unwrap_or(999);
                    *k += 1;
                    if n > 255 {
                        return false;
                    }
                    *colored = Some(SgrColor::Fixed(n as u8));
                    true
                }
                Some(2) => {
                    let mut rgb = [0u8; 3];
                    for c in rgb.iter_mut() {
                        let n = params.get(*k).copied().unwrap_or(999);
                        *k += 1;
                        if n > 255 {
                            return false;
                        }
                        *c = n as u8;
                    }
                    *colored = Some(SgrColor::Rgb(rgb[0], rgb[1], rgb[2]));
                    true
                }
                _ => false,
            }
        };
        while k < params.len() {
            let p = params[k];
            k += 1;
            match p {
                0 => *self = SgrState::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                9 => self.strike = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                29 => self.strike = false,
                30..=37 => self.fg = Some(SgrColor::Fixed((p - 30) as u8)),
                39 => self.fg = None,
                90..=97 => self.fg = Some(SgrColor::Fixed((p - 90 + 8) as u8)),
                40..=47 => self.bg = Some(SgrColor::Fixed((p - 40) as u8)),
                49 => self.bg = None,
                100..=107 => self.bg = Some(SgrColor::Fixed((p - 100 + 8) as u8)),
                38 => {
                    if !ext_color(&mut k, &mut self.fg) {
                        return false;
                    }
                }
                48 => {
                    if !ext_color(&mut k, &mut self.bg) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

/// Collapse redundant SGR sequences within one terminal line: a sequence
/// whose parameters change nothing about the tracked cell state is dropped.
///
/// Pure deletion machine — output bytes are always a subsequence of input,
/// so rendering can only lose redundancy, never meaning. State starts at
/// the SGR default per call (lines render independently); non-SGR escapes
/// (`\x1b[K`, …) pass through untouched. Sequences outside the model are
/// emitted verbatim and poison tracking until a full reset, so an exotic
/// code can never cause a later code to be wrongly dropped.
///
/// Streams into any `fmt::Write` sink so renderers never materialize an
/// intermediate string: this is the single implementation backing
/// [`optimize_ansi_line`].
pub fn write_optimized_ansi_line<W: Write>(out: &mut W, line: &str) -> fmt::Result {
    // Parse one `ESC [ … m` body (digits/semicolons only — callers exclude
    // anything else) into parameters. Empty body (`ESC[m`) resets.
    fn parse_params(body: &str) -> Option<Vec<u32>> {
        if body.is_empty() {
            return Some(Vec::new());
        }
        body.split(';')
            .map(|w| {
                if w.is_empty() {
                    Ok(0)
                } else {
                    w.parse::<u32>().map_err(|_| ())
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()
    }

    let bytes = line.as_bytes();
    let mut last_copied = 0;
    let mut state = SgrState::default();
    // `false` once an unmodeled sequence escapes tracking.
    let mut known = true;
    let mut i = 0;
    while i < bytes.len() {
        // Scan a CSI body. Colons ride along so colon-form SGR (`38:5:1`,
        // which real terminals apply) is recognised as unmodeled rather
        // than silently passed through untracked; anything else ending in
        // a non-`m` final byte is not SGR and passes through untouched.
        let seq_end = if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit() || bytes[j] == b';' || bytes[j] == b':')
            {
                j += 1;
            }
            (j < bytes.len() && bytes[j] == b'm').then_some(j)
        } else {
            None
        };
        let Some(j) = seq_end else {
            i += 1;
            continue;
        };
        // Bytes [i..=j]: one SGR-shaped sequence with body [i+2..j].
        let body = &line[i + 2..j];
        // Shared emitter for kept sequences.
        macro_rules! keep {
            () => {
                out.write_str(&line[last_copied..=j])?;
                last_copied = j + 1;
            };
        }
        if body.contains(':') {
            // Unmodeled shape: emit verbatim, poison tracking.
            keep!();
            known = false;
            i = j + 1;
            continue;
        }
        let Some(params) = parse_params(body) else {
            // Unparseable body: emit verbatim, poison tracking.
            keep!();
            known = false;
            i = j + 1;
            continue;
        };
        if !known {
            // Conservatively emit; a full reset re-establishes knowledge.
            keep!();
            if params.iter().all(|&p| p == 0) {
                known = true;
                state = SgrState::default();
            }
            i = j + 1;
            continue;
        }
        let mut sim = state;
        if sim.apply_params(&params) {
            if sim == state {
                // No-op: drop the sequence.
                out.write_str(&line[last_copied..i])?;
                last_copied = j + 1;
            } else {
                keep!();
                state = sim;
            }
        } else {
            keep!();
            known = false;
        }
        i = j + 1;
    }
    out.write_str(&line[last_copied..])?;
    Ok(())
}

/// Convenience over [`write_optimized_ansi_line`] for callers that need an
/// owned-or-borrowed string: escape-free input borrows (no allocation),
/// anything else builds exactly once through the writer core.
#[must_use]
pub fn optimize_ansi_line(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    write_optimized_ansi_line(&mut out, line).expect("writing to a String cannot fail");
    Cow::Owned(out)
}

/// Collapse redundant SGR sequences across a multi-line ANSI document,
/// line by line (each line renders independently, so tracking restarts
/// per line — matching what terminal emulators and the TUI's per-line
/// parser assume). Returns borrowed input when it contains no escapes at
/// all; otherwise rebuilds once through [`write_optimized_ansi_line`].
#[must_use]
pub fn optimize_ansi(output: &str) -> Cow<'_, str> {
    if !output.contains('\x1b') {
        return Cow::Borrowed(output);
    }
    let mut out: Option<String> = None;
    let mut pos = 0usize;
    for chunk in output.split_inclusive('\n') {
        let body = chunk.strip_suffix('\n').unwrap_or(chunk);
        match optimize_ansi_line(body) {
            Cow::Borrowed(_) => {
                if let Some(s) = out.as_mut() {
                    s.push_str(chunk);
                }
            }
            Cow::Owned(opt) => {
                let s = out.get_or_insert_with(|| {
                    let mut v = String::with_capacity(output.len());
                    v.push_str(&output[..pos]);
                    v
                });
                s.push_str(&opt);
                if chunk.len() > body.len() {
                    s.push('\n');
                }
            }
        }
        pos += chunk.len();
    }
    match out {
        Some(s) => Cow::Owned(s),
        None => Cow::Borrowed(output),
    }
}
