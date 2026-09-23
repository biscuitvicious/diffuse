//! Position badges (`F3/17`, `H2/3`): index of a file or hunk within its
//! total, shared by the TUI banners and the CLI pretty printer.

/// Position of a file or hunk within its parent total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Badge {
    /// Zero-based index of the item.
    pub idx: usize,
    /// Total number of items.
    pub total: usize,
}

impl Badge {
    /// File badge text (`F3/17`).
    #[must_use]
    pub fn file_text(&self) -> String {
        format!("F{}/{}", self.idx + 1, self.total)
    }

    /// Hunk badge text (`H2/3`).
    #[must_use]
    pub fn hunk_text(&self) -> String {
        format!("H{}/{}", self.idx + 1, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_texts() {
        let file = Badge { idx: 2, total: 17 };
        assert_eq!(file.file_text(), "F3/17");
        let hunk = Badge { idx: 1, total: 3 };
        assert_eq!(hunk.hunk_text(), "H2/3");
        let single = Badge { idx: 0, total: 1 };
        assert_eq!(single.file_text(), "F1/1");
        assert_eq!(single.hunk_text(), "H1/1");
    }
}
