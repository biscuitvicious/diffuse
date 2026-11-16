//! Position badges (`File 3/17`, `Hunk 2/3`): index of a file or hunk within
//! its total, shared by the TUI banners and the CLI pretty printer.

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
    /// File badge text (`File 3/17`, or `File 3` when the total is not known
    /// yet).
    #[must_use]
    pub fn file_text(&self) -> String {
        match self.total {
            Some(total) => format!("File {}/{total}", self.idx + 1),
            None => format!("File {}", self.idx + 1),
        }
    }

    /// Number text (`3/17`, or `3` when the total is not known yet).
    #[must_use]
    pub fn number_text(&self) -> String {
        match self.total {
            Some(total) => format!("{}/{total}", self.idx + 1),
            None => format!("{}", self.idx + 1),
        }
    }

    /// Hunk badge text (`Hunk 2/3`, or `Hunk 2` when the total is not known
    /// yet).
    #[must_use]
    pub fn hunk_text(&self) -> String {
        match self.total {
            Some(total) => format!("Hunk {}/{total}", self.idx + 1),
            None => format!("Hunk {}", self.idx + 1),
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
        assert_eq!(file.file_text(), "File 3/17");
        assert_eq!(file.number_text(), "3/17");
        let hunk = Badge {
            idx: 1,
            total: Some(3),
        };
        assert_eq!(hunk.hunk_text(), "Hunk 2/3");
        assert_eq!(hunk.number_text(), "2/3");
        let single = Badge {
            idx: 0,
            total: Some(1),
        };
        assert_eq!(single.file_text(), "File 1/1");
        assert_eq!(single.number_text(), "1/1");
        assert_eq!(single.hunk_text(), "Hunk 1/1");
    }

    #[test]
    fn test_badge_texts_without_a_known_total() {
        let file = Badge {
            idx: 2,
            total: None,
        };
        assert_eq!(file.file_text(), "File 3");
        assert_eq!(file.number_text(), "3");
        let hunk = Badge {
            idx: 1,
            total: None,
        };
        assert_eq!(hunk.hunk_text(), "Hunk 2");
        assert_eq!(hunk.number_text(), "2");
    }
}
