//! Visual column measurement, tab expansion, and display text slicing.

use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    core::{merge::RunKind, parse::RowKind, wire::OutputRun},
    width::{char_width, grapheme_width},
};

/// Extract display text of a row considering word diff and column offset.
pub fn display_text(
    text: &str,
    is_peer_row: bool,
    kind: RowKind,
    runs: &[OutputRun],
    col_offset: usize,
    word_diff: bool,
) -> String {
    let use_merged =
        (kind == RowKind::Mod || kind == RowKind::Mov || (kind == RowKind::Add && is_peer_row))
            && word_diff
            && !runs.is_empty();

    // Measure what renders: moved-out source text is skipped by the
    // renderer, so counting it here would wrap rows onto blank corpse
    // lines (geometry) and mis-slice scrolled views.
    let full_text = if use_merged {
        runs.iter()
            .filter(|r| r.kind != RunKind::MovedSrc)
            .map(|r| r.text.as_ref())
            .collect::<String>()
    } else {
        text.to_string()
    };

    if col_offset > 0 {
        full_text.graphemes(true).skip(col_offset).collect()
    } else {
        full_text
    }
}

/// Expands tabs to spaces using tab stops of `tab_width` columns.
pub fn expand_tabs(s: &str, tab_width: usize) -> Cow<'_, str> {
    if !s.contains('\t') {
        return Cow::Borrowed(s);
    }
    let tab_w = tab_width.max(1);
    let mut out = String::with_capacity(s.len() + 16);
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            let spaces = tab_w - (col % tab_w);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            let w = char_width(c);
            col += w;
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Expands tabs to spaces across a slice of [`OutputRun`]s, keeping visual
/// column alignment consistent across token boundaries.
pub fn expand_runs_tabs(runs: &[OutputRun], tab_width: usize) -> Vec<OutputRun> {
    if !runs.iter().any(|r| r.text.contains('\t')) {
        return runs.to_vec();
    }
    let tab_w = tab_width.max(1);
    let mut out = Vec::with_capacity(runs.len());
    let mut col = 0usize;
    for r in runs {
        if !r.text.contains('\t') {
            for c in r.text.chars() {
                col += char_width(c);
            }
            out.push(r.clone());
        } else {
            let mut exp = String::with_capacity(r.text.len() + 8);
            for c in r.text.chars() {
                if c == '\t' {
                    let spaces = tab_w - (col % tab_w);
                    for _ in 0..spaces {
                        exp.push(' ');
                    }
                    col += spaces;
                } else {
                    let w = char_width(c);
                    col += w;
                    exp.push(c);
                }
            }
            out.push(OutputRun {
                kind: r.kind,
                color: r.color,
                sgr: r.sgr,
                text: exp.into(),
            });
        }
    }
    out
}

/// Slices text by visual terminal column width rather than raw bytes or
/// character counts.
pub fn slice_by_visual_width(text: &str, start_col: usize, max_cols: usize) -> String {
    let exp_text = expand_tabs(text, 4);
    let mut current_col = 0;
    let mut out = String::new();
    let mut accumulated = 0;
    for g in exp_text.graphemes(true) {
        let gw = grapheme_width(g);
        if current_col >= start_col && accumulated + gw <= max_cols {
            out.push_str(g);
            accumulated += gw;
        }
        current_col += gw;
        if current_col >= start_col + max_cols {
            break;
        }
    }
    out
}
