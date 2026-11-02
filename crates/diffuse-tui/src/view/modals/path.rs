//! Fuzzy path matching, truncation, and highlighted span formatting.

use std::{borrow::Cow, mem};

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::render::{ColorExt, ThemePalette};

pub(crate) fn format_path_matches(
    path: &str,
    indices: &[usize],
    max_w: usize,
    selected: bool,
    palette: &ThemePalette,
) -> (String, usize) {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        (
            highlight_path_matches(&clean, indices, selected, palette),
            char_count,
        )
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        let len = truncated.chars().count();
        (
            highlight_path_matches(&truncated, indices, selected, palette),
            len,
        )
    } else {
        let keep_chars = max_w - 1;
        let skip_chars = char_count - keep_chars;
        let mut actual_skip = skip_chars;
        if let Some(slash_offset) = clean
            .chars()
            .skip(skip_chars)
            .take(8)
            .position(|c| c == '/')
        {
            let candidate_skip = skip_chars + slash_offset;
            if char_count - candidate_skip < max_w {
                actual_skip = candidate_skip;
            }
        }

        let tail: String = clean.chars().skip(actual_skip).collect();
        let shifted_indices: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&idx| idx >= actual_skip)
            .map(|idx| idx - actual_skip)
            .collect();
        let default_color = if selected {
            format!("\x1b[1m{}", palette.modal_title.fg_code())
        } else {
            palette.modal_text.fg_code()
        };
        let tail_highlighted = highlight_path_matches(&tail, &shifted_indices, selected, palette);
        (
            format!("{default_color}…{tail_highlighted}"),
            1 + tail.chars().count(),
        )
    }
}

pub(crate) fn format_path_matches_spans(
    path: &str,
    indices: &[usize],
    max_w: usize,
    selected: bool,
    palette: &ThemePalette,
) -> Vec<Span<'static>> {
    let clean = if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    };
    let char_count = clean.chars().count();
    if char_count <= max_w {
        highlight_path_matches_spans(&clean, indices, selected, palette)
    } else if max_w <= 2 {
        let truncated: String = clean.chars().take(max_w).collect();
        highlight_path_matches_spans(&truncated, indices, selected, palette)
    } else {
        let keep_chars = max_w - 1;
        let skip_chars = char_count - keep_chars;
        let mut actual_skip = skip_chars;
        if let Some(slash_offset) = clean
            .chars()
            .skip(skip_chars)
            .take(8)
            .position(|c| c == '/')
        {
            let candidate_skip = skip_chars + slash_offset;
            if char_count - candidate_skip < max_w {
                actual_skip = candidate_skip;
            }
        }

        let tail: String = clean.chars().skip(actual_skip).collect();
        let shifted_indices: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&idx| idx >= actual_skip)
            .map(|idx| idx - actual_skip)
            .collect();
        let default_style = if selected {
            Style::default()
                .fg(crate::view::to_color(palette.modal_title))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::view::to_color(palette.modal_text))
        };
        let mut spans = vec![Span::styled("…", default_style)];
        spans.extend(highlight_path_matches_spans(
            &tail,
            &shifted_indices,
            selected,
            palette,
        ));
        spans
    }
}

pub(crate) fn highlight_path_matches_spans(
    path: &str,
    indices: &[usize],
    selected: bool,
    palette: &ThemePalette,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let default_style = if selected {
        Style::default()
            .fg(crate::view::to_color(palette.modal_title))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(crate::view::to_color(palette.modal_text))
    };
    let match_style = Style::default()
        .fg(crate::view::to_color(palette.accent))
        .add_modifier(Modifier::BOLD);

    let mut cur_text = String::new();
    let mut cur_matched = None;

    for (i, ch) in path.char_indices() {
        let is_m = indices.contains(&i);
        if Some(is_m) != cur_matched && !cur_text.is_empty() {
            let style = if cur_matched.unwrap_or(false) {
                match_style
            } else {
                default_style
            };
            spans.push(Span::styled(mem::take(&mut cur_text), style));
        }
        cur_matched = Some(is_m);
        cur_text.push(ch);
    }
    if !cur_text.is_empty() {
        let style = if cur_matched.unwrap_or(false) {
            match_style
        } else {
            default_style
        };
        spans.push(Span::styled(cur_text, style));
    }
    spans
}

pub(crate) fn highlight_path_matches(
    path: &str,
    indices: &[usize],
    selected: bool,
    palette: &ThemePalette,
) -> String {
    let mut out = String::new();
    let default_color = if selected {
        format!("\x1b[1m{}", palette.modal_title.fg_code())
    } else {
        palette.modal_text.fg_code()
    };
    let match_color = format!("\x1b[1m{}", palette.accent.fg_code());

    let mut cur_matched = None;
    for (i, ch) in path.char_indices() {
        let is_m = indices.contains(&i);
        if Some(is_m) != cur_matched {
            if is_m {
                out.push_str(&match_color);
            } else {
                out.push_str(&default_color);
            }
            cur_matched = Some(is_m);
        }
        out.push(ch);
    }
    out
}
