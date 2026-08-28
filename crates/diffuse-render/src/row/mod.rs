//! Row & Segment Renderer with subword styling, background tinting, and column
//! slicing.

use unicode_segmentation::UnicodeSegmentation;

use super::theme::{DiffuseTheme, S_ADD, S_DEL, S_MOD, S_MOV};
use crate::core::{
    lcs::char_slice,
    merge::RunKind,
    parse::{RowKind, Spans},
    wire::OutputRun,
};

/// Check whether a row consists solely of common/context runs.
pub fn is_common_only_row(kind: RowKind, runs: &[OutputRun]) -> bool {
    kind == RowKind::Add
        && !runs.is_empty()
        && runs.iter().all(|x| {
            let k = x.kind;
            k == RunKind::Common || k == RunKind::Ctx
        })
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
            unsafe { std::str::from_utf8_unchecked(buf) }
        }
    }
}

/// Format 4-character right-aligned line numbers.
pub fn pad4(num: Option<u32>) -> String {
    let mut buf = [b' '; 4];
    pad4_bytes(num, &mut buf).to_string()
}

/// Generate plain 6-character prefix gutter (` 123 + `).
pub fn prefix_of(
    kind: RowKind,
    new_line: Option<u32>,
    old_line: Option<u32>,
    moved: bool,
    runs: &[OutputRun],
) -> String {
    let mut buf = [b' '; 4];
    if is_common_only_row(kind, runs) {
        return format!("{}  ", pad4_bytes(new_line, &mut buf));
    }
    match kind {
        RowKind::Add => {
            let icon = if moved { S_MOV.icon } else { S_ADD.icon };
            format!("{} {}", pad4_bytes(new_line, &mut buf), icon)
        }
        RowKind::Del => {
            let icon = if moved { S_MOV.icon } else { S_DEL.icon };
            format!("{} {}", pad4_bytes(old_line, &mut buf), icon)
        }
        RowKind::Mod => format!("{} {}", pad4_bytes(new_line, &mut buf), S_MOD.icon),
        _ => format!("{}  ", pad4_bytes(new_line.or(old_line), &mut buf)),
    }
}

/// Generate styled ANSI 6-character prefix gutter with color highlights.
pub fn styled_prefix(
    kind: RowKind,
    new_line: Option<u32>,
    old_line: Option<u32>,
    moved: bool,
    runs: &[OutputRun],
    in_hunk: bool,
) -> String {
    let fg_reset = "\x1b[39m\x1b[23m";
    let mut buf = [b' '; 4];
    if is_common_only_row(kind, runs) {
        let pad = pad4_bytes(new_line, &mut buf);
        let color = if in_hunk { "38;5;252" } else { "38;5;242" };
        return format!("\x1b[{color}m{pad}  {fg_reset}");
    }
    match kind {
        RowKind::Del => {
            let pad = pad4_bytes(old_line, &mut buf);
            let icon = if moved {
                format!("\x1b[38;5;117m{}", S_MOV.icon)
            } else {
                format!("\x1b[38;5;203m{}", S_DEL.icon)
            };
            format!("\x1b[3;38;5;174m{pad}\x1b[23m {icon}{fg_reset}")
        }
        RowKind::Add => {
            let pad = pad4_bytes(new_line, &mut buf);
            let icon = if moved {
                format!("\x1b[38;5;117m{}", S_MOV.icon)
            } else {
                format!("\x1b[38;5;120m{}", S_ADD.icon)
            };
            format!("\x1b[38;5;151m{pad} {icon}{fg_reset}")
        }
        RowKind::Mod => {
            let pad = pad4_bytes(new_line, &mut buf);
            format!("\x1b[38;5;253m{pad} \x1b[38;5;214m{}{fg_reset}", S_MOD.icon)
        }
        _ => {
            if moved {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                format!("\x1b[38;5;147m{pad} \x1b[38;5;117m{}{fg_reset}", S_MOV.icon)
            } else {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                let color = if in_hunk { "38;5;252" } else { "38;5;242" };
                format!("\x1b[{color}m{pad}  {fg_reset}")
            }
        }
    }
}

/// Extract display text of a row considering word diff and column offset.
pub fn display_text(
    text: &str,
    del_text: Option<&str>,
    kind: RowKind,
    moved: bool,
    runs: &[OutputRun],
    col_offset: usize,
    word_diff: bool,
) -> String {
    let use_merged = (kind == RowKind::Mod || moved || kind == RowKind::Add)
        && del_text.is_some()
        && word_diff
        && !runs.is_empty();

    let full_text = if use_merged {
        runs.iter().map(|r| r.text.as_ref()).collect::<String>()
    } else {
        text.to_string()
    };

    if col_offset > 0 {
        full_text.graphemes(true).skip(col_offset).collect()
    } else {
        full_text
    }
}

