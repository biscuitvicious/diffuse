//! Unicode word and identifier boundary utilities.

/// Whether `c` is a word/identifier character (`c.is_alphanumeric() || c ==
/// '_'`).
///
/// Supports full Unicode script sets (Latin, Cyrillic, Greek, Han, Arabic,
/// etc.).
#[inline]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Leading Unicode word run of `s` (may be empty).
pub fn word_prefix(s: &str) -> &str {
    let end = s.find(|c: char| !is_word_char(c)).unwrap_or(s.len());
    &s[..end]
}

/// Byte length of the leading Unicode word run of `s`.
pub fn word_prefix_len(s: &str) -> usize {
    word_prefix(s).len()
}

/// First Unicode word after trimming leading whitespace, if any.
pub fn first_word(s: &str) -> Option<&str> {
    let w = word_prefix(s.trim_start());
    if w.is_empty() { None } else { Some(w) }
}
