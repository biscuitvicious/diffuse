//! Syntactic frame predicates for delimiter balance.

use diffuse_algo::{DelimiterCategory, UniversalDelimiterClassifier};

/// Computes the net delimiter balance delta (`+1` per opener, `-1` per closer)
/// of a single line, ignoring string literals, character literals, and ANSI
/// escape sequences.
pub(crate) fn robust_delim_delta(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut delta = 0i32;
    let mut in_str = false;
    let mut str_char = b'"';
    let mut escaped = false;
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
            i += 2;
            while i < len && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            continue;
        }
        if in_str {
            if b == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'`' {
            in_str = true;
            str_char = b;
            i += 1;
            continue;
        }
        if b == b'\'' {
            let is_char_lit = (i + 2 < len && bytes[i + 2] == b'\'')
                || (i + 3 < len && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'');
            if is_char_lit {
                in_str = true;
                str_char = b'\'';
            }
            i += 1;
            continue;
        }
        match UniversalDelimiterClassifier.classify_char(b as char) {
            Some(DelimiterCategory::Open(_)) => delta += 1,
            Some(DelimiterCategory::Close(_)) => delta -= 1,
            _ => {}
        }
        i += 1;
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_delim_delta_ignores_strings_char_lits_escapes_and_ansi() {
        assert_eq!(robust_delim_delta("fn foo(a: &str) {"), 1);
        assert_eq!(robust_delim_delta("    });"), -2);
        assert_eq!(robust_delim_delta("let s = \"({[\";"), 0);
        assert_eq!(robust_delim_delta("let s = `({[`;"), 0);
        assert_eq!(robust_delim_delta("let c = '('; let e = '\\'';"), 0);
        assert_eq!(robust_delim_delta("fn f<'a>(x: &'a str) {"), 1);
        assert_eq!(robust_delim_delta("\x1b[31mfn f(\x1b[0m"), 1);
        assert_eq!(robust_delim_delta("let s = \"\\\")\";"), 0);
        assert_eq!(robust_delim_delta("\x1b[31"), 0);
    }
}
