//! Markdown Table Border & Cell Classification.
//!
//! Recognizes table alignment delimiters (`|---|---|`) and classifies border
//! and cell text tokens within Markdown tables.

use super::{style::emit_styled_token, token::ClassifiedToken};
use crate::{color::TokenColor, profile::SGR_BOLD_UNDERLINE};

/// Returns whether `line` is a markdown table alignment row (e.g.
/// `|---|:--:|`).
pub fn is_line_alignment_row(line: &str) -> bool {
    let s = line.trim();
    let stripped = s.trim_start_matches(['/', '*', '#', '!', ' ']);
    is_table_alignment_row(stripped)
}

pub(crate) fn is_table_alignment_row(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() || !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    if !trimmed
        .chars()
        .all(|c| c == '|' || c == ':' || c == '-' || c.is_whitespace())
    {
        return false;
    }
    let cols: Vec<&str> = trimmed
        .split('|')
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .collect();
    !cols.is_empty() && cols.iter().all(|c| c.contains('-'))
}

pub(crate) fn emit_table_row_text_tokens<'a>(
    tok_slice: &'a str,
    active_color: TokenColor,
    active_sgr: Option<u32>,
    is_header: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    let (cell_color, cell_sgr) = if is_header {
        (TokenColor::TableHeader, Some(SGR_BOLD_UNDERLINE))
    } else {
        (active_color, active_sgr)
    };
    emit_table_line_slice(tok_slice, cell_color, cell_sgr, is_header, f);
}

pub(crate) fn emit_table_line_slice<'a>(
    slice: &'a str,
    active_color: TokenColor,
    active_sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if is_table_alignment_row(slice) {
        f(ClassifiedToken {
            color: TokenColor::TableBorder,
            sgr: None,
            text: slice,
        });
        return;
    }
    let mut last = 0usize;
    for (i, c) in slice.char_indices() {
        if c == '|' {
            if i > last {
                let cell_part = &slice[last..i];
                let trimmed = cell_part.trim();
                if !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '-' || ch == ':') {
                    f(ClassifiedToken {
                        color: TokenColor::TableBorder,
                        sgr: None,
                        text: cell_part,
                    });
                } else {
                    emit_styled_token(cell_part, active_color, active_sgr, in_table_head, f);
                }
            }
            f(ClassifiedToken {
                color: TokenColor::TableBorder,
                sgr: None,
                text: &slice[i..i + 1],
            });
            last = i + 1;
        }
    }
    if last < slice.len() {
        let cell_part = &slice[last..];
        let trimmed = cell_part.trim();
        if !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '-' || ch == ':') {
            f(ClassifiedToken {
                color: TokenColor::TableBorder,
                sgr: None,
                text: cell_part,
            });
        } else {
            emit_styled_token(cell_part, active_color, active_sgr, in_table_head, f);
        }
    }
}
