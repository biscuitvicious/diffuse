//! Diff row segment rendering with ANSI styling, background tinting, and column
//! slicing.

use std::{borrow::Cow, fmt, fmt::Write};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    gutter::write_gutter_prefix,
    text::{display_text, expand_runs_tabs, expand_tabs, slice_by_visual_width},
};
use crate::{
    ansi::{Color, ColorExt},
    core::{
        lcs::char_slice,
        merge::RunKind,
        parse::{RowKind, Spans},
        wire::{ModSide, MovSide, OutputRow, OutputRun},
    },
    span::{StyledSpan, pad_spans, spans_width, write_span_token},
    theme::{Cursor, DiffKind, DiffuseTheme},
    width::grapheme_width,
};

/// Background escape for an optional line color: [`None`] and
/// [`Color::Primary`] inherit (no escape), matching the ANSI path where a
/// primary background resolves to an empty code.
fn bg_code(bg: Option<Color>) -> String {
    match bg.filter(|color| *color != Color::Primary) {
        Some(color) => color.bg_code(),
        None => String::new(),
    }
}

/// Check whether a row consists solely of common/context runs.
/// Addition rows added in diffs are never treated as context rows.
#[inline]
pub fn is_common_only_row(_kind: RowKind, _runs: &[OutputRun]) -> bool {
    false
}

/// Check if a mod row is a pure deletion (e.g. whitespace-only additions).
pub fn is_pure_deletion_mod(spans: &Spans, text: &str) -> bool {
    spans.add.iter().all(|&(s, e)| {
        if s <= e {
            char_slice(text, s, e).trim().is_empty()
        } else {
            true
        }
    })
}

/// Write a single slice/segment of a row with full ANSI styling directly to a
/// buffer. Style decisions come from [`row_segment_spans`]; this keeps the
/// exact emission skeleton (lead background, gutter prefix, per-run
/// backgrounds, trail background) so output stays byte-identical.
/// `is_continuation` selects the wrapped-segment prefix spelling (no
/// italic-off quirks, as on the ANSI path).
#[allow(clippy::too_many_arguments)]
pub fn write_row_segment<W: Write>(
    out: &mut W,
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &[StyledSpan],
    is_continuation: bool,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let row = row_segment_spans(
        text,
        is_aux_row,
        kind,
        mov_side,
        mod_side,
        runs,
        seg_start,
        seg_end,
        prefix.to_vec(),
        selected,
        band,
        col_offset,
        word_diff,
        syntax_on,
        theme,
    );
    let line_bg_code = bg_code(row.line_bg);
    let ctx_tinted = theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg != Color::Primary;
    // Mirrors the old `prefix.starts_with("\\x1b")` probe: a prefix with
    // no foreground color carries no escapes.
    let has_styled_prefix = prefix
        .iter()
        .any(|span| span.fg.is_some_and(|color| color != Color::Primary))
        || selected
        || band
        || ctx_tinted;
    let lead_bg = if has_styled_prefix {
        line_bg_code.clone()
    } else {
        String::new()
    };
    let is_ctx = matches!(kind, RowKind::Ctx) || is_common_only_row(kind, runs);
    let trail_bg = if selected || band || !is_ctx || ctx_tinted {
        line_bg_code
    } else {
        String::new()
    };
    out.write_str(&lead_bg)?;
    let is_mov_src = mov_side == Some(MovSide::Src);
    let is_del_num = kind == RowKind::Del || is_mov_src || mod_side == Some(ModSide::Del);
    let mid_italic_off = is_del_num && !is_continuation;
    write_gutter_prefix(out, prefix, mid_italic_off, !is_continuation)?;
    let body = &row.spans[prefix.len()..];
    // Fast-path conditions mirror `row_segment_spans`: plain deletion rows and
    // run-less rows render one body span with no trailing background.
    let is_mov_src = mov_side == Some(MovSide::Src);
    let is_plain_del =
        kind == RowKind::Del || (is_mov_src && !runs.iter().any(|t| t.kind == RunKind::Ctx));
    let body_empty = if runs.iter().any(|tok| tok.kind == RunKind::MovedSrc) {
        runs.iter().all(|tok| tok.kind == RunKind::MovedSrc)
    } else {
        runs.is_empty()
    };
    if is_plain_del || body_empty {
        if let Some(span) = body.first() {
            out.write_str(&bg_code(span.bg))?;
            write_span_token(out, span)?;
        }
    } else {
        for span in body {
            out.write_str(&bg_code(span.bg))?;
            write_span_token(out, span)?;
            // Deletion-run spans close without a trailing background, as
            // on the ANSI path; every other run repaints its background.
            if span.sgrs.is_some() || !span.strikethrough {
                out.write_str(&bg_code(span.bg))?;
            }
        }
    }
    out.write_str(&trail_bg)?;
    Ok(())
}

