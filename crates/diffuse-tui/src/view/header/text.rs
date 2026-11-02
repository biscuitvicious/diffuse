#[cfg(test)]
use crate::core::wire::OutputFile;
use crate::render::{
    slice_by_visual_width,
    width::{display_width, grapheme_width},
};
use unicode_segmentation::UnicodeSegmentation;

/// Calculate the visible column width of a string by ignoring ANSI escape
/// sequences and measuring grapheme display widths (wide chars count two).
pub fn visible_width(s: &str) -> usize {
    let mut in_escape = false;
    let mut count = 0;
    let mut text_start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c == '\x1b' {
            if let Some(start) = text_start.take() {
                count += display_width(&s[start..i]);
            }
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\t' {
            if let Some(start) = text_start.take() {
                count += display_width(&s[start..i]);
            }
            let tab_spaces = 4 - (count % 4);
            count += tab_spaces;
        } else if text_start.is_none() {
            text_start = Some(i);
        }
    }
    if let Some(start) = text_start {
        count += display_width(&s[start..]);
    }
    count
}

/// Wrap text into segments of `max_cols` visual width.
pub fn wrap_text_by_visual_width(text: &str, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        return vec![text.to_string()];
    }
    let total_w = visible_width(text);
    if total_w <= max_cols {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut start_col = 0;
    while start_col < total_w {
        let piece = slice_by_visual_width(text, start_col, max_cols);
        if piece.is_empty() {
            break;
        }
        start_col += max_cols;
        lines.push(piece);
    }
    lines
}

/// Truncate or right-pad an ANSI-escaped string with spaces to exactly
/// `target_width` terminal columns, guaranteeing that the rendered line never
/// exceeds or falls short of `target_width`.
pub fn clamp_or_pad_ansi(s: &str, target_width: usize) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut vis_w = 0usize;

    let mut chars = s.chars().peekable();
    while let Some(c) = chars.peek().copied() {
        if c == '\x1b' {
            chars.next();
            out.push('\x1b');
            if chars.peek() == Some(&'[') {
                out.push(chars.next().unwrap());
                while let Some(&ch) = chars.peek() {
                    out.push(chars.next().unwrap());
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }

        if c == '\t' {
            chars.next();
            let tab_spaces = 4 - (vis_w % 4);
            if vis_w + tab_spaces <= target_width {
                out.push_str(&" ".repeat(tab_spaces));
                vis_w += tab_spaces;
            } else {
                let rem = target_width.saturating_sub(vis_w);
                if rem > 0 {
                    out.push_str(&" ".repeat(rem));
                    vis_w += rem;
                }
                break;
            }
            continue;
        }

        let mut g = String::new();
        g.push(chars.next().unwrap());
        while let Some(&next_c) = chars.peek() {
            if next_c == '\x1b' || next_c == '\t' {
                break;
            }
            let mut test_s = g.clone();
            test_s.push(next_c);
            if test_s.graphemes(true).count() == 1 {
                g.push(chars.next().unwrap());
            } else {
                break;
            }
        }

        let gw = grapheme_width(&g);
        if gw > 0 && vis_w + gw > target_width {
            let rem = target_width.saturating_sub(vis_w);
            if rem > 0 {
                out.push_str(&" ".repeat(rem));
                vis_w += rem;
            }
            break;
        }
        out.push_str(&g);
        vis_w += gw;
    }

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            out.push(c);
            out.push(chars.next().unwrap());
            while let Some(&ch) = chars.peek() {
                out.push(chars.next().unwrap());
                if ch.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }

    if vis_w < target_width {
        out.push_str(&" ".repeat(target_width - vis_w));
    }
    out.push_str("\x1b[0m");
    out
}

/// Truncate or right-pad a string with spaces to exactly `target_width`
/// terminal columns (accounting for wide characters/emojis).
pub fn truncate_pad(s: &str, target_width: usize) -> String {
    let mut current_width = 0;
    let mut out = String::new();
    for g in s.graphemes(true) {
        let gw = grapheme_width(g);
        if current_width + gw > target_width {
            break;
        }
        out.push_str(g);
        current_width += gw;
    }
    if current_width < target_width {
        out.push_str(&" ".repeat(target_width - current_width));
    }
    out
}

/// Shorten a status-bar path to fit `budget` columns, keeping the end and
/// marking the cut with a leading `…` (grapheme-safe for wide glyphs).
/// Returns `None` when the path fits or the budget cannot hold even the
/// ellipsis.
#[must_use]
pub fn truncate_status_path(path: &str, budget: usize) -> Option<String> {
    let path_w = display_width(path);
    if path_w <= budget || budget == 0 {
        return None;
    }
    let keep = budget.saturating_sub(1);
    let tail = slice_by_visual_width(path, path_w.saturating_sub(keep), keep);
    Some(format!("…{tail}"))
}

/// Current file path stripped of the shared directory prefix, rendering
/// renames as `old -> new`.
#[cfg(test)]
pub(crate) fn stripped_file_path(file: &OutputFile, common_prefix: Option<&str>) -> String {
    let stripped_old = file.old_path.as_deref().map(|old| {
        if let Some(pfx) = common_prefix {
            old.strip_prefix(pfx).unwrap_or(old).to_string()
        } else {
            old.to_string()
        }
    });
    let stripped_path = if let Some(pfx) = common_prefix {
        file.path
            .strip_prefix(pfx)
            .unwrap_or(&file.path)
            .to_string()
    } else {
        file.path.clone()
    };
    if let Some(old) = stripped_old
        && old != stripped_path
    {
        format!("{old} -> {stripped_path}")
    } else {
        stripped_path
    }
}

/// File status tag; routine `modified`/`renamed`/`unmodified` states stay
/// unadorned.
#[cfg(test)]
pub(crate) fn file_status_tag(file: &OutputFile) -> String {
    if file.status == "renamed" || file.status == "modified" || file.status == "unmodified" {
        String::new()
    } else {
        format!(" ({})", file.status)
    }
}
