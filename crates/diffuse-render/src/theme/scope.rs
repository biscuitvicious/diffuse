//! Visual containment scope axis for themed rendering.
//!
//! Scopes formalize the visual hierarchy of diff review:
//! $\text{None (Global)} \supset \text{File} \supset \text{Hunk} \supset
//! \text{Row}$. A palette lookup resolves styling at the innermost active scope
//! and falls back outward along this axis.

/// Containment scope of a rendered element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Scope {
    /// Global baseline (outside the active file, or rest content).
    #[default]
    None = 0,
    /// Inside the active file, outside any active hunk.
    File = 1,
    /// Inside the active hunk, not the cursor row.
    Hunk = 2,
    /// The cursor row, or a focused header / divider banner.
    Row = 3,
}

impl Scope {
    /// Number of scopes on the axis.
    pub const COUNT: usize = 4;

    /// All scopes ordered innermost-first, the resolution order for
    /// cascading palette lookups (Row $\to$ Hunk $\to$ File $\to$ None).
    #[inline]
    #[must_use]
    pub const fn cascade() -> [Scope; Scope::COUNT] {
        [Scope::Row, Scope::Hunk, Scope::File, Scope::None]
    }

    /// The next coarser scope on the axis, if any.
    #[inline]
    #[must_use]
    pub const fn outer(self) -> Option<Scope> {
        match self {
            Scope::Row => Some(Scope::Hunk),
            Scope::Hunk => Some(Scope::File),
            Scope::File => Some(Scope::None),
            Scope::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_discriminants_and_default() {
        assert_eq!(Scope::default(), Scope::None);
        assert_eq!(Scope::None as u8, 0);
        assert_eq!(Scope::File as u8, 1);
        assert_eq!(Scope::Hunk as u8, 2);
        assert_eq!(Scope::Row as u8, 3);
        assert_eq!(Scope::COUNT, 4);
    }

    #[test]
    fn test_scope_outer_axis() {
        assert_eq!(Scope::Row.outer(), Some(Scope::Hunk));
        assert_eq!(Scope::Hunk.outer(), Some(Scope::File));
        assert_eq!(Scope::File.outer(), Some(Scope::None));
        assert_eq!(
            Scope::None.outer(),
            None,
            "axis terminates at the global scope"
        );
    }

    #[test]
    fn test_scope_cascade_order() {
        let cascade = Scope::cascade();
        assert_eq!(cascade, [Scope::Row, Scope::Hunk, Scope::File, Scope::None]);
        // Every scope must be reachable by walking `outer` from the innermost.
        let mut walked = 1;
        let mut current = Scope::Row;
        while let Some(next) = current.outer() {
            assert_eq!(next, cascade[walked]);
            current = next;
            walked += 1;
        }
        assert_eq!(walked, Scope::COUNT);
    }
}
