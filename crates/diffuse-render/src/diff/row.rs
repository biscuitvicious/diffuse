//! Single diff row line formatting and original source row rendering.

use crate::{
    ansi::{SGR_RESET, strip_ansi},
    core::{ModGlyph, ModSide, wire::OutputRow},
    row::{
        continuation_prefix_plain_with_mod_side_and_width,
        continuation_prefix_spans_with_mod_side_and_width, display_text, expand_tabs, gutter_width,
        prefix_of_with_mod_side_and_width, styled_prefix_spans_with_width, write_row_segment,
    },
    span::StyledSpan,
    theme::DiffuseTheme,
    width::display_width,
};

/// Render one diff row (gutter prefix, run coloring, wrapping, padding)
/// exactly as `render_file_diff` does, appending complete lines to `out`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_row_line(
    out: &mut String,
    r: &OutputRow,
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    line_num_width: usize,
) {
    let (pfx, cont_pfx): (Vec<StyledSpan>, Vec<StyledSpan>) = if styled_prefixes {
        (
            styled_prefix_spans_with_width(
                r.kind,
                r.mov_side,
                mod_side,
                mod_glyph,
                r.group,
                r.new_line,
                r.old_line,
                &r.runs,
                false,
                None,
                theme,
                line_num_width,
            ),
            continuation_prefix_spans_with_mod_side_and_width(
                mod_side,
                mod_glyph,
                theme,
                None,
                line_num_width,
            ),
        )
    } else {
        let plain = prefix_of_with_mod_side_and_width(
            r.kind,
            r.mov_side,
            mod_side,
            mod_glyph,
            r.group,
            r.new_line,
            r.old_line,
            &r.runs,
            line_num_width,
        );
        (
            vec![StyledSpan::plain(format!("{plain} "))],
            vec![StyledSpan::plain(
                continuation_prefix_plain_with_mod_side_and_width(
                    mod_side,
                    mod_glyph,
                    line_num_width,
                ),
            )],
        )
    };

    let text = match mod_side {
        Some(ModSide::Del) => r.old_text(),
        Some(ModSide::Add) => r.new_text(),
        None => r.text(),
    };

    if width == 0 {
        let mut row_buf = String::with_capacity(text.len() + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            mod_side,
            &r.runs,
            0,
            usize::MAX,
            &pfx,
            false,
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

    let gutter_w = gutter_width(line_num_width);
    let text_cols = width.saturating_sub(gutter_w).max(1);
    let disp = if mod_side.is_some() {
        text.clone()
    } else {
        display_text(&text, r.is_aux(), r.kind, &r.runs, 0, true)
    };
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
            mod_side,
            &r.runs,
            0,
            usize::MAX,
            &pfx,
            false,
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
        let (seg_pfx, is_cont) = if s_idx == 0 {
            (&pfx, false)
        } else {
            (&cont_pfx, true)
        };
        let seg_start = s_idx * text_cols;
        let seg_end = seg_start + text_cols;
        let mut row_buf = String::with_capacity(text_cols + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            mod_side,
            &r.runs,
            seg_start,
            seg_end,
            seg_pfx,
            is_cont,
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
        render_row_line(
            &mut out,
            r,
            theme,
            width,
            styled_prefixes,
            None,
            None,
            crate::row::DEFAULT_PAGER_LINE_NUM_WIDTH,
        );
    }
    out.trim_end_matches('\n')
        .split('\n')
        .map(str::to_string)
        .collect()
}