/// Render a single slice/segment of a row with full ANSI styling.
#[allow(clippy::too_many_arguments)]
pub fn render_row_segment(
    text: &str,
    del_text: Option<&str>,
    spans: &Spans,
    kind: RowKind,
    moved: bool,
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
    let is_ctx = matches!(kind, RowKind::Ctx) || is_common_only_row(kind, runs);
    let is_del_row = kind == RowKind::Del;
    let is_mod_row = kind == RowKind::Mod;
    let is_pure_del_mod = is_mod_row && is_pure_deletion_mod(spans, text);

    // The visual-selection band: one neutral shade applied across the whole
    // row (distinct from the cursor's per-style lift).
    let band_bg = "\x1b[48;5;240m";

    let line_bg: &str = if is_ctx {
        if selected {
            "\x1b[48;5;237m"
        } else if band {
            band_bg
        } else {
            ""
        }
    } else if moved {
        if selected {
            S_MOV.cursor_bg()
        } else if band {
            band_bg
        } else {
            S_MOV.tint_bg()
        }
    } else if is_mod_row {
        if selected {
            S_MOD.cursor_bg()
        } else if band {
            band_bg
        } else {
            S_MOD.tint_bg()
        }
    } else if is_del_row {
        if selected {
            S_DEL.cursor_bg()
        } else if band {
            band_bg
        } else {
            S_DEL.tint_bg()
        }
    } else if selected {
        S_ADD.cursor_bg()
    } else if band {
        band_bg
    } else {
        S_ADD.tint_bg()
    };

    let word_bg = if selected {
        S_MOD.fill_cursor_bg()
    } else {
        S_MOD.fill_bg()
    };
    let del_bg = S_DEL.tint_bg();

    let has_subwords =
        runs.iter().any(|t| t.kind == RunKind::Add) && runs.iter().any(|t| t.kind != RunKind::Add);
    let is_expansion_add = kind == RowKind::Add && del_text.is_some();

    let base_bg = |tok_kind: RunKind| -> &str {
        if is_ctx {
            if selected {
                "\x1b[48;5;237m"
            } else if band {
                band_bg
            } else {
                ""
            }
        } else if tok_kind == RunKind::Del {
            del_bg
        } else if tok_kind == RunKind::Add
            && (is_mod_row || has_subwords || is_expansion_add)
            && !is_pure_del_mod
        {
            word_bg
        } else {
            line_bg
        }
    };

    let is_full_row = seg_start == 0 && seg_end == usize::MAX && col_offset == 0;

    let has_styled_prefix = prefix.starts_with("\x1b[") || selected || band;
    let lead_bg = if has_styled_prefix { line_bg } else { "" };
    let trail_bg = if selected || band || !is_ctx {
        line_bg
    } else {
        ""
    };

    if is_del_row {
        let bg = if is_ctx { "" } else { line_bg };
        let del_fg = DiffuseTheme::fallback_named_color(S_DEL.fg).unwrap_or("");
        let reset = "\x1b[39m";
        let piece = if is_full_row {
            text
        } else {
            let disp = display_text(text, del_text, kind, moved, runs, col_offset, word_diff);
            let slice_graphemes: Vec<&str> = disp.graphemes(true).collect();
            let total_len = slice_graphemes.len();
            let s_idx = seg_start.min(total_len);
            let e_idx = seg_end.min(total_len);
            &slice_graphemes[s_idx..e_idx].concat()
        };
        return format!("{lead_bg}{prefix}{bg}{del_fg}{piece}{reset}{trail_bg}");
    }

    if runs.is_empty() {
        let bg = if is_ctx {
            if selected {
                "\x1b[48;5;237m"
            } else if band {
                band_bg
            } else {
                ""
            }
        } else {
            line_bg
        };
        let piece = if is_full_row {
            text
        } else {
            let disp = display_text(text, del_text, kind, moved, runs, col_offset, word_diff);
            let slice_graphemes: Vec<&str> = disp.graphemes(true).collect();
            let total_len = slice_graphemes.len();
            let s_idx = seg_start.min(total_len);
            let e_idx = seg_end.min(total_len);
            &slice_graphemes[s_idx..e_idx].concat()
        };
        return format!("{lead_bg}{prefix}{bg}{piece}{trail_bg}");
    }

    if is_full_row {
        let mut out = String::new();
        out.push_str(lead_bg);
        out.push_str(prefix);
        for tok in runs {
            if tok.text.is_empty() {
                continue;
            }
            let bg = base_bg(tok.kind);
            let tok_kind_str = match tok.kind {
                RunKind::Add => "add",
                RunKind::Del => "del",
                _ => "ctx",
            };
            let row_kind_str = match kind {
                RowKind::Add => "add",
                RowKind::Del => "del",
                RowKind::Mod => "mod",
                _ => "ctx",
            };

            if tok.kind == RunKind::Del {
                let del_fg = DiffuseTheme::fallback_named_color(S_DEL.fg).unwrap_or("");
                let reset = "\x1b[39m";
                out.push_str(bg);
                out.push_str("\x1b[9m");
                out.push_str(del_fg);
                out.push_str(&tok.text);
                out.push_str(reset);
                out.push_str("\x1b[29m");
            } else {
                let color = if syntax_on {
                    tok.color.as_deref().unwrap_or("text")
                } else {
                    "text"
                };
                let sgr = if syntax_on { tok.sgr } else { None };
                out.push_str(bg);
                theme.write_token(
                    color,
                    &tok.text,
                    row_kind_str,
                    Some(tok_kind_str),
                    sgr,
                    &mut out,
                );
                out.push_str(bg);
            }
        }
        out.push_str(trail_bg);
        return out;
    }

    let _disp = display_text(text, del_text, kind, moved, runs, col_offset, word_diff);
    let mut out = String::new();
    let mut pos = 0usize;
    for tok in runs {
        let tok_len = tok.text.graphemes(true).count();
        let tok_start = pos;
        let tok_end = pos + tok_len;
        pos = tok_end;

        // Map the run into the displayed (col_offset-shifted) coordinate
        // space: display position d corresponds to global position
        // d + col_offset (display_text already skipped the offset chars).
        let d_start = tok_start.saturating_sub(col_offset);
        let d_end = tok_end.saturating_sub(col_offset);
        if d_end <= seg_start || d_start >= seg_end {
            continue;
        }

        let s = seg_start.max(d_start);
        let e = seg_end.min(d_end);
        let tok_graphemes: Vec<&str> = tok.text.graphemes(true).collect();
        let idx_s = (s + col_offset).saturating_sub(tok_start);
        let idx_e = (e + col_offset).saturating_sub(tok_start);
        let sub_piece: String = tok_graphemes[idx_s..idx_e].concat();
        if sub_piece.is_empty() {
            continue;
        }

        let bg = base_bg(tok.kind);
        let tok_kind_str = match tok.kind {
            RunKind::Add => "add",
            RunKind::Del => "del",
            _ => "ctx",
        };
        let row_kind_str = match kind {
            RowKind::Add => "add",
            RowKind::Del => "del",
            RowKind::Mod => "mod",
            _ => "ctx",
        };

        if tok.kind == RunKind::Del {
            let del_fg = DiffuseTheme::fallback_named_color(S_DEL.fg).unwrap_or("");
            let reset = "\x1b[39m";
            out.push_str(bg);
            out.push_str("\x1b[9m");
            out.push_str(del_fg);
            out.push_str(&sub_piece);
            out.push_str(reset);
            out.push_str("\x1b[29m");
        } else {
            // syntax off downgrades every run to plain text (no fg, no sgr)
            let color = if syntax_on {
                tok.color.as_deref().unwrap_or("text")
            } else {
                "text"
            };
            let sgr = if syntax_on { tok.sgr } else { None };
            out.push_str(bg);
            theme.write_token(
                color,
                &sub_piece,
                row_kind_str,
                Some(tok_kind_str),
                sgr,
                &mut out,
            );
            out.push_str(bg);
        }
    }

    format!("{lead_bg}{prefix}{out}{trail_bg}")
}

