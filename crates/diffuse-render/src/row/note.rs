//! Review note block segment rendering with syntax highlighting and tab
//! expansion.

use std::{fmt, fmt::Write};
use unicode_segmentation::UnicodeSegmentation;

use super::text::{expand_runs_tabs, expand_tabs, slice_by_visual_width};
use crate::{
    core::{merge::RunKind, parse::RowKind, wire::OutputRun},
    span::{StyledSpan, write_span_token},
    theme::DiffuseTheme,
    width::grapheme_width,
};

/// Project a review note row slice onto ordered [`StyledSpan`]s: the
/// caller prefix spans plus the sliced run pieces. The tab-expansion
/// pre-step mirrors [`write_note_segment`], so builders see the same
/// expanded text the ANSI path slices. Run pieces become
/// [`DiffuseTheme::token_span`]s (`ctx` channel, exactly as
/// `write_token` resolves them), unhighlighted pieces stay plain.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn note_segment_spans(
    text: &str,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &[StyledSpan],
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> Vec<StyledSpan> {
    let has_tab = text.contains('\t') || runs.iter().any(|r| r.text.contains('\t'));
    let exp_text;
    let exp_runs;
    let (text, runs): (&str, &[OutputRun]) = if has_tab {
        exp_text = expand_tabs(text, 4);
        exp_runs = expand_runs_tabs(runs, 4);
        (&exp_text, &exp_runs)
    } else {
        (text, runs)
    };

    let mut out = prefix.to_vec();
    if runs.is_empty() {
        let piece = slice_by_visual_width(text, seg_start, seg_end.saturating_sub(seg_start));
        out.push(StyledSpan::plain(piece));
        return out;
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

        if !syntax_on {
            out.push(StyledSpan::plain(sub_piece));
            continue;
        }
        out.push(theme.token_span(tok.color, &sub_piece, RowKind::Ctx, RunKind::Ctx, tok.sgr));
    }

    out
}

/// Write a single slice/segment of a review note row with syntax highlighting
/// directly to a buffer. Style decisions come from [`note_segment_spans`];
/// emission keeps the exact skeleton (prefix text, one token emission per
/// run) so output stays byte-identical.
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
    // Plain prefixes serialize to their bare text, exactly as the ANSI
    // path writes the prefix string verbatim.
    let spans = note_segment_spans(
        text,
        runs,
        seg_start,
        seg_end,
        &[StyledSpan::plain(prefix)],
        syntax_on,
        theme,
    );
    for span in &spans {
        write_span_token(out, span)?;
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
