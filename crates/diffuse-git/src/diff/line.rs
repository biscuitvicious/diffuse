use std::hash::{Hash, Hasher};

use gix::{bstr::ByteSlice, diff::blob::TokenSource};

/// Whitespace comparison semantics for line diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceMode {
    /// Exact line equality including all whitespace and indentation.
    Exact,
    /// Canonical diffuse trim-equality: compare `line.trim()` for leading and
    /// trailing whitespace equality, while preserving internal token
    /// spacing.
    #[default]
    TrimEquality,
}

/// A line wrapper that hashes and compares lines according to a
/// [`WhitespaceMode`].
#[derive(Clone, Copy, Debug)]
pub struct DiffLine<'a> {
    pub bytes: &'a [u8],
    pub mode: WhitespaceMode,
}

impl<'a> AsRef<[u8]> for DiffLine<'a> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.bytes
    }
}

impl<'a> PartialEq for DiffLine<'a> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match self.mode {
            WhitespaceMode::Exact => self.bytes == other.bytes,
            WhitespaceMode::TrimEquality => self.bytes.trim() == other.bytes.trim(),
        }
    }
}

impl<'a> Eq for DiffLine<'a> {}

impl<'a> Hash for DiffLine<'a> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.mode {
            WhitespaceMode::Exact => self.bytes.hash(state),
            WhitespaceMode::TrimEquality => self.bytes.trim().hash(state),
        }
    }
}

/// Tokenizer yielding [`DiffLine`] items from byte slices.
#[derive(Clone, Copy)]
pub struct DiffLines<'a> {
    bytes: &'a [u8],
    mode: WhitespaceMode,
}

impl<'a> DiffLines<'a> {
    pub fn new(bytes: &'a [u8], mode: WhitespaceMode) -> Self {
        Self { bytes, mode }
    }
}

impl<'a> Iterator for DiffLines<'a> {
    type Item = DiffLine<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        let line_len = match self.bytes.find_byte(b'\n') {
            Some(pos) => pos + 1,
            None => self.bytes.len(),
        };
        let (line, rest) = self.bytes.split_at(line_len);
        self.bytes = rest;
        Some(DiffLine {
            bytes: line,
            mode: self.mode,
        })
    }
}

impl<'a> TokenSource for DiffLines<'a> {
    type Token = DiffLine<'a>;
    type Tokenizer = Self;

    #[inline]
    fn tokenize(&self) -> Self::Tokenizer {
        *self
    }

    #[inline]
    fn estimate_tokens(&self) -> u32 {
        self.bytes.lines().count() as u32
    }
}

/// Split a byte slice into line slices preserving line endings.
pub fn split_lines(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    while !bytes.is_empty() {
        let line_len = match bytes.find_byte(b'\n') {
            Some(pos) => pos + 1,
            None => bytes.len(),
        };
        let (line, rest) = bytes.split_at(line_len);
        lines.push(line);
        bytes = rest;
    }
    lines
}
