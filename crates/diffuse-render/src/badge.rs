//! Position badges (`F3/17`, `H2/3`): index of a file or hunk within its
//! total, shared by the TUI banners and the CLI pretty printer.

/// Position of a file or hunk within its parent total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Badge {
    /// Zero-based index of the item.
    pub idx: usize,
    /// Total number of items, or `None` while it is still unknown because the
    /// diff is streaming in: the badge then shows the position alone (`F3`).
    pub total: Option<usize>,
}

impl Badge {
    /// File badge text (`F3/17`, or `F3` when the total is not known yet).
    #[must_use]
    pub fn file_text(&self) -> String {
        match self.total {
            Some(total) => format!("F{}/{total}", self.idx + 1),
            None => format!("F{}", self.idx + 1),
        }
    }

    /// Hunk badge text (`H2/3`, or `H2` when the total is not known yet).
    #[must_use]
    pub fn hunk_text(&self) -> String {
        match self.total {
            Some(total) => format!("H{}/{total}", self.idx + 1),
            None => format!("H{}", self.idx + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_texts() {
        let file = Badge {
            idx: 2,
            total: Some(17),
        };
        assert_eq!(file.file_text(), "F3/17");
        let hunk = Badge {
            idx: 1,
            total: Some(3),
        };
        assert_eq!(hunk.hunk_text(), "H2/3");
        let single = Badge {
            idx: 0,
            total: Some(1),
        };
        assert_eq!(single.file_text(), "F1/1");
        assert_eq!(single.hunk_text(), "H1/1");
    }

    #[test]
    fn test_badge_texts_without_a_known_total() {
        let file = Badge {
            idx: 2,
            total: None,
        };
        assert_eq!(file.file_text(), "F3");
        let hunk = Badge {
            idx: 1,
            total: None,
        };
        assert_eq!(hunk.hunk_text(), "H2");
    }
}
