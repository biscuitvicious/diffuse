//! Profile registry — `lang_profile` lookup and caching, `fence_lang_ext`
//! normalization, and `token_re_for` regex construction with its hand-rolled
//! character-class scanner.
//!
//! The scanner (`syntax_token_spans`) is the performance-critical path: the
//! regex crate's NFA on the ~12-alternative token alternation took ~47µs/call;
//! the hand-rolled scan is ~ns/char (~50× faster) and behavior-identical.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use regex::Regex;

use super::{
    language::{c_family, config, go, markdown, python, rust, shell, web},
    profile::LangProfile,
};

// ---------------------------------------------------------------------------
// Regex literal parts — the per-language alternation is composed from these.
// ---------------------------------------------------------------------------

const RE_HASH: &str = r"#[^\n]*";
const RE_LINE: &str = r"//[^\n]*";
const RE_BLOCK: &str = r"/\*[\s\S]*?\*/";
const RE_RAW: &str = r##"r#*"(?:[^"\\]|\\.)*"#*"##;
const RE_DSTR: &str = r##""(?:[^"\\n]|\\.)*""##;
const RE_SSTR: &str = r"'(?:[^'\\n]|\\.)*'";
const RE_BT: &str = r"`(?:[^`\\n]|\\.)*`";
const RE_HEX: &str = r"\b0x[0-9a-fA-F_]+\b";
const RE_NUM: &str = r"\b\d[\d_]*(?:[\d_]+)?(?:[eE][+-]?\d+)?\b";
const RE_ID: &str = r"[A-Za-z_][A-Za-z0-9_]*";
const RE_WS: &str = r"\s+";
const RE_OTHER: &str = ".";

// ---------------------------------------------------------------------------
// Fence-tag normalization
// ---------------------------------------------------------------------------

/// Normalize a fence tag (or file extension) to a profile key.
///
/// Language aliases map to their profile (`rust`→`rs`, `typescript`→`ts`…).
/// A Rust doc-comment fence carries a comma-separated **doctest attribute
/// list** — `rust,no_run`, `should_panic`, `edition2021`, `ignore_linux`, or
/// combos like `rust,no_run,edition2021`. Any `rust` token ⇒ a Rust example;
/// any modifier (`no_run`, `ignore`, `should_panic`, `compile_fail`,
/// `edition*`, `ignore_*`) ⇒ Rust is implied (the default lang in a `.rs`
/// doc fence is rust). A named language alias embedded in a rust crate's
/// doc (` ```python `) takes precedence and highlights as that language.
pub fn fence_lang_ext(tag: &str) -> &str {
    let mut rust_implied = false;
    for part in tag.split(',') {
        let p = part.trim();
        match p {
            "rust" => return "rs",
            "javascript" | "jsx" => return "js",
            "typescript" | "tsx" => return "ts",
            "python" => return "py",
            "shell" | "bash" => return "sh",
            "c++" | "cpp" => return "cpp",
            "markdown" | "md" => return "md",
            "ignore" | "no_run" | "should_panic" | "compile_fail" => rust_implied = true,
            _ if p.starts_with("edition") || p.starts_with("ignore_") => rust_implied = true,
            _ => {}
        }
    }
    if rust_implied { "rs" } else { tag }
}

// ---------------------------------------------------------------------------
// Profile registry
// ---------------------------------------------------------------------------

pub fn lang_profile(ext: &str) -> Option<&'static LangProfile> {
    // Fence ids are LANGUAGE names (` ```rust `); the profiles are keyed by
    // the file EXTENSIONS (`rs`). Map the common aliases — and the rust
    // doctest attribute lists — so a labeled fence actually highlights.
    let ext = fence_lang_ext(ext);
    let profile = match ext {
        "rs" => rust::profile(),
        "ts" | "tsx" | "js" | "jsx" | "mjs" => web::profile(),
        "py" => python::profile(),
        "sh" | "bash" => shell::profile(),
        "c" | "h" | "cpp" | "hpp" | "java" => c_family::profile(),
        "go" => go::profile(),
        "md" | "markdown" => markdown::profile(),
        "toml" | "yaml" | "yml" => config::toml_yaml_profile(),
        "json" => config::json_profile(),
        _ => return None,
    };
    // The profile rebuilds 2 regexes — and lang_profile is called PER ROW
    // inside fence regions. Cache by the canonical ext: build once, leak the
    // Box, hand out a 'static ref so no caller clones or rebuilds.
    static CACHE: OnceLock<Mutex<HashMap<String, &'static LangProfile>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(p) = guard.get(ext) {
        return Some(*p);
    }
    let boxed: &'static LangProfile = Box::leak(Box::new(profile));
    guard.insert(ext.to_string(), boxed);
    Some(boxed)
}

