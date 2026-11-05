//! Diff row segment rendering with ANSI styling, background tinting, and column
//! slicing.

use std::{borrow::Cow, fmt, fmt::Write};
use unicode_segmentation::UnicodeSegmentation;

use super::text::{display_text, expand_runs_tabs, expand_tabs, slice_by_visual_width};
use crate::{
    ansi::{Color, ColorExt},
    core::{
        lcs::char_slice,
        merge::RunKind,
        parse::{RowKind, Spans},
        wire::{MovSide, OutputRun},
    },
    theme::{Cursor, DiffKind, DiffuseTheme},
    width::grapheme_width,
};

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
/// buffer.
#[allow(clippy::too_many_arguments)]
pub fn write_row_segment<W: Write>(
    out: &mut W,
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &str,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let is_ctx = matches!(kind, RowKind::Ctx) || is_common_only_row(kind, runs);
    let is_mov_src = mov_side == Some(MovSide::Src);
    let is_del_row = kind == RowKind::Del || is_mov_src;
    let is_mod_row = kind == RowKind::Mod;

    // The visual-selection band: one neutral shade applied across the whole
    // row (distinct from the cursor's per-style lift). Resolved from the row
    // cursor tier (the asset declares it once and it cascades to every tier).
    let band_bg = theme.palette[Cursor::Row].band.bg.bg_code();
    // Interactive cursor backgrounds live at the row tier.
    let ctx_cursor_bg = theme.palette[Cursor::Row][DiffKind::Ctx]
        .surface
        .bg
        .bg_code();
    let ctx_tint_bg = match theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg {
        Color::Primary => String::new(),
        color => color.bg_code(),
    };

    let s_mod = theme.row_style(RowKind::Mod);
    let s_del = theme.row_style(RowKind::Del);
    let s_add = theme.row_style(RowKind::Add);

    let s_mod_cursor_bg = s_mod.cursor_bg();
    let s_mod_tint_bg = s_mod.tint_bg();
    let s_del_cursor_bg = s_del.cursor_bg();
    let s_del_tint_bg = s_del.tint_bg();
    let s_add_cursor_bg = s_add.cursor_bg();
    let s_add_tint_bg = s_add.tint_bg();

    let line_bg: &str = if is_ctx {
        if selected {
            &ctx_cursor_bg
        } else if band {
            &band_bg
        } else {
            &ctx_tint_bg
        }
    } else if is_mod_row {
        if selected {
            &s_mod_cursor_bg
        } else if band {
            &band_bg
        } else {
            &s_mod_tint_bg
        }
    } else if is_del_row {
        if selected {
            &s_del_cursor_bg
        } else if band {
            &band_bg
        } else {
            &s_del_tint_bg
        }
    } else if selected {
        &s_add_cursor_bg
    } else if band {
        &band_bg
    } else {
        &s_add_tint_bg
    };

    let word_bg = if selected {
        s_mod.fill_cursor_bg()
    } else {
        s_mod.fill_bg()
    };
    let del_bg = s_del.tint_bg();

    let has_subwords =
        runs.iter().any(|t| t.kind == RunKind::Add) && runs.iter().any(|t| t.kind != RunKind::Add);
    let is_expansion_add = kind == RowKind::Add && is_aux_row;

    // Moved-out source text never renders (struck or otherwise): the aux
    // rows portion it out as context, so emitting it here would claim the
    // text twice. The wire keeps the runs. Filtered once up front so the
    // full-row and segmented paths — including visual-column bookkeeping —
    // agree on displayed text; no allocation when no moved runs exist.
    let filtered_runs: Vec<OutputRun>;
    let runs: &[OutputRun] = if runs.iter().any(|t| t.kind == RunKind::MovedSrc) {
        filtered_runs = runs
            .iter()
            .filter(|t| t.kind != RunKind::MovedSrc)
            .cloned()
            .collect();
        &filtered_runs
    } else {
        runs
    };

    let base_bg = |tok_kind: RunKind| -> &str {
        if is_ctx {
            if selected {
                &ctx_cursor_bg
            } else if band {
                &band_bg
            } else {
                &ctx_tint_bg
            }
        } else if tok_kind == RunKind::Del {
            &del_bg
        } else if tok_kind == RunKind::Add && (is_mod_row || has_subwords || is_expansion_add) {
            &word_bg
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
        expand_tabs(text, 4)
    } else {
        Cow::Borrowed(text)
    };
    let text = exp_text.as_ref();

    let has_styled_prefix =
        prefix.starts_with("\x1b[") || selected || band || !ctx_tint_bg.is_empty();
    let lead_bg = if has_styled_prefix { line_bg } else { "" };
    let trail_bg = if selected || band || !is_ctx || !ctx_tint_bg.is_empty() {
        line_bg
    } else {
        ""
    };

    if is_del_row {
        let bg = if is_ctx { "" } else { line_bg };
        let del_fg = s_del.fg.fg_code();
        let reset = "\x1b[39m";
        if is_full_row {
            return write!(out, "{lead_bg}{prefix}{bg}{del_fg}{text}{reset}{trail_bg}");
        }
        let disp = display_text(text, is_aux_row, kind, runs, col_offset, word_diff);
        // Slice by visual columns, not grapheme count: wide graphemes (CJK,
        // emoji) occupy two columns, so grapheme-index slicing overshoots.
        let piece = slice_by_visual_width(&disp, seg_start, seg_end.saturating_sub(seg_start));
        return write!(out, "{lead_bg}{prefix}{bg}{del_fg}{piece}{reset}{trail_bg}");
    }

    if runs.is_empty() {
        let bg: &str = if is_ctx {
            if selected {
                &ctx_cursor_bg
            } else if band {
                &band_bg
            } else {
                &ctx_tint_bg
            }
        } else {
            line_bg
        };
        if is_full_row {
            return write!(out, "{lead_bg}{prefix}{bg}{text}{trail_bg}");
        }
        let disp = display_text(text, is_aux_row, kind, runs, col_offset, word_diff);
        let piece = slice_by_visual_width(&disp, seg_start, seg_end.saturating_sub(seg_start));
        return write!(out, "{lead_bg}{prefix}{bg}{piece}{trail_bg}");
    }

    if is_full_row {
        write!(out, "{lead_bg}{prefix}")?;
        for tok in runs {
            if tok.text.is_empty() {
                continue;
            }
            let bg = base_bg(tok.kind);
            // Moved-in target text renders exactly as context: it is
            // shared, not novel. Moved-out source never reaches here
            // (filtered above) — failing loudly rather than mislabeling
            // it context if that invariant ever breaks.
            let tok_kind_str = match tok.kind {
                RunKind::Add => "add",
                RunKind::Del => "del",
                RunKind::Ctx | RunKind::MovedTgt => "ctx",
                RunKind::MovedSrc => unreachable!("moved-out runs are filtered before rendering"),
            };
            let row_kind_str = match kind {
                RowKind::Add => "add",
                RowKind::Del => "del",
                RowKind::Mod => "mod",
                RowKind::Mov => "mov",
                _ => "ctx",
            };

            if tok.kind == RunKind::Del {
                let del_fg = s_del.fg.fg_code();
                write!(out, "{bg}\x1b[9m{del_fg}{}\x1b[39m\x1b[29m", tok.text)?;
            } else if !syntax_on {
                write!(out, "{bg}{}", tok.text)?;
                write!(out, "{bg}")?;
            } else {
                write!(out, "{bg}")?;
                theme.write_token(
                    tok.color,
                    &tok.text,
                    row_kind_str,
                    Some(tok_kind_str),
                    tok.sgr,
                    out,
                );
                write!(out, "{bg}")?;
            }
        }
        write!(out, "{trail_bg}")?;
        return Ok(());
    }

    write!(out, "{lead_bg}{prefix}")?;
    // Visual columns consumed by the col_offset-skipped grapheme prefix, so
    // run positions map into displayed coordinates even with wide graphemes.
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

        // Map the run into the displayed (col_offset-shifted) coordinate
        // space: display position d corresponds to global position
        // d + skipped_visual (display_text already skipped the offset chars).
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
        // Moved-in target text renders exactly as context (see above).
        let tok_kind_str = match tok.kind {
            RunKind::Add => "add",
            RunKind::Del => "del",
            RunKind::Ctx | RunKind::MovedTgt => "ctx",
            RunKind::MovedSrc => unreachable!("moved-out runs are filtered before rendering"),
        };
        let row_kind_str = match kind {
            RowKind::Add => "add",
            RowKind::Del => "del",
            RowKind::Mod => "mod",
            RowKind::Mov => "mov",
            _ => "ctx",
        };

        if tok.kind == RunKind::Del {
            let del_fg = s_del.fg.fg_code();
            write!(out, "{bg}\x1b[9m{del_fg}{sub_piece}\x1b[39m\x1b[29m")?;
        } else {
            // Syntax off bypasses the theme: plain runs must not inherit a
            // default token color from the palette.
            if !syntax_on {
                write!(out, "{bg}{sub_piece}{bg}")?;
                continue;
            }
            write!(out, "{bg}")?;
            theme.write_token(
                tok.color,
                &sub_piece,
                row_kind_str,
                Some(tok_kind_str),
                tok.sgr,
                out,
            );
            write!(out, "{bg}")?;
        }
    }

    write!(out, "{trail_bg}")
}

/// Render a single slice/segment of a row with full ANSI styling.
#[allow(clippy::too_many_arguments)]
pub fn render_row_segment(
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    mov_side: Option<MovSide>,
    runs: &[OutputRun],
    seg_start: usize,
    seg_end: usize,
    prefix: &str,
    selected: bool,
    band: bool,
    col_offset: usize,
    word_diff: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_row_segment(
        &mut out, text, is_aux_row, kind, mov_side, runs, seg_start, seg_end, prefix, selected,
        band, col_offset, word_diff, syntax_on, theme,
    );
    out
}
