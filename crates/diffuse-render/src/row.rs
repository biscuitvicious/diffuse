//! Row & Segment Renderer with subword styling, background tinting, and column
//! slicing.

use std::{borrow::Cow, fmt, fmt::Write, str};
use unicode_segmentation::UnicodeSegmentation;

use super::width::{char_width, grapheme_width};

use super::theme::{DiffuseTheme, FG_RESET};
use crate::{
    ansi::{Color, ColorExt},
    core::{
        lcs::char_slice,
        merge::RunKind,
        parse::{RowGroup, RowKind, Spans},
        wire::{MovSide, OutputRun},
    },
};
use diffuse_syntax::color::TokenColor;

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

/// Format 4-character right-aligned line numbers into a stack buffer without
/// heap allocation.
pub fn pad4_bytes(num: Option<u32>, buf: &mut [u8; 4]) -> &str {
    match num {
        None => "    ",
        Some(n) => {
            *buf = *b"    ";
            let mut val = n;
            let mut idx = 3;
            if val == 0 {
                buf[3] = b'0';
            } else {
                while val > 0 {
                    buf[idx] = b'0' + (val % 10) as u8;
                    val /= 10;
                    if idx == 0 {
                        break;
                    }
                    idx -= 1;
                }
            }
            // SAFETY: buf contains only ASCII spaces and digits.
            unsafe { str::from_utf8_unchecked(buf) }
        }
    }
}

/// Format 4-character right-aligned line numbers.
pub fn pad4(num: Option<u32>) -> String {
    let mut buf = [b' '; 4];
    pad4_bytes(num, &mut buf).to_string()
}

/// Generate plain 6-character prefix gutter (` 123 +`, ` 123 -`, ` 123 ~`, `
/// 123~ `, ` 123~+`, ` 123~-`, ` 123≈+`, ` 123≈-`).
pub fn prefix_of(
    kind: RowKind,
    mov_side: Option<crate::core::wire::MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
) -> String {
    let mut buf = [b' '; 4];
    if is_common_only_row(kind, runs) {
        return format!("{}  ", pad4_bytes(new_line, &mut buf));
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            format!("{}~ ", pad4_bytes(new_line, &mut buf))
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => format!("{}~+", pad4_bytes(new_line, &mut buf)),
            RowKind::Del => format!("{}~-", pad4_bytes(old_line, &mut buf)),
            _ => format!("{}  ", pad4_bytes(new_line.or(old_line), &mut buf)),
        },
        _ => match kind {
            RowKind::Add => format!("{} +", pad4_bytes(new_line, &mut buf)),
            RowKind::Del => format!("{} -", pad4_bytes(old_line, &mut buf)),
            RowKind::Mod => format!("{} ~", pad4_bytes(new_line, &mut buf)),
            RowKind::Mov => {
                if mov_side == Some(crate::core::wire::MovSide::Src) {
                    format!("{}≈-", pad4_bytes(old_line, &mut buf))
                } else {
                    format!("{}≈+", pad4_bytes(new_line, &mut buf))
                }
            }
            _ => format!("{}  ", pad4_bytes(new_line.or(old_line), &mut buf)),
        },
    }
}