// ---------------------------------------------------------------------------
// Token-regex builder
// ---------------------------------------------------------------------------

/// The token regex for a profile: the comment/string/number/id/ws/other
/// alternation in the TS's fixed order.
pub fn token_re_for(profile: &LangProfile) -> Regex {
    static CACHE: OnceLock<Mutex<HashMap<u8, Regex>>> = OnceLock::new();
    // The key: the profile's feature flags (the regex depends on them —
    // comment kinds). The TS caches per profile; a per-flags code works the
    // same. THE CACHE MUST BE USED — the regex compile is expensive and the
    // classify runs per row (and per merged fragment).
    let key = (u8::from(profile.hash_comment))
        | (u8::from(profile.line_comment) << 1)
        | (u8::from(profile.block_comment) << 2);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache.lock().unwrap();
        if let Some(re) = guard.get(&key) {
            return re.clone();
        }
    }
    let parts: Vec<&str> = [
        if profile.hash_comment {
            Some(RE_HASH)
        } else {
            None
        },
        if profile.line_comment {
            Some(RE_LINE)
        } else {
            None
        },
        if profile.block_comment {
            Some(RE_BLOCK)
        } else {
            None
        },
        Some(RE_RAW),
        Some(RE_DSTR),
        Some(RE_SSTR),
        Some(RE_BT),
        Some(RE_HEX),
        Some(RE_NUM),
        Some(RE_ID),
        Some(RE_WS),
        Some(RE_OTHER),
    ]
    .into_iter()
    .flatten()
    .collect();
    let re = Regex::new(&format!("({})", parts.join("|"))).expect("token regex");
    cache.lock().unwrap().insert(key, re.clone());
    re
}

// ---------------------------------------------------------------------------
// Hand-rolled token-span scanner (performance-critical path)
// ---------------------------------------------------------------------------

/// The token spans of the syntax regex's `find_iter` — a hand-rolled
/// char-class scanner that produces the SAME byte-offset spans as
/// `token_re_for(profile).find_iter(text)`, in the same alternation order
/// (first match wins). The regex crate takes the SLOW NFA path on this
/// alternation (the `\b` word boundaries + non-greedy block comments +
/// ~12 alternatives defeat the DFA) — ~47µs/call, the whole classify cost.
/// This scanner is ~ns/char: a ~50× cut. The color logic in
/// `classify_tokens` runs unchanged on these spans (behavior-identical,
/// golden-gated).
pub(super) fn syntax_token_spans(text: &str, profile: &LangProfile) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let end_off = |i: usize| if i < n { chars[i].0 } else { text.len() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let start_off = chars[i].0;
        let c = chars[i].1;
        // Try the alternation in the TS's fixed order; first match wins.
        // The `#`-comment and `/`-comment both scan to EOL — one merged arm.
        let next = if (profile.hash_comment && c == '#')
            || (profile.line_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '/')
        {
            scan_to_eol(&chars, i)
        } else if profile.block_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '*' {
            scan_block(&chars, i).unwrap_or(i + 1)
        } else if c == 'r' {
            scan_raw(&chars, i).unwrap_or_else(|| scan_ident(&chars, i))
        } else if c == '"' {
            scan_dstr(&chars, i)
        } else if c == '\'' {
            scan_sstr(&chars, i)
        } else if c == '`' {
            scan_bt(&chars, i)
        } else if c == '0' && i + 1 < n && chars[i + 1].1 == 'x' {
            // \b0x[0-9a-fA-F_]+\b. The leading \b is satisfied (a '0'
            // start never follows a word char — idents consume those).
            scan_hex(&chars, i + 2).unwrap_or_else(|| scan_num_or_other(&chars, i))
        } else if c.is_ascii_digit() {
            scan_num_or_other(&chars, i)
        } else if c.is_ascii_alphabetic() || c == '_' {
            scan_ident(&chars, i)
        } else if c.is_whitespace() {
            scan_ws(&chars, i)
        } else {
            i + 1 // RE_OTHER: one char (. does not match \n, but \n is ws)
        };
        out.push((start_off, end_off(next)));
        i = next;
    }
    out
}

