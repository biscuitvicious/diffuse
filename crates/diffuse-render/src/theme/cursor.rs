//! Visual focus-depth axis for themed rendering.
//!
//! The cursor axis formalizes the visual hierarchy of diff review:
//! $\text{Rest (Global)} \supset \text{File} \supset \text{Hunk} \supset
//! \text{Row}$. A palette lookup resolves styling at the innermost active
//! cursor tier and falls back outward along this axis.

/// Depth of the interactive focus when a rendered element resolves its
/// palette: the rest baseline, inside the current file, inside the current
/// hunk, or at the cursor row itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Cursor {
    /// Global baseline: rest content outside any active context.
    #[default]
    Rest = 0,
    /// Inside the active file, outside any active hunk.
    File = 1,
    /// Inside the active hunk, not the cursor row.
    Hunk = 2,
    /// The cursor row, or a focused header / divider banner.
    Row = 3,
}

impl Cursor {
    /// Number of tiers on the axis.
    pub const COUNT: usize = 4;

    /// All tiers ordered innermost-first, the resolution order for
    /// cascading palette lookups (Row $\to$ Hunk $\to$ File $\to$ Rest).
    #[inline]
    #[must_use]
    pub const fn cascade() -> [Cursor; Cursor::COUNT] {
        [Cursor::Row, Cursor::Hunk, Cursor::File, Cursor::Rest]
    }

    /// The next coarser tier on the axis, if any.
    #[inline]
    #[must_use]
    pub const fn outer(self) -> Option<Cursor> {
        match self {
            Cursor::Row => Some(Cursor::Hunk),
            Cursor::Hunk => Some(Cursor::File),
            Cursor::File => Some(Cursor::Rest),
            Cursor::Rest => None,
        }
    }

    /// The tier a banner or divider banner renders at, from its interactive
    /// state: the selected banner lifts to the row tier, the current banner
    /// sits at the file tier, and everything else rests.
    #[inline]
    #[must_use]
    pub const fn from_banner_state(is_selected: bool, is_current: bool) -> Cursor {
        if is_selected {
            Cursor::Row
        } else if is_current {
            Cursor::File
        } else {
            Cursor::Rest
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_discriminants_and_default() {
        assert_eq!(Cursor::default(), Cursor::Rest);
        assert_eq!(Cursor::Rest as u8, 0);
        assert_eq!(Cursor::File as u8, 1);
        assert_eq!(Cursor::Hunk as u8, 2);
        assert_eq!(Cursor::Row as u8, 3);
        assert_eq!(Cursor::COUNT, 4);
    }

    #[test]
    fn test_cursor_outer_axis() {
        assert_eq!(Cursor::Row.outer(), Some(Cursor::Hunk));
        assert_eq!(Cursor::Hunk.outer(), Some(Cursor::File));
        assert_eq!(Cursor::File.outer(), Some(Cursor::Rest));
        assert_eq!(
            Cursor::Rest.outer(),
            None,
            "axis terminates at the rest baseline"
        );
    }

    #[test]
    fn test_cursor_cascade_order() {
        let cascade = Cursor::cascade();
        assert_eq!(
            cascade,
            [Cursor::Row, Cursor::Hunk, Cursor::File, Cursor::Rest]
        );
        // Every tier must be reachable by walking `outer` from the innermost.
        let mut walked = 1;
        let mut current = Cursor::Row;
        while let Some(next) = current.outer() {
            assert_eq!(next, cascade[walked]);
            current = next;
            walked += 1;
        }
        assert_eq!(walked, Cursor::COUNT);
    }

    #[test]
    fn test_cursor_from_banner_state() {
        assert_eq!(Cursor::from_banner_state(true, true), Cursor::Row);
        assert_eq!(Cursor::from_banner_state(true, false), Cursor::Row);
        assert_eq!(Cursor::from_banner_state(false, true), Cursor::File);
        assert_eq!(Cursor::from_banner_state(false, false), Cursor::Rest);
    }
}