/// Render a single slice/segment of a row with full ANSI styling.
/// Style decisions come from [`row_segment_spans`]; emission keeps the
/// exact skeleton so output stays byte-identical.
#[allow(clippy::too_many_arguments)]
pub fn render_row_segment(
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &[StyledSpan],
    is_continuation: bool,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_row_segment(
        &mut out,
        text,
        is_aux_row,
        kind,
        mov_side,
        None,
        runs,
        seg_start,
        seg_end,
        prefix,
        is_continuation,
        selected,
        band,
        col_offset,
        word_diff,
        syntax_on,
        theme,
    );
    out
}

#[allow(clippy::too_many_arguments)]
pub fn render_row_segment_with_mod_side(
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &[StyledSpan],
    is_continuation: bool,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_row_segment(
        &mut out,
        text,
        is_aux_row,
        kind,
        mov_side,
        mod_side,
        runs,
        seg_start,
        seg_end,
        prefix,
        is_continuation,
        selected,
        band,
        col_offset,
        word_diff,
        syntax_on,
        theme,
    );
    out
}

/// A row segment as ordered styled spans plus its resolved line background:
/// the span-native twin of [`render_row_segment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSegmentLine {
    /// Ordered styled spans covering the segment window, prefix first.
    pub spans: Vec<StyledSpan>,
    /// Resolved line background ([`None`] inherits the canvas). The prefix
    /// and any padding ride on it, exactly as the ANSI lead and trail
    /// backgrounds.
    pub line_bg: Option<Color>,
}

