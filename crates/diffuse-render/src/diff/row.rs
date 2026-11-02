//! Single diff row line formatting and original source row rendering.

use crate::{
    ansi::{SGR_RESET, strip_ansi},
    core::wire::OutputRow,
    row::{display_text, expand_tabs, prefix_of, styled_prefix, write_row_segment},
    theme::DiffuseTheme,
    width::display_width,
};

/// Render one diff row (gutter prefix, run coloring, wrapping, padding)
/// exactly as `render_file_diff` does, appending complete lines to `out`.
pub(crate) fn render_row_line(
    out: &mut String,
    r: &OutputRow,
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
) {
    let pfx = if styled_prefixes {
        styled_prefix(
            r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, false, theme,
        )
    } else {
        let plain = prefix_of(r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs);
        format!("{plain} ")
    };

    let cont_pfx = if styled_prefixes {
        "\x1b[38;5;242m      \x1b[39m".to_string()
    } else {
        "      ".to_string()
    };

    let text = r.text();

    if width == 0 {
        let mut row_buf = String::with_capacity(text.len() + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            &r.runs,
            0,
            usize::MAX,
            &pfx,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        out.push_str(&row_buf);
        out.push_str(SGR_RESET);
        out.push('\n');
        return;
    }

    let text_cols = width.saturating_sub(6).max(1);
    let disp = display_text(&text, r.is_aux(), r.kind, &r.runs, 0, true);
    let exp = expand_tabs(&disp, 4);
    let len = display_width(&exp);
    let seg_count = if len == 0 { 1 } else { len.div_ceil(text_cols) };

    if seg_count == 1 {
        let mut row_buf = String::with_capacity(text.len() + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            &r.runs,
            0,
            usize::MAX,
            &pfx,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        let plain = strip_ansi(&row_buf);
        let exp = expand_tabs(&plain, 4);
        let vis_w = display_width(&exp);
        out.push_str(&row_buf);
        if vis_w < width {
            for _ in 0..(width - vis_w) {
                out.push(' ');
            }
        }
        out.push_str(SGR_RESET);
        out.push('\n');
        return;
    }

    for s_idx in 0..seg_count {
        let seg_pfx = if s_idx == 0 { &pfx } else { &cont_pfx };
        let seg_start = s_idx * text_cols;
        let seg_end = seg_start + text_cols;
        let mut row_buf = String::with_capacity(text_cols + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            &r.runs,
            seg_start,
            seg_end,
            seg_pfx,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        let plain = strip_ansi(&row_buf);
        let exp = expand_tabs(&plain, 4);
        let vis_w = display_width(&exp);
        out.push_str(&row_buf);
        if vis_w < width {
            for _ in 0..(width - vis_w) {
                out.push(' ');
            }
        }
        out.push_str(SGR_RESET);
        out.push('\n');
    }
}

/// Render original source rows with the standard diff row coloring for
/// their kind and language-derived token runs — the presentation used for
/// note `original_source_rows` snapshots.
pub fn render_source_rows(
    rows: &[OutputRow],
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
) -> Vec<String> {
    let mut out = String::with_capacity(rows.len() * 128);
    for r in rows {
        render_row_line(&mut out, r, theme, width, styled_prefixes);
    }
    out.trim_end_matches('\n')
        .split('\n')
        .map(str::to_string)
        .collect()
}
