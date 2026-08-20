//! Row & Segment Renderer with subword styling, background tinting, and column
//! slicing.

use super::theme::{DiffuseTheme, S_ADD, S_DEL, S_MOD, S_MOV};
use crate::{
    bin_pipeline::OutputRun,
    parse::{RowKind, Spans},
};
use unicode_segmentation::UnicodeSegmentation;

/// Check whether a row consists solely of common/context runs.
pub fn is_common_only_row(kind: RowKind, runs: &[OutputRun]) -> bool {
    kind == RowKind::Add
        && !runs.is_empty()
        && runs.iter().all(|x| {
            let k = x.kind;
            k == crate::merge::RunKind::Common || k == crate::merge::RunKind::Ctx
        })
}

/// Check if a mod row is a pure deletion (e.g. whitespace-only additions).
pub fn is_pure_deletion_mod(spans: &Spans, text: &str) -> bool {
    spans.add.iter().all(|&(s, e)| {
        if s <= e {
            crate::lcs::char_slice(text, s, e).trim().is_empty()
        } else {
            true
        }
    })
}

/// Format 4-character right-aligned line numbers.
pub fn pad4(num: Option<u32>) -> String {
    match num {
        Some(n) => format!("{n:>4}"),
        None => "    ".to_string(),
    }
}

/// Generate plain 6-character prefix gutter (` 123 + `).
pub fn prefix_of(
    kind: RowKind,
    new_line: Option<u32>,
    old_line: Option<u32>,
    moved: bool,
    runs: &[OutputRun],
) -> String {
    if is_common_only_row(kind, runs) {
        return format!("{}  ", pad4(new_line));
    }
    match kind {
        RowKind::Add => {
            let icon = if moved { S_MOV.icon } else { S_ADD.icon };
            format!("{} {}", pad4(new_line), icon)
        }
        RowKind::Del => {
            let icon = if moved { S_MOV.icon } else { S_DEL.icon };
            format!("{} {}", pad4(old_line), icon)
        }
        RowKind::Mod => format!("{} {}", pad4(new_line), S_MOD.icon),
        _ => format!("{}  ", pad4(new_line.or(old_line))),
    }
}

