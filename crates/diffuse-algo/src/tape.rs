//! Algebraic monoidal edit tape with provable involution and projection
//! functors.
//!
//! Models alignment as an invertible sequence of atomic edits (`Keep`,
//! `Strike`, `Fill`, `Relocate`) satisfying monoid laws:
//! - Associativity: `(A ∘ B) ∘ C == A ∘ (B ∘ C)`
//! - Identity: `A ∘ ∅ == A == ∅ ∘ A`
//! - Involution: `invert(invert(A)) == A`

/// An atomic edit chunk along a tape.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TapeChunk<T> {
    /// Item surviving unmodified across both sides.
    Keep(T),
    /// Struck deletion from source sequence.
    Strike(T),
    /// Filled addition to target sequence.
    Fill(T),
    /// Relocated segment transferred between coordinates.
    Relocate {
        from_span: (usize, usize),
        to_span: (usize, usize),
        item: T,
    },
}

impl<T> TapeChunk<T> {
    /// Borrow the underlying item.
    #[inline]
    pub fn item(&self) -> &T {
        match self {
            Self::Keep(item)
            | Self::Strike(item)
            | Self::Fill(item)
            | Self::Relocate { item, .. } => item,
        }
    }

    /// Consumes the chunk and returns the underlying item.
    #[inline]
    pub fn into_item(self) -> T {
        match self {
            Self::Keep(item)
            | Self::Strike(item)
            | Self::Fill(item)
            | Self::Relocate { item, .. } => item,
        }
    }

    /// Returns true if this is a `Keep` chunk.
    #[inline]
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep(_))
    }

    /// Returns true if this is a `Strike` chunk.
    #[inline]
    pub fn is_strike(&self) -> bool {
        matches!(self, Self::Strike(_))
    }

    /// Returns true if this is a `Fill` chunk.
    #[inline]
    pub fn is_fill(&self) -> bool {
        matches!(self, Self::Fill(_))
    }

    /// Returns true if this is a `Relocate` chunk.
    #[inline]
    pub fn is_relocate(&self) -> bool {
        matches!(self, Self::Relocate { .. })
    }

    /// Inverts the edit chunk.
    #[inline]
    pub fn invert(self) -> Self {
        match self {
            Self::Keep(item) => Self::Keep(item),
            Self::Strike(item) => Self::Fill(item),
            Self::Fill(item) => Self::Strike(item),
            Self::Relocate {
                from_span,
                to_span,
                item,
            } => Self::Relocate {
                from_span: to_span,
                to_span: from_span,
                item,
            },
        }
    }

    /// Maps the inner item to another type `U`.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> TapeChunk<U> {
        match self {
            Self::Keep(item) => TapeChunk::Keep(f(item)),
            Self::Strike(item) => TapeChunk::Strike(f(item)),
            Self::Fill(item) => TapeChunk::Fill(f(item)),
            Self::Relocate {
                from_span,
                to_span,
                item,
            } => TapeChunk::Relocate {
                from_span,
                to_span,
                item: f(item),
            },
        }
    }
}

/// An algebraic edit tape representing a sequence of edit chunks.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Tape<T> {
    chunks: Vec<TapeChunk<T>>,
}

/// Projection function mapping a tape chunk to an item reference.
pub type ChunkProjector<T> = fn(&TapeChunk<T>) -> Option<&T>;

