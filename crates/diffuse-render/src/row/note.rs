//! Review note block segment rendering with syntax highlighting and tab
//! expansion.

use std::{fmt, fmt::Write};
use unicode_segmentation::UnicodeSegmentation;

use super::text::{expand_runs_tabs, expand_tabs, slice_by_visual_width};
use crate::{core::wire::OutputRun, theme::DiffuseTheme, width::grapheme_width};
use diffuse_syntax::color::TokenColor;

/// Write a single slice/segment of a review note row with syntax highlighting
/// directly to a buffer.
#[allow(clippy::too_many_arguments)]
pub fn write_note_segment<W: Write>(
    out: &mut W,
    text: &str,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let has_tab = text.contains('\t') || runs.iter().any(|r| r.text.contains('\t'));
    let exp_text = if has_tab {
        Some(expand_tabs(text, 4))
    } else {
        None
    };
    let text = exp_text.as_deref().unwrap_or(text);
    let exp_runs = if has_tab {
        Some(expand_runs_tabs(runs, 4))
    } else {
        None
    };
    let runs = exp_runs.as_deref().unwrap_or(runs);

    write!(out, "{prefix}")?;
    if runs.is_empty() {
        let piece = slice_by_visual_width(text, seg_start, seg_end.saturating_sub(seg_start));
        return out.write_str(&piece);
    }

    let mut pos = 0usize;
    for tok in runs {
        let tok_width: usize = tok.text.graphemes(true).map(grapheme_width).sum();
        let tok_start = pos;
        let tok_end = pos + tok_width;
        pos = tok_end;

        if tok_end <= seg_start || tok_start >= seg_end {
            continue;
        }

        let s = seg_start.max(tok_start);
        let e = seg_end.min(tok_end);
        let sub_piece =
            slice_by_visual_width(&tok.text, s.saturating_sub(tok_start), e.saturating_sub(s));
        if sub_piece.is_empty() {
            continue;
        }

        let color = if syntax_on {
            tok.color
        } else {
            TokenColor::Default
        };
        let sgr = if syntax_on { tok.sgr } else { None };
        theme.write_token(color, &sub_piece, "ctx", Some("ctx"), sgr, out);
    }

    Ok(())
}

/// Render a single slice/segment of a review note row with syntax highlighting.
#[allow(clippy::too_many_arguments)]
pub fn render_note_segment(
    text: &str,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_note_segment(
        &mut out, text, runs, seg_start, seg_end, prefix, syntax_on, theme,
    );
    out
}