/// Generate styled ANSI 6-character prefix gutter with color highlights.
pub fn styled_prefix(
    kind: RowKind,
    new_line: Option<u32>,
    old_line: Option<u32>,
    moved: bool,
    runs: &[OutputRun],
) -> String {
    let fg_reset = "\x1b[39m\x1b[23m";
    if is_common_only_row(kind, runs) {
        let pad = pad4(new_line);
        return format!("\x1b[38;5;242m{pad}  {fg_reset}");
    }
    match kind {
        RowKind::Del => {
            let pad = pad4(old_line);
            let icon = if moved {
                format!("\x1b[38;5;117m{}", S_MOV.icon)
            } else {
                format!("\x1b[38;5;203m{}", S_DEL.icon)
            };
            format!("\x1b[3;38;5;174m{pad}\x1b[23m {icon}{fg_reset}")
        }
        RowKind::Add => {
            let pad = pad4(new_line);
            let icon = if moved {
                format!("\x1b[38;5;117m{}", S_MOV.icon)
            } else {
                format!("\x1b[38;5;120m{}", S_ADD.icon)
            };
            format!("\x1b[38;5;151m{pad} {icon}{fg_reset}")
        }
        RowKind::Mod => {
            let pad = pad4(new_line);
            format!("\x1b[38;5;253m{pad} \x1b[38;5;214m{}{fg_reset}", S_MOD.icon)
        }
        _ => {
            if moved {
                let pad = pad4(new_line.or(old_line));
                format!("\x1b[38;5;147m{pad} \x1b[38;5;117m{}{fg_reset}", S_MOV.icon)
            } else {
                let pad = pad4(new_line.or(old_line));
                format!("\x1b[38;5;242m{pad}  {fg_reset}")
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
        runs.iter().map(|r| r.text.as_str()).collect::<String>()
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
    let is_ctx = matches!(kind, RowKind::Ctx | RowKind::Sep) || is_common_only_row(kind, runs);
    let is_del_row = kind == RowKind::Del;
    let is_mod_row = kind == RowKind::Mod;
    let is_pure_del_mod = is_mod_row && is_pure_deletion_mod(spans, text);

    // The visual-selection band: one neutral shade applied across the whole
    // row (distinct from the cursor's per-style lift).
    let band_bg = "\x1b[48;5;240m";

    let line_bg = if is_ctx {
        if selected {
            "\x1b[48;5;237m".to_string()
        } else if band {
            band_bg.to_string()
        } else {
            String::new()
        }
    } else if moved {
        if selected {
            S_MOV.cursor_bg()
        } else if band {
            band_bg.to_string()
        } else {
            S_MOV.tint_bg()
        }
    } else if is_mod_row {
        if selected {
            S_MOD.cursor_bg()
        } else if band {
            band_bg.to_string()
        } else {
            S_MOD.tint_bg()
        }
    } else if is_del_row {
        if selected {
            S_DEL.cursor_bg()
        } else if band {
            band_bg.to_string()
        } else {
            S_DEL.tint_bg()
        }
    } else {
        if selected {
            S_ADD.cursor_bg()
        } else if band {
            band_bg.to_string()
        } else {
            S_ADD.tint_bg()
        }
    };

    let word_bg = if selected {
        S_MOD.fill_cursor_bg()
    } else {
        S_MOD.fill_bg()
    };
    let del_bg = S_DEL.tint_bg();

    let has_subwords = runs.iter().any(|t| t.kind == crate::merge::RunKind::Add)
        && runs.iter().any(|t| t.kind != crate::merge::RunKind::Add);
    let is_expansion_add = kind == RowKind::Add && del_text.is_some();

    let base_bg = |tok_kind: crate::merge::RunKind| -> &str {
        if is_ctx && !selected {
            ""
        } else if tok_kind == crate::merge::RunKind::Del {
            &del_bg
        } else if tok_kind == crate::merge::RunKind::Add
            && (is_mod_row || has_subwords || is_expansion_add)
            && !is_pure_del_mod
        {
            &word_bg
        } else {
            &line_bg
        }
    };

    let disp = display_text(text, del_text, kind, moved, runs, col_offset, word_diff);
    let slice_graphemes: Vec<&str> = disp.graphemes(true).collect();
    let total_len = slice_graphemes.len();
    let s_idx = seg_start.min(total_len);
    let e_idx = seg_end.min(total_len);
    let piece: String = slice_graphemes[s_idx..e_idx].concat();

    if is_del_row {
        let bg = if is_ctx { "" } else { &line_bg };
        let sel_lead = if selected { &line_bg } else { "" };
        let del_fg = DiffuseTheme::fallback_named_color(S_DEL.fg).unwrap_or("");
        let reset = "\x1b[39m";
        return format!("{sel_lead}{prefix}{bg}{del_fg}{piece}{reset}{line_bg}");
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
            &line_bg
        };
        let sel_lead = if selected { &line_bg } else { "" };
        return format!("{sel_lead}{prefix}{bg}{piece}{bg}");
    }

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
            crate::merge::RunKind::Add => "add",
            crate::merge::RunKind::Del => "del",
            _ => "ctx",
        };
        let row_kind_str = match kind {
            RowKind::Add => "add",
            RowKind::Del => "del",
            RowKind::Mod => "mod",
            _ => "ctx",
        };

        if tok.kind == crate::merge::RunKind::Del {
            let del_fg = DiffuseTheme::fallback_named_color(S_DEL.fg).unwrap_or("");
            let reset = "\x1b[39m";
            out.push_str(&format!("{bg}\x1b[9m{del_fg}{sub_piece}{reset}\x1b[29m"));
        } else {
            // syntax off downgrades every run to plain text (no fg, no sgr)
            let color = if syntax_on {
                tok.color.as_deref().unwrap_or("text")
            } else {
                "text"
            };
            let sgr = if syntax_on { tok.sgr } else { None };
            let styled_tok =
                theme.render_token(color, &sub_piece, row_kind_str, Some(tok_kind_str), sgr);
            out.push_str(&format!("{bg}{styled_tok}{bg}"));
        }
    }

    let trail_bg = if selected || !is_ctx { &line_bg } else { "" };
    let lead_bg = if selected && !is_ctx { &line_bg } else { "" };
    format!("{lead_bg}{prefix}{out}{trail_bg}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bin_pipeline::OutputRun, merge::RunKind};

    #[test]
    fn test_prefix_gutters() {
        let r_add = prefix_of(RowKind::Add, Some(42), None, false, &[]);
        assert_eq!(r_add, "  42 +");

        let r_del = prefix_of(RowKind::Del, None, Some(10), false, &[]);
        assert_eq!(r_del, "  10 -");

        let r_mod = prefix_of(RowKind::Mod, Some(55), Some(54), false, &[]);
        assert_eq!(r_mod, "  55 ~");
    }

    #[test]
    fn test_render_addition_row_with_subword_styling() {
        let theme = DiffuseTheme::MAGIC;
        let runs = vec![
            OutputRun {
                kind: RunKind::Common,
                color: Some("syntaxKeyword".into()),
                sgr: None,
                text: "pub fn ".into(),
            },
            OutputRun {
                kind: RunKind::Add,
                color: Some("syntaxFunction".into()),
                sgr: None,
                text: "render".into(),
            },
        ];
        let prefix = "  12 + ";
        let rendered = render_row_segment(
            "pub fn render",
            None,
            &Spans::default(),
            RowKind::Add,
            false,
            &runs,
            0,
            13,
            prefix,
            false,
            false,
            0,
            true,
            true,
            &theme,
        );
        assert!(rendered.contains("pub fn "));
        assert!(rendered.contains("render"));
        assert!(rendered.starts_with(prefix));
    }

    #[test]
    fn test_grapheme_clusters_never_split() {
        let theme = DiffuseTheme::MAGIC;
        // "e" + combining acute accent = ONE grapheme cluster; "x" the second.
        let text = "e\u{301}x";
        let runs = vec![OutputRun {
            kind: RunKind::Common,
            color: None,
            sgr: None,
            text: text.into(),
        }];
        let prefix = "   1  ";
        // seg [0,1) in grapheme units must keep the whole cluster, not split it.
        let seg = render_row_segment(
            text,
            None,
            &Spans::default(),
            RowKind::Ctx,
            false,
            &runs,
            0,
            1,
            prefix,
            false,
            false,
            0,
            true,
            true,
            &theme,
        );
        assert!(
            seg.contains("e\u{301}"),
            "a grapheme cluster must survive slicing: {seg:?}"
        );
        assert!(!seg.contains('x'));

        // col_offset=1 skips one grapheme (the whole cluster), landing on "x".
        let off = render_row_segment(
            text,
            None,
            &Spans::default(),
            RowKind::Ctx,
            false,
            &runs,
            0,
            1,
            prefix,
            false,
            false,
            1,
            true,
            true,
            &theme,
        );
        assert!(
            off.contains('x'),
            "offset must land on the second grapheme: {off:?}"
        );
        assert!(!off.contains("e\u{301}"));
    }

    #[test]
    fn test_syntax_on_toggle_disables_token_colors() {
        let theme = DiffuseTheme::MAGIC;
        let runs = vec![OutputRun {
            kind: RunKind::Common,
            color: Some("syntaxKeyword".into()),
            sgr: Some(1),
            text: "fn".into(),
        }];
        let prefix = "   1  ";
        let on = render_row_segment(
            "fn",
            None,
            &Spans::default(),
            RowKind::Ctx,
            false,
            &runs,
            0,
            2,
            prefix,
            false,
            false,
            0,
            true,
            true,
            &theme,
        );
        assert!(
            on.contains("\x1b[38;5;"),
            "syntax on must color the token: {on:?}"
        );
        let off = render_row_segment(
            "fn",
            None,
            &Spans::default(),
            RowKind::Ctx,
            false,
            &runs,
            0,
            2,
            prefix,
            false,
            false,
            0,
            true,
            false,
            &theme,
        );
        assert!(
            !off.contains("\x1b[38;5;"),
            "syntax off must leave the token plain: {off:?}"
        );
    }

    #[test]
    fn test_col_offset_applies_to_token_runs() {
        let theme = DiffuseTheme::MAGIC;
        let runs = vec![
            OutputRun {
                kind: RunKind::Common,
                color: Some("syntaxKeyword".into()),
                sgr: None,
                text: "pub fn ".into(),
            },
            OutputRun {
                kind: RunKind::Add,
                color: Some("syntaxFunction".into()),
                sgr: None,
                text: "render".into(),
            },
        ];
        let prefix = "  12 + ";
        let rendered = render_row_segment(
            "pub fn render",
            None,
            &Spans::default(),
            RowKind::Add,
            false,
            &runs,
            0,
            13,
            prefix,
            false,
            false,
            4, // col_offset skips "pub "
            true,
            true,
            &theme,
        );
        assert!(
            !rendered.contains("pub"),
            "horizontal scroll must skip the first 4 chars, got: {rendered}"
        );
        assert!(rendered.contains("fn "));
        assert!(rendered.contains("render"));
    }

    #[test]
    fn test_is_pure_deletion_mod_multibyte_unicode() {
        let text = "abc—def";
        let spans = Spans {
            del: vec![],
            add: vec![(0, 4)],
        };
        // Should not panic on multibyte character boundary
        let res = is_pure_deletion_mod(&spans, text);
        assert!(!res);
    }
}
