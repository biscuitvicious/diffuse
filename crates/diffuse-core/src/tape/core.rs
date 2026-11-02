//! Alignment tape core structure, monoidal composition, and projections.

use super::chunk::TapeChunk;

/// An immutable, monoidal sequence of alignment operations between old and
/// new text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlignmentTape<'a> {
    pub(crate) chunks: Vec<TapeChunk<'a>>,
}

impl<'a> AlignmentTape<'a> {
    /// Create an empty alignment tape.
    #[inline]
    pub fn empty() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Append a chunk to the tape, skipping empty slices.
    pub fn push(&mut self, chunk: TapeChunk<'a>) {
        if chunk.is_empty() {
            return;
        }
        self.chunks.push(chunk);
    }

    /// Monoidally compose another tape into this tape.
    #[inline]
    pub fn compose(&mut self, other: AlignmentTape<'a>) {
        self.chunks.extend(other.chunks);
    }

    /// Borrow the underlying chunks.
    #[inline]
    pub fn chunks(&self) -> &[TapeChunk<'a>] {
        &self.chunks
    }

    /// Returns a streaming projection iterator over the old text sequence.
    pub fn project_old_iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.chunks.iter().filter_map(|chunk| match chunk {
            TapeChunk::Keep(text) | TapeChunk::Strike(text) | TapeChunk::Relocate { text, .. } => {
                Some(*text)
            }
            TapeChunk::Fill(_) => None,
        })
    }

    /// Returns a streaming projection iterator over the new text sequence.
    pub fn project_new_iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.chunks.iter().filter_map(|chunk| match chunk {
            TapeChunk::Keep(text) | TapeChunk::Fill(text) | TapeChunk::Relocate { text, .. } => {
                Some(*text)
            }
            TapeChunk::Strike(_) => None,
        })
    }

    /// Projects the old text: concatenates `Keep` + `Strike` + `Relocate`.
    pub fn project_old(&self) -> String {
        let mut s = String::new();
        for part in self.project_old_iter() {
            s.push_str(part);
        }
        s
    }

    /// Projects the new text: concatenates `Keep` + `Fill` + `Relocate`.
    pub fn project_new(&self) -> String {
        let mut s = String::new();
        for part in self.project_new_iter() {
            s.push_str(part);
        }
        s
    }

    /// Compute the algebraic inverse alignment tape.
    ///
    /// Swaps `Strike` and `Fill`, inverts `Relocate` spans, and preserves
    /// `Keep`. Satisfies the involution property: `tape.reverse().reverse()
    /// == tape`.
    pub fn reverse(mut self) -> AlignmentTape<'a> {
        for c in &mut self.chunks {
            *c = match c {
                TapeChunk::Keep(s) => TapeChunk::Keep(s),
                TapeChunk::Strike(s) => TapeChunk::Fill(s),
                TapeChunk::Fill(s) => TapeChunk::Strike(s),
                TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text,
                } => TapeChunk::Relocate {
                    from_span: *to_span,
                    to_span: *from_span,
                    text,
                },
            };
        }
        self
    }

    /// Converts this alignment tape into an algebraic `Tape<&'a str>`.
    pub fn to_algebraic_tape(&self) -> diffuse_algo::tape::Tape<&'a str> {
        self.chunks
            .iter()
            .map(|c| match c {
                TapeChunk::Keep(s) => diffuse_algo::tape::TapeChunk::Keep(*s),
                TapeChunk::Strike(s) => diffuse_algo::tape::TapeChunk::Strike(*s),
                TapeChunk::Fill(s) => diffuse_algo::tape::TapeChunk::Fill(*s),
                TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text,
                } => diffuse_algo::tape::TapeChunk::Relocate {
                    from_span: *from_span,
                    to_span: *to_span,
                    item: *text,
                },
            })
            .collect()
    }

    /// Creates an `AlignmentTape` from an algebraic `Tape<&'a str>`.
    pub fn from_algebraic_tape(tape: diffuse_algo::tape::Tape<&'a str>) -> Self {
        let chunks = tape
            .into_chunks()
            .into_iter()
            .map(|c| match c {
                diffuse_algo::tape::TapeChunk::Keep(s) => TapeChunk::Keep(s),
                diffuse_algo::tape::TapeChunk::Strike(s) => TapeChunk::Strike(s),
                diffuse_algo::tape::TapeChunk::Fill(s) => TapeChunk::Fill(s),
                diffuse_algo::tape::TapeChunk::Relocate {
                    from_span,
                    to_span,
                    item,
                } => TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text: item,
                },
            })
            .collect();
        Self { chunks }
    }
}