/// Slices text by visual terminal column width rather than raw bytes or
/// character counts.
pub fn slice_by_visual_width(text: &str, start_col: usize, max_cols: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let mut current_col = 0;
    let mut out = String::new();
    let mut accumulated = 0;
    for g in text.graphemes(true) {
        let gw = g.width();
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

/// Render a row in the raw unified-diff representation: the 6-character
/// line-number gutter plus the line text in flat green/red/muted text color
/// on a default black background (or cursor/band background when selected).
pub fn render_raw_row(
    kind: RowKind,
    prefix: &str,
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
) -> String {
    let (fg, sub_bg, sub_fg) = match kind {
        RowKind::Add => ("\x1b[38;5;120m", "\x1b[48;5;28m", "\x1b[38;5;151m"),
        RowKind::Del => ("\x1b[38;5;203m", "\x1b[48;5;88m", "\x1b[38;5;167m"),
        _ => ("\x1b[38;5;255m", "\x1b[48;5;238m", "\x1b[38;5;244m"),
    };

    let bg = if selected {
        "\x1b[48;5;237m"
    } else if band {
        "\x1b[48;5;240m"
    } else {
        ""
    };
    let reset_bg = if bg.is_empty() { "\x1b[49m" } else { bg };

    let trailing_len = if highlight_trailing {
        text.len() - text.trim_end_matches(|c: char| c.is_whitespace()).len()
    } else {
        0
    };

    let mut out = format!("{bg}{prefix}{fg}");
    if trailing_len > 0 {
        let (body, trailing) = text.split_at(text.len() - trailing_len);
        out.push_str(body);
        if kind == RowKind::Del {
            out.push_str(&format!(
                "{sub_bg}\x1b[9m{sub_fg}{trailing}\x1b[29m{reset_bg}{fg}"
            ));
        } else {
            out.push_str(&format!("{sub_bg}{sub_fg}{trailing}{reset_bg}{fg}"));
        }
    } else {
        out.push_str(text);
    }

    if selected || band {
        out.push_str(bg);
    } else {
        out.push_str("\x1b[49m");
    }
    out.push_str("\x1b[39m\x1b[22m");
    out
}

#[cfg(test)]
mod tests;
