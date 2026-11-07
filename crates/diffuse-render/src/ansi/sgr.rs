//! ANSI SGR constants, escape sequence formatters, and encoder/decoder helpers.

pub const SGR_RESET: &str = "\x1b[0m";
pub const FG_RESET: &str = "\x1b[39m";
pub const BG_RESET: &str = "\x1b[49m";
/// Plain strikethrough on sequence: the bespoke del-run spelling, distinct
/// from the stacked id-9 dim+strike sequence.
pub const STRIKE_ON: &str = "\x1b[9m";
/// Plain strikethrough off sequence (pairs with [`STRIKE_ON`]).
pub const STRIKE_OFF: &str = "\x1b[29m";

/// Return the static ANSI sequence to turn on a known SGR modifier, if
/// available.
#[must_use]
pub fn sgr_on_str(sgr: u32) -> Option<&'static str> {
    match sgr {
        1 => Some("\x1b[1m"),
        2 => Some("\x1b[2m"),
        3 => Some("\x1b[3m"),
        4 => Some("\x1b[4m"),
        9 => Some("\x1b[2;9m"),  // dim + strikethrough
        14 => Some("\x1b[1;4m"), // bold + underline
        _ => None,
    }
}

/// Return the ANSI sequence to turn on an SGR modifier (e.g. 1=bold, 2=dim,
/// 3=italic, 4=underline, 9=dim+strike).
#[must_use]
pub fn sgr_on(sgr: u32) -> String {
    if let Some(s) = sgr_on_str(sgr) {
        s.to_string()
    } else {
        format!("\x1b[{sgr}m")
    }
}

/// Return the ANSI sequence to turn off an SGR modifier.
#[must_use]
pub fn sgr_off(sgr: u32) -> Option<&'static str> {
    match sgr {
        1 | 2 => Some("\x1b[22m"),     // bold / dim off
        3 => Some("\x1b[23m"),         // italic off
        4 => Some("\x1b[24m"),         // underline off
        9 => Some("\x1b[29m\x1b[22m"), // strikethrough off, dim off
        14 => Some("\x1b[24;22m"),     // underline off, bold off
        _ => None,
    }
}

/// Return the numeric SGR code to turn off an SGR modifier.
#[must_use]
pub fn sgr_off_code(sgr: u32) -> Option<u32> {
    match sgr {
        1 | 2 => Some(22), // bold / dim off
        3 => Some(23),     // italic off
        4 => Some(24),     // underline off
        9 => Some(29),     // strikethrough off
        _ => None,
    }
}

/// Strip all ANSI escape sequences from a string.
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert ANSI escape sequences `\x1b[...m` to readable golden markup `{...}`.
#[must_use]
pub fn encode_sgr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last_copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if i > last_copied {
                out.push_str(&s[last_copied..i]);
            }
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' {
                out.push('{');
                out.push_str(&s[i + 2..j]);
                out.push('}');
                i = j + 1;
                last_copied = i;
                continue;
            }
        }
        i += 1;
    }
    if last_copied < s.len() {
        out.push_str(&s[last_copied..]);
    }
    out
}

/// Convert readable golden markup `{...}` back to ANSI escape sequences
/// `\x1b[...m`.
#[must_use]
pub fn decode_sgr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 32);
    let mut last_copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut j = i + 1;
            let mut is_sgr = true;
            while j < bytes.len() && bytes[j] != b'}' {
                if !matches!(bytes[j], b'0'..=b'9' | b';') {
                    is_sgr = false;
                    break;
                }
                j += 1;
            }
            if is_sgr && j < bytes.len() && bytes[j] == b'}' {
                out.push_str(&s[last_copied..i]);
                out.push_str("\x1b[");
                out.push_str(&s[i + 1..j]);
                out.push('m');
                i = j + 1;
                last_copied = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[last_copied..]);
    out
}