/// Build the span-native twin of [`render_row_segment`]: identical text,
/// foregrounds, backgrounds, and modifiers as ordered [`StyledSpan`]s, so
/// the Ratatui grid path renders without an ANSI round-trip. Branching
/// mirrors [`write_row_segment`] arm-for-arm (including tab expansion,
/// column-offset mapping, and the full-row fast path); the ANSI function
/// stays the canonical batch serializer.
#[allow(clippy::too_many_arguments)]
pub fn row_segment_spans(
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: Vec<StyledSpan>,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> RowSegmentLine {
    let is_ctx = matches!(kind, RowKind::Ctx) || is_common_only_row(kind, runs);
    let is_mov_src = mov_side == Some(MovSide::Src);
    let is_del_row = kind == RowKind::Del || is_mov_src || mod_side == Some(ModSide::Del);
    let is_mod_row = kind == RowKind::Mod && mod_side.is_none();

    let band_bg = theme.palette[Cursor::Row].band.bg;
    let ctx_cursor_bg = theme.palette[Cursor::Row][DiffKind::Ctx].surface.bg;
    let ctx_tint_bg = theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg;

    let s_mod = theme.row_style(RowKind::Mod);
    let s_del = theme.row_style(RowKind::Del);
    let s_add = theme.row_style(RowKind::Add);

    let line_bg = if is_ctx {
        if selected {
            Some(ctx_cursor_bg)
        } else if band {
            Some(band_bg)
        } else {
            Some(ctx_tint_bg)
        }
    } else if is_mod_row {
        if selected {
            Some(s_mod.cursor_bg)
        } else if band {
            Some(band_bg)
        } else {
            Some(s_mod.tint_bg)
        }
    } else if is_del_row {
        if selected {
            Some(s_del.cursor_bg)
        } else if band {
            Some(band_bg)
        } else {
            Some(s_del.tint_bg)
        }
    } else if selected {
        Some(s_add.cursor_bg)
    } else if band {
        Some(band_bg)
    } else {
        Some(s_add.tint_bg)
    };

    let word_bg = if selected {
        s_mod.fill_cursor_bg
    } else {
        s_mod.fill_bg
    };
    let del_bg = s_del.fill_bg;
    let del_fg = s_del.fg;

    let mut spans: Vec<StyledSpan> = prefix;
    for span in &mut spans {
        span.bg = line_bg;
    }

    let has_subwords =
        runs.iter().any(|t| t.kind == RunKind::Add) && runs.iter().any(|t| t.kind != RunKind::Add);
    let is_expansion_add = kind == RowKind::Add && is_aux_row;

    let filtered_runs: Vec<OutputRun>;
    let runs: &[OutputRun] = match (mov_side, mod_side) {
        (Some(MovSide::Src), _) | (_, Some(ModSide::Del)) => {
            filtered_runs = runs
                .iter()
                .filter(|t| !t.kind.is_add_like() && t.kind != RunKind::MovedSrc)
                .cloned()
                .collect();
            &filtered_runs
        }
        (Some(MovSide::Tgt), _) | (_, Some(ModSide::Add)) => {
            filtered_runs = runs
                .iter()
                .filter(|t| !t.kind.is_del_like())
                .cloned()
                .collect();
            &filtered_runs
        }
        _ if runs.iter().any(|t| t.kind == RunKind::MovedSrc) => {
            filtered_runs = runs
                .iter()
                .filter(|t| t.kind != RunKind::MovedSrc)
                .cloned()
                .collect();
            &filtered_runs
        }
        _ => runs,
    };

    let base_bg = |tok_kind: RunKind| -> Option<Color> {
        if is_ctx {
            line_bg
        } else if tok_kind == RunKind::Del {
            del_bg
        } else if tok_kind == RunKind::Add
            && (is_mod_row || has_subwords || is_expansion_add || mod_side == Some(ModSide::Add))
        {
            word_bg
        } else if matches!(tok_kind, RunKind::Ctx | RunKind::MovedTgt | RunKind::Add) {
            line_bg
        } else {
            unreachable!("moved-out runs are filtered before rendering")
        }
    };

    let is_full_row = seg_start == 0 && seg_end == usize::MAX && col_offset == 0;

    let (runs_storage, runs): (Option<Vec<OutputRun>>, &[OutputRun]) =
        if !is_full_row && runs.iter().any(|r| r.text.contains('\t')) {
            let exp = expand_runs_tabs(runs, 4);
            (Some(exp), &[])
        } else {
            (None, runs)
        };
    let runs = runs_storage.as_deref().unwrap_or(runs);

    let exp_text = if !is_full_row && text.contains('\t') {
        Cow::Owned(expand_tabs(text, 4).into_owned())
    } else {
        Cow::Borrowed(text)
    };
    let text = exp_text.as_ref();

    let is_plain_del = kind == RowKind::Del
        || ((is_mov_src || mod_side == Some(ModSide::Del))
            && !runs.iter().any(|t| t.kind == RunKind::Ctx));
    if is_plain_del {
        let body = if is_full_row {
            text.to_string()
        } else {
            let disp = display_text(text, is_aux_row, kind, runs, col_offset, word_diff);
            slice_by_visual_width(&disp, seg_start, seg_end.saturating_sub(seg_start))
        };
        // Deletion rows render as plain filled text: no strikethrough (the
        // strike is reserved for deletion *spans* inside mod rows).
        spans.push(StyledSpan {
            text: body,
            fg: Some(del_fg),
            bg: line_bg,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            sgrs: None,
        });
        return RowSegmentLine { spans, line_bg };
    }

    if runs.is_empty() {
        let body = if is_full_row {
            text.to_string()
        } else {
            let disp = display_text(text, is_aux_row, kind, runs, col_offset, word_diff);
            slice_by_visual_width(&disp, seg_start, seg_end.saturating_sub(seg_start))
        };
        spans.push(StyledSpan::styled(body, None, line_bg));
        return RowSegmentLine { spans, line_bg };
    }

    if is_full_row {
        for tok in runs {
            if tok.text.is_empty() {
                continue;
            }
            let bg = base_bg(tok.kind);
            if tok.kind == RunKind::Del {
                spans.push(StyledSpan {
                    text: tok.text.to_string(),
                    fg: Some(del_fg),
                    bg,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                    strikethrough: true,
                    sgrs: None,
                });
            } else if !syntax_on {
                spans.push(StyledSpan::styled(tok.text.to_string(), None, bg));
            } else {
                let mut span = theme.token_span(tok.color, &tok.text, kind, tok.kind, tok.sgr);
                span.bg = bg;
                spans.push(span);
            }
        }
        return RowSegmentLine { spans, line_bg };
    }

    let skipped_visual: usize = if col_offset == 0 {
        0
    } else {
        runs.iter()
            .flat_map(|tok| tok.text.graphemes(true))
            .take(col_offset)
            .map(grapheme_width)
            .sum()
    };
    let mut pos = 0usize;
    for tok in runs {
        let tok_width: usize = tok.text.graphemes(true).map(grapheme_width).sum();
        let tok_start = pos;
        let tok_end = pos + tok_width;
        pos = tok_end;

        let d_start = tok_start.saturating_sub(skipped_visual);
        let d_end = tok_end.saturating_sub(skipped_visual);
        if d_end <= seg_start || d_start >= seg_end {
            continue;
        }

        let s = seg_start.max(d_start);
        let e = seg_end.min(d_end);
        let sub_piece = slice_by_visual_width(
            &tok.text,
            (s + skipped_visual).saturating_sub(tok_start),
            e.saturating_sub(s),
        );
        if sub_piece.is_empty() {
            continue;
        }

        let bg = base_bg(tok.kind);
        if tok.kind == RunKind::Del {
            spans.push(StyledSpan {
                text: sub_piece,
                fg: Some(del_fg),
                bg,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
                strikethrough: true,
                sgrs: None,
            });
        } else if !syntax_on {
            spans.push(StyledSpan::styled(sub_piece, None, bg));
        } else {
            let mut span = theme.token_span(tok.color, &sub_piece, kind, tok.kind, tok.sgr);
            span.bg = bg;
            spans.push(span);
        }
    }

    RowSegmentLine { spans, line_bg }
}

/// Write a slice of styled spans to an ANSI writer, updating background
/// escapes only when the background changes and resetting at the end.
pub fn write_spans_ansi<W: Write>(out: &mut W, spans: &[StyledSpan]) -> fmt::Result {
    let mut cur_bg: Option<Color> = None;
    for span in spans {
        let effective_bg = span.bg.filter(|c| *c != Color::Primary);
        if effective_bg != cur_bg {
            match effective_bg {
                Some(color) => color.write_bg(out)?,
                None => out.write_str(crate::ansi::BG_RESET)?,
            }
            cur_bg = effective_bg;
        }
        write_span_token(out, span)?;
    }
    if cur_bg.is_some() {
        out.write_str(crate::ansi::BG_RESET)?;
    }
    Ok(())
}

#[inline]
const fn is_left_active(kind: RowKind, mov_side: Option<MovSide>) -> bool {
    match kind {
        RowKind::Ctx | RowKind::Mod | RowKind::Del => true,
        RowKind::Mov => match mov_side {
            Some(MovSide::Src) => true,
            Some(MovSide::Tgt) => false,
            None => true,
        },
        _ => false,
    }
}

#[inline]
const fn is_right_active(kind: RowKind, mov_side: Option<MovSide>) -> bool {
    match kind {
        RowKind::Ctx | RowKind::Mod | RowKind::Add => true,
        RowKind::Mov => match mov_side {
            Some(MovSide::Tgt) => true,
            Some(MovSide::Src) => false,
            None => true,
        },
        _ => false,
    }
}

fn build_blank_pane_spans(
    pane_width: usize,
    mut prefix: Vec<StyledSpan>,
    blank_bg: Option<Color>,
) -> (Vec<StyledSpan>, Option<Color>) {
    for span in &mut prefix {
        span.bg = blank_bg;
    }
    let padded = pad_spans(prefix, pane_width, blank_bg);
    (padded, blank_bg)
}

#[allow(clippy::too_many_arguments)]
fn build_pane_segment_spans(
    text: &str,
    is_aux: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    runs: &[OutputRun],
    seg_idx: usize,
    pane_width: usize,
    prefix: Vec<StyledSpan>,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> (Vec<StyledSpan>, Option<Color>) {
    let gutter_w = spans_width(&prefix);
    let cols = pane_width.saturating_sub(gutter_w).max(1);
    let seg_start = seg_idx * cols;
    let seg_end = seg_start + cols;
    let line = row_segment_spans(
        text, is_aux, kind, mov_side, mod_side, runs, seg_start, seg_end, prefix, selected, band,
        col_offset, word_diff, syntax_on, theme,
    );
    let padded = pad_spans(line.spans, pane_width, line.line_bg);
    (padded, line.line_bg)
}

/// Compose a side-by-side row segment consisting of the left (old / pre-image)
/// pane, a 1-column vertical divider (`│`), and the right (new / post-image)
/// pane.
#[allow(clippy::too_many_arguments)]
pub fn side_by_side_row_segment_spans(
    row: &OutputRow,
    s_idx: usize,
    left_width: usize,
    right_width: usize,
    left_prefix: Vec<StyledSpan>,
    right_prefix: Vec<StyledSpan>,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> RowSegmentLine {
    if left_width == 0 && right_width == 0 {
        return RowSegmentLine {
            spans: Vec::new(),
            line_bg: None,
        };
    }

    let blank_bg = if selected || band {
        Some(theme.palette[Cursor::Row].band.bg)
    } else {
        None
    };

    let (left_spans, left_bg) = if left_width == 0 {
        (Vec::new(), None)
    } else if is_left_active(row.kind, row.mov_side) {
        let (kind, mod_side) = if row.kind == RowKind::Mod {
            (RowKind::Mod, Some(ModSide::Del))
        } else {
            (row.kind, None)
        };
        build_pane_segment_spans(
            &row.old_text(),
            row.is_aux(),
            kind,
            row.mov_side,
            mod_side,
            &row.runs,
            s_idx,
            left_width,
            left_prefix,
            selected,
            band,
            col_offset,
            word_diff,
            syntax_on,
            theme,
        )
    } else {
        build_blank_pane_spans(left_width, left_prefix, blank_bg)
    };

    let (right_spans, _right_bg) = if right_width == 0 {
        (Vec::new(), None)
    } else if is_right_active(row.kind, row.mov_side) {
        let (kind, mod_side) = if row.kind == RowKind::Mod {
            (RowKind::Mod, Some(ModSide::Add))
        } else {
            (row.kind, None)
        };
        build_pane_segment_spans(
            &row.new_text(),
            row.is_aux(),
            kind,
            row.mov_side,
            mod_side,
            &row.runs,
            s_idx,
            right_width,
            right_prefix,
            selected,
            band,
            col_offset,
            word_diff,
            syntax_on,
            theme,
        )
    } else {
        build_blank_pane_spans(right_width, right_prefix, blank_bg)
    };

    let line_bg = if row.kind == RowKind::Ctx {
        left_bg
    } else if selected || band {
        Some(theme.palette[Cursor::Row].band.bg)
    } else {
        None
    };

    let mut spans = Vec::with_capacity(left_spans.len() + right_spans.len() + 1);
    spans.extend(left_spans);

    if left_width > 0 && right_width > 0 {
        let divider_color = theme.palette[Cursor::Rest][DiffKind::Ctx]
            .syntax
            .table_border;
        let divider_bg = if selected || band {
            Some(theme.palette[Cursor::Row].band.bg)
        } else {
            None
        };
        spans.push(StyledSpan::styled("│", Some(divider_color), divider_bg));
    }

    spans.extend(right_spans);

    RowSegmentLine { spans, line_bg }
}

/// Write a side-by-side row segment directly to an ANSI output writer.
#[allow(clippy::too_many_arguments)]
pub fn write_side_by_side_row_segment<W: Write>(
    out: &mut W,
    row: &OutputRow,
    s_idx: usize,
    left_width: usize,
    right_width: usize,
    left_prefix: &[StyledSpan],
    right_prefix: &[StyledSpan],
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let seg_line = side_by_side_row_segment_spans(
        row,
        s_idx,
        left_width,
        right_width,
        left_prefix.to_vec(),
        right_prefix.to_vec(),
        selected,
        band,
        col_offset,
        word_diff,
        syntax_on,
        theme,
    );
    write_spans_ansi(out, &seg_line.spans)
}

/// Render a side-by-side row segment with ANSI escape sequences to a
/// [`String`].
#[allow(clippy::too_many_arguments)]
pub fn render_side_by_side_row_segment(
    row: &OutputRow,
    s_idx: usize,
    left_width: usize,
    right_width: usize,
    left_prefix: &[StyledSpan],
    right_prefix: &[StyledSpan],
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(left_width + right_width + 64);
    let _ = write_side_by_side_row_segment(
        &mut out,
        row,
        s_idx,
        left_width,
        right_width,
        left_prefix,
        right_prefix,
        selected,
        band,
        col_offset,
        word_diff,
        syntax_on,
        theme,
    );
    out
}