// ---------------------------------------------------------------------------
// Character-class helpers (each mirrors one regex alternative)
// ---------------------------------------------------------------------------

/// `#[^\n]*` / `//[^\n]*` — to end of line (the char before the next `\n`).
fn scan_to_eol(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 != '\n' {
        j += 1;
    }
    j
}

/// `/*[\s\S]*?*/` — the FIRST `*/` (non-greedy). None if unterminated
/// (the regex requires the closing `*/`; unterminated → no block match).
fn scan_block(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 2;
    while j < n {
        if chars[j].1 == '*' && j + 1 < n && chars[j + 1].1 == '/' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// `r#*"(?:[^"\\]|\\.)*"#*` — the raw string. Opening `#*` is the run before
/// the `"`; trailing `#*` is an independent run after the closing `"`
/// (the regex does NOT tie the counts). None if malformed.
fn scan_raw(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    if j >= n || chars[j].1 != '"' {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 != '"' {
        // `\\.` skips one escaped char.
        j += if chars[j].1 == '\\' { 2 } else { 1 };
    }
    if j >= n {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    Some(j)
}

/// `"(?:[^"\\n]|\\.)*"` / `'...'` / `` `...` `` — to the closing quote,
/// no raw newline. No close → None (→ caller falls back to `RE_OTHER` for the
/// opening quote char).
fn scan_quoted(chars: &[(usize, char)], i: usize, quote: char) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c == '\\' {
            j += 2;
        } else if c == quote {
            return Some(j + 1);
        } else if c == '\n' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

fn scan_dstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '"').unwrap_or(i + 1)
}
fn scan_sstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '\'').unwrap_or(i + 1)
}
fn scan_bt(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '`').unwrap_or(i + 1)
}

/// `[0-9a-fA-F_]+` then `\b`: trailing word boundary. None if no hex digit
/// or the trailing char is a word char (the `+` needs ≥1 hex digit, and the
/// `\b` needs a non-word after — a trailing word char fails the whole hex).
fn scan_hex(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i;
    while j < n {
        let c = chars[j].1;
        if c.is_ascii_hexdigit() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    if j == i {
        return None; // `+` needs ≥1 hex digit
    }
    // trailing \b: next char non-word (or EOF).
    if j < n && is_word_char(chars[j].1) {
        return None;
    }
    Some(j)
}

/// `\b\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?\b`. Scan the full greedy num
/// (digits, optional decimal, optional exponent), then verify the trailing
/// `\b`. If it fails, the regex backtracks: drop the exponent, then the
/// decimal, down to the bare digit run — the first drop whose end sits
/// before a non-word char wins. If even the bare digit run fails the
/// trailing `\b` (a word char immediately follows, e.g. `5x`), the whole
/// num fails → the single digit is `.` (`RE_OTHER`).
fn scan_num_or_other(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    // bare digit run `\d[\d_]*`
    let mut digits_end = i;
    while digits_end < n {
        let c = chars[digits_end].1;
        if c.is_ascii_digit() || c == '_' {
            digits_end += 1;
        } else {
            break;
        }
    }
    // optional decimal `\.[\d_]+`
    let mut after_dec = digits_end;
    if digits_end < n && chars[digits_end].1 == '.' {
        let mut j = digits_end + 1;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > digits_end + 1 {
            after_dec = j;
        }
    }
    // optional exponent `[eE][+-]?\d+`
    let mut after_exp = after_dec;
    if after_dec < n && (chars[after_dec].1 == 'e' || chars[after_dec].1 == 'E') {
        let mut j = after_dec + 1;
        if j < n && (chars[j].1 == '+' || chars[j].1 == '-') {
            j += 1;
        }
        let start_digits = j;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > start_digits {
            after_exp = j;
        }
    }
    // Backtrack ladder (longest first): full, without exp, without decimal.
    for &end in &[after_exp, after_dec, digits_end] {
        let tail_ok = end >= n || !is_word_char(chars[end].1);
        if tail_ok {
            return end;
        }
    }
    i + 1 // bare digit run followed by a word char → num fails → RE_OTHER `.`
}

/// `[A-Za-z_][A-Za-z0-9_]*`
fn scan_ident(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c.is_ascii_alphanumeric() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    j
}

/// `\s+` (Unicode whitespace, like the regex crate's `\s`).
fn scan_ws(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1.is_whitespace() {
        j += 1;
    }
    j
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