impl<T> Tape<T> {
    /// Creates an empty edit tape (the monoidal identity element).
    #[inline]
    pub fn empty() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Creates an empty edit tape with preallocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            chunks: Vec::with_capacity(capacity),
        }
    }

    /// Appends a chunk to the tape.
    #[inline]
    pub fn push(&mut self, chunk: TapeChunk<T>) {
        self.chunks.push(chunk);
    }

    /// Monoidally composes another tape onto the end of this tape: `self ∘
    /// other`.
    #[inline]
    pub fn compose(&mut self, other: Tape<T>) {
        self.chunks.extend(other.chunks);
    }

    /// Borrow the underlying chunks.
    #[inline]
    pub fn chunks(&self) -> &[TapeChunk<T>] {
        &self.chunks
    }

    /// Returns the number of chunks in the tape.
    #[inline]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Returns true if the tape contains no chunks.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Consumes the tape and returns the underlying chunk vector.
    #[inline]
    pub fn into_chunks(self) -> Vec<TapeChunk<T>> {
        self.chunks
    }

    /// Inverts the tape, swapping deletions and additions and inverting
    /// relocations.
    ///
    /// Forms a mathematical involution: `tape.invert().invert() == tape`.
    pub fn invert(self) -> Self {
        Self {
            chunks: self.chunks.into_iter().map(TapeChunk::invert).collect(),
        }
    }

    /// Returns a streaming iterator projecting the left (old/source) sequence.
    /// Yields items from `Keep`, `Strike`, and `Relocate` chunks.
    pub fn project_left_iter(&self) -> TapeProjection<'_, T, ChunkProjector<T>> {
        fn project_left<T>(chunk: &TapeChunk<T>) -> Option<&T> {
            match chunk {
                TapeChunk::Keep(item)
                | TapeChunk::Strike(item)
                | TapeChunk::Relocate { item, .. } => Some(item),
                TapeChunk::Fill(_) => None,
            }
        }
        TapeProjection::new(&self.chunks, project_left)
    }

    /// Returns a streaming iterator projecting the right (new/target) sequence.
    /// Yields items from `Keep`, `Fill`, and `Relocate` chunks.
    pub fn project_right_iter(&self) -> TapeProjection<'_, T, ChunkProjector<T>> {
        fn project_right<T>(chunk: &TapeChunk<T>) -> Option<&T> {
            match chunk {
                TapeChunk::Keep(item)
                | TapeChunk::Fill(item)
                | TapeChunk::Relocate { item, .. } => Some(item),
                TapeChunk::Strike(_) => None,
            }
        }
        TapeProjection::new(&self.chunks, project_right)
    }

    /// Maps items in the tape using function `f`.
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Tape<U> {
        Tape {
            chunks: self.chunks.into_iter().map(|c| c.map(&mut f)).collect(),
        }
    }
}

impl<T> std::ops::Add for Tape<T> {
    type Output = Self;

    #[inline]
    fn add(mut self, rhs: Self) -> Self::Output {
        self.compose(rhs);
        self
    }
}

impl<T> std::ops::AddAssign for Tape<T> {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.compose(rhs);
    }
}

impl<T> FromIterator<TapeChunk<T>> for Tape<T> {
    fn from_iter<I: IntoIterator<Item = TapeChunk<T>>>(iter: I) -> Self {
        Self {
            chunks: iter.into_iter().collect(),
        }
    }
}

/// Streaming iterator projecting a sub-sequence from an edit tape.
#[derive(Clone, Debug)]
pub struct TapeProjection<'a, T, F> {
    chunks: &'a [TapeChunk<T>],
    start: usize,
    end: usize,
    projector: F,
}

impl<'a, T, F> TapeProjection<'a, T, F>
where
    F: Fn(&'a TapeChunk<T>) -> Option<&'a T>,
{
    pub fn new(chunks: &'a [TapeChunk<T>], projector: F) -> Self {
        Self {
            chunks,
            start: 0,
            end: chunks.len(),
            projector,
        }
    }
}

impl<'a, T, F> Iterator for TapeProjection<'a, T, F>
where
    F: Fn(&'a TapeChunk<T>) -> Option<&'a T>,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while self.start < self.end {
            let chunk = &self.chunks[self.start];
            self.start += 1;
            if let Some(item) = (self.projector)(chunk) {
                return Some(item);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.end.saturating_sub(self.start)))
    }
}

