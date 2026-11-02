//! Atomic operation chunks for the alignment tape.

/// An atomic operation along the alignment tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeChunk<'a> {
    /// Identical token run surviving unmodified across both sides.
    Keep(&'a str),
    /// Struck deletion from old text, anchored directly at its common context.
    Strike(&'a str),
    /// Filled addition to new text, anchored directly at its common context.
    Fill(&'a str),
    /// Relocated / moved segment (subline reflow or cross-line transfer).
    Relocate {
        from_span: (usize, usize),
        to_span: (usize, usize),
        text: &'a str,
    },
}

impl<'a> TapeChunk<'a> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => {
                s.is_empty()
            }
        }
    }

    #[inline]
    pub fn text(&self) -> &'a str {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => s,
        }
    }

    /// Number of bytes consumed by this chunk in old text.
    #[inline]
    pub fn old_len(&self) -> usize {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Relocate { text: s, .. } => s.len(),
            Self::Fill(_) => 0,
        }
    }

    /// Number of bytes consumed by this chunk in new text.
    #[inline]
    pub fn new_len(&self) -> usize {
        match self {
            Self::Keep(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => s.len(),
            Self::Strike(_) => 0,
        }
    }
}
