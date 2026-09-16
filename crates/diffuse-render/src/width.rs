//! Terminal display-width measurement.
//!
//! Single choke point for "how many terminal columns wide is this text".
//! All layout math in the renderer and TUI must go through these helpers
//! rather than touching width crates directly, so there is exactly one
//! notion of width in the codebase:
//!
//! - Boundaries always come from [`unicode_segmentation`] extended grapheme
//!   clusters (never split a cluster).
//! - Column counts come from UAX #11 display widths: CJK wide/fullwidth and
//!   emoji-presentation graphemes occupy two columns, combining and format
//!   characters occupy zero, everything else occupies one, and control
//!   characters count as zero.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of one extended grapheme cluster in terminal columns.
///
/// Measured in isolation: cross-grapheme UAX #11 sequencing rules (e.g. the
/// Arabic Lam-Alef ligature) do not apply. Use this when accumulating
/// widths cluster by cluster, as slicing loops do.
pub fn grapheme_width(glyph: impl AsRef<str>) -> usize {
    glyph.as_ref().width()
}

/// Display width of a text run in terminal columns.
///
/// Measured over the whole string so multi-character presentation
/// sequences (emoji modifiers, ZWJ sequences, regional-indicator pairs)
/// resolve exactly as terminals render them. Note this can differ from the
/// sum of [`grapheme_width`] over the string where UAX #11 assigns a
/// multi-grapheme sequence a single column.
pub fn display_width(text: impl AsRef<str>) -> usize {
    text.as_ref().width()
}

/// Display width of a single character in terminal columns; control
/// characters count as zero.
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_width_basics() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('\x1b'), 0);
        assert_eq!(char_width('\n'), 0);
        assert_eq!(grapheme_width("✅"), 2);
        assert_eq!(grapheme_width("💬"), 2);
        assert_eq!(grapheme_width("e\u{301}"), 1);
        assert_eq!(display_width("a✅b"), 4);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn test_display_differs_from_grapheme_sum_on_ligatures() {
        use unicode_segmentation::UnicodeSegmentation;
        // Arabic Lam + Alef: two graphemes, but UAX #11 assigns the ligature
        // a single column when measured as a whole string.
        let ligature = "\u{644}\u{627}";
        assert_eq!(display_width(ligature), 1);
        let per_grapheme: usize = ligature.graphemes(true).map(grapheme_width).sum();
        assert_eq!(per_grapheme, 2);
    }
}