impl<'a, T, F> DoubleEndedIterator for TapeProjection<'a, T, F>
where
    F: Fn(&'a TapeChunk<T>) -> Option<&'a T>,
{
    fn next_back(&mut self) -> Option<Self::Item> {
        while self.start < self.end {
            self.end -= 1;
            let chunk = &self.chunks[self.end];
            if let Some(item) = (self.projector)(chunk) {
                return Some(item);
            }
        }
        None
    }
}

impl<'a, T, F> std::iter::FusedIterator for TapeProjection<'a, T, F> where
    F: Fn(&'a TapeChunk<T>) -> Option<&'a T>
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projections_basic() {
        let mut tape = Tape::empty();
        tape.push(TapeChunk::Keep(1));
        tape.push(TapeChunk::Strike(2));
        tape.push(TapeChunk::Fill(3));
        tape.push(TapeChunk::Relocate {
            from_span: (0, 1),
            to_span: (2, 3),
            item: 4,
        });

        let left: Vec<_> = tape.project_left_iter().copied().collect();
        let right: Vec<_> = tape.project_right_iter().copied().collect();

        assert_eq!(
            left,
            vec![1, 2, 4],
            "left projection must yield Keep + Strike + Relocate"
        );
        assert_eq!(
            right,
            vec![1, 3, 4],
            "right projection must yield Keep + Fill + Relocate"
        );
    }

    #[test]
    fn test_monoid_identity() {
        let mut tape = Tape::empty();
        tape.push(TapeChunk::Keep("hello"));
        tape.push(TapeChunk::Strike("world"));

        let empty = Tape::empty();

        // T ∘ ∅ == T
        let mut t1 = tape.clone();
        t1.compose(empty.clone());
        assert_eq!(t1, tape);

        // ∅ ∘ T == T
        let mut t2 = empty;
        t2.compose(tape.clone());
        assert_eq!(t2, tape);

        // Add syntax: tape + Tape::empty() == tape
        assert_eq!(tape.clone() + Tape::empty(), tape);
    }

    #[test]
    fn test_monoid_associativity() {
        let mut a = Tape::empty();
        a.push(TapeChunk::Keep(1));

        let mut b = Tape::empty();
        b.push(TapeChunk::Strike(2));

        let mut c = Tape::empty();
        c.push(TapeChunk::Fill(3));

        // (A ∘ B) ∘ C == A ∘ (B ∘ C)
        let ab_c = (a.clone() + b.clone()) + c.clone();
        let a_bc = a + (b + c);
        assert_eq!(ab_c, a_bc);
    }

    #[test]
    fn test_monoid_involution() {
        let mut tape = Tape::empty();
        tape.push(TapeChunk::Keep("shared"));
        tape.push(TapeChunk::Strike("deleted"));
        tape.push(TapeChunk::Fill("added"));
        tape.push(TapeChunk::Relocate {
            from_span: (10, 20),
            to_span: (50, 60),
            item: "moved",
        });

        // Involution: invert(invert(T)) == T
        let inverted = tape.clone().invert();
        assert_eq!(inverted.clone().invert(), tape);

        // Dual projection property:
        // π_left(invert(T)) == π_right(T)
        let inv_left: Vec<_> = inverted.project_left_iter().copied().collect();
        let orig_right: Vec<_> = tape.project_right_iter().copied().collect();
        assert_eq!(inv_left, orig_right);

        // π_right(invert(T)) == π_left(T)
        let inv_right: Vec<_> = inverted.project_right_iter().copied().collect();
        let orig_left: Vec<_> = tape.project_left_iter().copied().collect();
        assert_eq!(inv_right, orig_left);
    }

    #[test]
    fn test_double_ended_projection_iterator() {
        let mut tape = Tape::empty();
        tape.push(TapeChunk::Keep(10));
        tape.push(TapeChunk::Strike(20));
        tape.push(TapeChunk::Fill(30));
        tape.push(TapeChunk::Keep(40));

        let mut iter = tape.project_left_iter();
        assert_eq!(iter.next(), Some(&10));
        assert_eq!(iter.next_back(), Some(&40));
        assert_eq!(iter.next(), Some(&20));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_tape_chunk_inspection_and_map() {
        let chunk = TapeChunk::Keep(100);
        assert!(chunk.is_keep());
        assert!(!chunk.is_strike());
        assert!(!chunk.is_fill());
        assert!(!chunk.is_relocate());
        assert_eq!(*chunk.item(), 100);
        assert_eq!(chunk.into_item(), 100);

        let strike = TapeChunk::Strike("old");
        assert!(strike.is_strike());
        let mapped = strike.map(|s| s.len());
        assert_eq!(mapped, TapeChunk::Strike(3));

        let fill = TapeChunk::Fill("new");
        assert!(fill.is_fill());

        let relocate = TapeChunk::Relocate {
            from_span: (1, 2),
            to_span: (3, 4),
            item: "item",
        };
        assert!(relocate.is_relocate());

        let mut tape: Tape<i32> = vec![TapeChunk::Keep(1), TapeChunk::Fill(2)]
            .into_iter()
            .collect();
        tape += Tape::empty();
        assert_eq!(tape.len(), 2);
        assert!(!tape.is_empty());

        let mapped_tape = tape.map(|x| x * 10);
        assert_eq!(
            mapped_tape.into_chunks(),
            vec![TapeChunk::Keep(10), TapeChunk::Fill(20)]
        );
    }
}
