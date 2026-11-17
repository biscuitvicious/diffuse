//! Single diff row line formatting and original source row rendering.

use crate::{
    ansi::{SGR_RESET, strip_ansi},
    core::{ModGlyph, ModSide, wire::OutputRow},
    row::{
        continuation_prefix_plain_with_mod_side_and_width,
        continuation_prefix_spans_with_mod_side_and_width, display_text, expand_tabs, gutter_width,
        prefix_of_with_mod_side_and_width, render_side_by_side_row_segment,
        side_by_side_row_seg_count, split_pane_layout, styled_prefix_spans_with_width,
        styled_split_left_continuation_prefix_spans, styled_split_left_prefix_spans,
        styled_split_right_continuation_prefix_spans, styled_split_right_prefix_spans,
        write_row_segment,
    },
    span::StyledSpan,
    theme::DiffuseTheme,
    width::display_width,
};

fn make_prefix(
    r: &OutputRow,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    styled_prefixes: bool,
    theme: &DiffuseTheme,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    if styled_prefixes {
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
        vec![StyledSpan::plain(format!("{plain} "))]
    }
}

fn make_cont_prefix(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    styled_prefixes: bool,
    theme: &DiffuseTheme,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    if styled_prefixes {
        continuation_prefix_spans_with_mod_side_and_width(
            mod_side,
            mod_glyph,
            theme,
            None,
            line_num_width,
        )
    } else {
        vec![StyledSpan::plain(
            continuation_prefix_plain_with_mod_side_and_width(mod_side, mod_glyph, line_num_width),
        )]
    }
}

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
    let text = match mod_side {
        Some(ModSide::Del) => r.old_text(),
        Some(ModSide::Add) => r.new_text(),
        None => r.text(),
    };

    if width == 0 {
        let pfx = make_prefix(
            r,
            mod_side,
            mod_glyph,
            styled_prefixes,
            theme,
            line_num_width,
        );
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
        let pfx = make_prefix(
            r,
            mod_side,
            mod_glyph,
            styled_prefixes,
            theme,
            line_num_width,
        );
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

    let (pfx, cont_mid_pfx, cont_bottom_pfx) = if mod_glyph == Some(ModGlyph::Bottom) {
        (
            make_prefix(
                r,
                mod_side,
                Some(ModGlyph::Mid),
                styled_prefixes,
                theme,
                line_num_width,
            ),
            make_cont_prefix(
                mod_side,
                Some(ModGlyph::Mid),
                styled_prefixes,
                theme,
                line_num_width,
            ),
            make_cont_prefix(
                mod_side,
                Some(ModGlyph::Bottom),
                styled_prefixes,
                theme,
                line_num_width,
            ),
        )
    } else {
        let p = make_prefix(
            r,
            mod_side,
            mod_glyph,
            styled_prefixes,
            theme,
            line_num_width,
        );
        let c = make_cont_prefix(mod_side, mod_glyph, styled_prefixes, theme, line_num_width);
        (p, c.clone(), c)
    };

    for s_idx in 0..seg_count {
        let (seg_pfx, is_cont) = if s_idx == 0 {
            (&pfx, false)
        } else if s_idx + 1 == seg_count {
            (&cont_bottom_pfx, true)
        } else {
            (&cont_mid_pfx, true)
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

/// Render one diff row in side-by-side (split dual-pane) layout, appending
/// complete lines to `out`.
pub(crate) fn render_side_by_side_row_line(
    out: &mut String,
    r: &OutputRow,
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
    old_line_num_width: usize,
    new_line_num_width: usize,
) {
    let layout = split_pane_layout(width, old_line_num_width, new_line_num_width);
    if layout.left_w == 0 && layout.right_w == 0 {
        return;
    }

    let seg_count = side_by_side_row_seg_count(r, layout.left_cols, layout.right_cols, 0);

    let mut left_pfx = styled_split_left_prefix_spans(
        r.kind,
        r.mov_side,
        r.group,
        r.old_line,
        false,
        None,
        theme,
        old_line_num_width,
    );
    let mut right_pfx = styled_split_right_prefix_spans(
        r.kind,
        r.mov_side,
        r.group,
        r.new_line,
        false,
        None,
        theme,
        new_line_num_width,
    );
    let mut left_cont_pfx = styled_split_left_continuation_prefix_spans(None, old_line_num_width);
    let mut right_cont_pfx = styled_split_right_continuation_prefix_spans(None, new_line_num_width);

    if !styled_prefixes {
        for s in &mut left_pfx {
            *s = StyledSpan::plain(std::mem::take(&mut s.text));
        }
        for s in &mut right_pfx {
            *s = StyledSpan::plain(std::mem::take(&mut s.text));
        }
        for s in &mut left_cont_pfx {
            *s = StyledSpan::plain(std::mem::take(&mut s.text));
        }
        for s in &mut right_cont_pfx {
            *s = StyledSpan::plain(std::mem::take(&mut s.text));
        }
    }

    for s_idx in 0..seg_count {
        let lp = if s_idx == 0 {
            &left_pfx
        } else {
            &left_cont_pfx
        };
        let rp = if s_idx == 0 {
            &right_pfx
        } else {
            &right_cont_pfx
        };
        let line = render_side_by_side_row_segment(
            r,
            s_idx,
            layout.left_w,
            layout.right_w,
            lp,
            rp,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        out.push_str(&line);
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
