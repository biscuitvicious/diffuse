//! Salient token matching helpers shared by path segmentation and cross-slot
//! relocation.

/// Reports whether two salient word tokens match either verbatim or as a
/// `snake_case` segment of length at least 4.
#[inline]
pub(crate) fn is_salient_word_match(w1: &str, w2: &str) -> bool {
    w1 == w2
        || (w2.len() >= 4 && w1.split('_').any(|s| s == w2))
        || (w1.len() >= 4 && w2.split('_').any(|s| s == w1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_and_snake_case_segments_at_min_len() {
        assert!(is_salient_word_match("alpha", "alpha"));
        assert!(is_salient_word_match("scroll_offset", "offset"));
        assert!(is_salient_word_match("offset", "scroll_offset"));
        assert!(!is_salient_word_match("foo_bar", "bar"));
        assert!(!is_salient_word_match("bar", "foo_bar"));
        assert!(!is_salient_word_match("alpha", "beta"));
    }
}
