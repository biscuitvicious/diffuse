use diffuse_parse::{AddLine, CtxLine, DelLine, Hunk, Row};
use gix::{
    bstr::ByteSlice,
    diff::blob::{
        Algorithm, InternedInput, UnifiedDiff, diff_with_slider_heuristics,
        unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader},
    },
};

use super::line::{DiffLines, WhitespaceMode, split_lines};

/// Hunk consumer accumulating structured [`diffuse_parse::Hunk`] instances
/// and unified diff text.
pub struct AstHunkConsumer<'a> {
    pub hunks: Vec<Hunk>,
    pub patch: String,
    old_lines: &'a [&'a [u8]],
    new_lines: &'a [&'a [u8]],
}

impl<'a> AstHunkConsumer<'a> {
    pub fn new(old_lines: &'a [&'a [u8]], new_lines: &'a [&'a [u8]]) -> Self {
        Self {
            hunks: Vec::new(),
            patch: String::new(),
            old_lines,
            new_lines,
        }
    }
}

impl<'a> ConsumeHunk for AstHunkConsumer<'a> {
    type Out = (Vec<Hunk>, String);

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        let mut hunk = Hunk {
            old_start: header.before_hunk_start,
            old_count: header.before_hunk_len,
            new_start: header.after_hunk_start,
            new_count: header.after_hunk_len,
            section: String::new(),
            rows: Vec::with_capacity(lines.len()),
            above_ctx: 0,
            below_ctx: 0,
        };

        use std::fmt::Write;
        let _ = writeln!(
            self.patch,
            "@@ -{},{} +{},{} @@",
            header.before_hunk_start,
            header.before_hunk_len,
            header.after_hunk_start,
            header.after_hunk_len
        );

        let mut old_pos = header.before_hunk_start;
        let mut new_pos = header.after_hunk_start;

        for &(kind, fallback_bytes) in lines {
            let line_bytes = match kind {
                DiffLineKind::Context => {
                    if new_pos > 0 && (new_pos as usize) <= self.new_lines.len() {
                        self.new_lines[(new_pos - 1) as usize]
                    } else if old_pos > 0 && (old_pos as usize) <= self.old_lines.len() {
                        self.old_lines[(old_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
                DiffLineKind::Remove => {
                    if old_pos > 0 && (old_pos as usize) <= self.old_lines.len() {
                        self.old_lines[(old_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
                DiffLineKind::Add => {
                    if new_pos > 0 && (new_pos as usize) <= self.new_lines.len() {
                        self.new_lines[(new_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
            };

            let text = line_bytes.to_str_lossy();
            let text_trimmed = text.trim_end_matches(['\r', '\n']);

            match kind {
                DiffLineKind::Context => {
                    self.patch.push(' ');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows.push(Row::Ctx(CtxLine::new(
                        old_pos,
                        new_pos,
                        text_trimmed.to_string(),
                    )));
                    old_pos += 1;
                    new_pos += 1;
                }
                DiffLineKind::Remove => {
                    self.patch.push('-');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows
                        .push(Row::del(DelLine::new(old_pos, text_trimmed.to_string())));
                    old_pos += 1;
                }
                DiffLineKind::Add => {
                    self.patch.push('+');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows
                        .push(Row::add(AddLine::new(new_pos, text_trimmed.to_string())));
                    new_pos += 1;
                }
            }
        }

        self.hunks.push(hunk);
        Ok(())
    }

    fn finish(self) -> Self::Out {
        (self.hunks, self.patch)
    }
}

/// Compute unified diff hunks and patch text between two byte buffers.
pub fn diff_buffers(
    old: &[u8],
    new: &[u8],
    context: u32,
    mode: WhitespaceMode,
) -> (Vec<Hunk>, String) {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let old_source = DiffLines::new(old, mode);
    let new_source = DiffLines::new(new, mode);
    let input = InternedInput::new(old_source, new_source);
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);
    let consumer = AstHunkConsumer::new(&old_lines, &new_lines);
    let unified = UnifiedDiff::new(&diff, &input, consumer, ContextSize::symmetrical(context));
    unified.consume().unwrap_or_default()
}

/// Convenience wrapper for diffing string slices.
pub fn diff_text(old: &str, new: &str, context: u32, mode: WhitespaceMode) -> (Vec<Hunk>, String) {
    diff_buffers(old.as_bytes(), new.as_bytes(), context, mode)
}