/// Generate styled ANSI 6-character prefix gutter with color highlights and
/// theme support.
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix(
    kind: RowKind,
    mov_side: Option<crate::core::wire::MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut buf = [b' '; 4];
    if is_common_only_row(kind, runs) {
        let pad = pad4_bytes(new_line, &mut buf);
        let color = if in_hunk {
            theme.palette.ctx_gutter_in_hunk
        } else {
            theme.palette.ctx_gutter_out_hunk
        };
        let color_fg = color.fg_code();
        return format!("{color_fg}{pad}  {FG_RESET}\x1b[23m");
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            let pad = pad4_bytes(new_line, &mut buf);
            let num_fg = theme.palette.mod_gutter_num.fg_code();
            let mod_fg = theme.palette.mod_gutter_icon.fg_code();
            format!("{num_fg}{pad}{mod_fg}~ {FG_RESET}\x1b[23m")
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                // Aux numbers share the anchor's number color so the block
                // reads as one unit; the ~+ icons carry the add signal.
                let num_fg = theme.palette.mod_gutter_num.fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                let add_fg = theme.palette.add_gutter_icon.fg_code();
                format!("{num_fg}{pad}{mod_fg}~{add_fg}+{FG_RESET}\x1b[23m")
            }
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                // Aux numbers replicate the anchor number style exactly
                // (no del italic): the block reads as one unit, with the
                // ~- icons carrying the del signal.
                let num_fg = theme.palette.mod_gutter_num.fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                let del_fg = theme.palette.del_gutter_icon.fg_code();
                format!("{num_fg}{pad}{mod_fg}~{del_fg}-{FG_RESET}\x1b[23m")
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                let color = if in_hunk {
                    theme.palette.ctx_gutter_in_hunk
                } else {
                    theme.palette.ctx_gutter_out_hunk
                };
                let color_fg = color.fg_code();
                format!("{color_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
        _ => match kind {
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                let num_fg = theme.palette.del_gutter_num.italic_fg_code();
                let icon_fg = theme.palette.del_gutter_icon.fg_code();
                format!("{num_fg}{pad}\x1b[23m {icon_fg}-{FG_RESET}\x1b[23m")
            }
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                let num_fg = theme.palette.add_gutter_num.fg_code();
                let icon_fg = theme.palette.add_gutter_icon.fg_code();
                format!("{num_fg}{pad} {icon_fg}+{FG_RESET}\x1b[23m")
            }
            RowKind::Mod => {
                let pad = pad4_bytes(new_line, &mut buf);
                let num_fg = theme.palette.mod_gutter_num.fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                format!("{num_fg}{pad} {mod_fg}~{FG_RESET}\x1b[23m")
            }
            RowKind::Mov => {
                let num_fg = theme.palette.mov_gutter_num.fg_code();
                let mov_fg = theme.palette.mov_gutter_icon.fg_code();
                if mov_side == Some(crate::core::wire::MovSide::Src) {
                    let pad = pad4_bytes(old_line, &mut buf);
                    let del_fg = theme.palette.del_gutter_icon.fg_code();
                    format!("{num_fg}{pad}{mov_fg}≈{del_fg}-{FG_RESET}\x1b[23m")
                } else {
                    let pad = pad4_bytes(new_line, &mut buf);
                    let add_fg = theme.palette.add_gutter_icon.fg_code();
                    format!("{num_fg}{pad}{mov_fg}≈{add_fg}+{FG_RESET}\x1b[23m")
                }
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                let color = if in_hunk {
                    theme.palette.ctx_gutter_in_hunk
                } else {
                    theme.palette.ctx_gutter_out_hunk
                };
                let color_fg = color.fg_code();
                format!("{color_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
    }
}

/// Extract display text of a row considering word diff and column offset.
pub fn display_text(
    text: &str,
    is_aux_row: bool,
    kind: RowKind,
    runs: &[OutputRun],
    col_offset: usize,
    word_diff: bool,
) -> String {
    let use_merged = (kind == RowKind::Mod || (kind == RowKind::Add && is_aux_row))
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
    // row (distinct from the cursor's per-style lift).
    let band_bg = theme.palette.band_bg.bg_code();
    let ctx_cursor_bg = theme.palette.ctx_cursor_bg.bg_code();
    let ctx_tint_bg = match theme.palette.ctx_tint_bg {
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
            } else {
                let color = if syntax_on {
                    tok.color
                } else {
                    TokenColor::Default
                };
                let sgr = if syntax_on { tok.sgr } else { None };
                write!(out, "{bg}")?;
                theme.write_token(color, &tok.text, row_kind_str, Some(tok_kind_str), sgr, out);
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
            // syntax off downgrades every run to plain text (no fg, no sgr)
            let color = if syntax_on {
                tok.color
            } else {
                TokenColor::Default
            };
            let sgr = if syntax_on { tok.sgr } else { None };
            write!(out, "{bg}")?;
            theme.write_token(
                color,
                &sub_piece,
                row_kind_str,
                Some(tok_kind_str),
                sgr,
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

/// Slices text by visual terminal column width rather than raw bytes or
/// character counts.
pub fn slice_by_visual_width(text: &str, start_col: usize, max_cols: usize) -> String {
    use super::width::grapheme_width;
    use unicode_segmentation::UnicodeSegmentation;
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

/// Write a row in the raw unified-diff representation with theme support
/// directly to a buffer.
#[allow(clippy::too_many_arguments)]
pub fn write_raw_row<W: Write>(
    out: &mut W,
    kind: RowKind,
    prefix: &str,
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let (fg_color, sub_bg_color, sub_fg_color) = match kind {
        RowKind::Add => (
            theme.palette.add_gutter_icon,
            theme.palette.add_fill_bg,
            theme.palette.add_gutter_num,
        ),
        RowKind::Del => (
            theme.palette.del_gutter_icon,
            theme.palette.del_cursor_bg,
            theme.palette.del_gutter_num,
        ),
        _ => (
            theme.palette.ctx_gutter_in_hunk,
            theme.palette.ctx_cursor_bg,
            theme.palette.ctx_gutter_out_hunk,
        ),
    };
    let fg = fg_color.fg_code();
    let sub_bg = sub_bg_color.bg_code();
    let sub_fg = sub_fg_color.fg_code();

    let bg_color = if selected {
        theme.palette.ctx_cursor_bg
    } else if band {
        theme.palette.band_bg
    } else {
        theme.palette.ctx_tint_bg
    };
    let bg = match bg_color {
        Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let reset_bg = if bg.is_empty() { "\x1b[49m" } else { &bg };

    let trailing_len = if highlight_trailing {
        text.len() - text.trim_end_matches(|c: char| c.is_whitespace()).len()
    } else {
        0
    };

    write!(out, "{bg}{prefix}{fg}")?;
    if trailing_len > 0 {
        let (body, trailing) = text.split_at(text.len() - trailing_len);
        out.write_str(body)?;
        if kind == RowKind::Del {
            write!(
                out,
                "{sub_bg}\x1b[9m{sub_fg}{trailing}\x1b[29m{reset_bg}{fg}"
            )?;
        } else {
            write!(out, "{sub_bg}{sub_fg}{trailing}{reset_bg}{fg}")?;
        }
    } else {
        out.write_str(text)?;
    }

    if selected || band {
        out.write_str(&bg)?;
    } else {
        out.write_str("\x1b[49m")?;
    }
    out.write_str("\x1b[39m\x1b[22m")
}

/// Render a row in the raw unified-diff representation with theme support:
/// the 6-character line-number gutter plus the line text in flat
/// green/red/muted text color on a default black background (or cursor/band
/// background when selected).
pub fn render_raw_row(
    kind: RowKind,
    prefix: &str,
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_raw_row(
        &mut out,
        kind,
        prefix,
        text,
        selected,
        band,
        highlight_trailing,
        theme,
    );
    out
}

#[cfg(test)]
mod tests;
