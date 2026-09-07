//! Function scope and block-ending delimiter anchor helpers for diff pairing.

use crate::syntax::profile::{BlockDelimStyle, LangProfile};

/// True when the row is the last expression of its block: the next line
/// on its side is a closing delimiter at a LOWER indent (the fn's `}` after the
/// return `None`/`Some(result)` — not the inner if's `}`).
///
/// For brace languages: the next line is a lone `}` at 4 fewer spaces of indent
/// than the current line (the canonical 4-space fn-body indent). For other
/// block styles the same logic applies via `profile.is_block_close`.
pub fn is_tail_before_closing(
    line: Option<u32>,
    old_side: bool,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    let Some(profile) = profile else {
        return false;
    };
    let content = if old_side { old_content } else { new_content };
    let line = match line {
        Some(l) => l as usize,
        None => return false,
    };
    let content = match content {
        Some(c) => c,
        None => return false,
    };
    if line == 0 || line > content.len() {
        return false;
    }
    let curr = content[line - 1];
    if profile.is_block_open(curr) {
        return false;
    }
    let next = match content.get(line) {
        Some(n) => n,
        None => return false,
    };
    match profile.block_style {
        BlockDelimStyle::Braces | BlockDelimStyle::Keywords => profile.is_block_close(next),
        BlockDelimStyle::IndentColon => {
            if profile.significant_indent {
                let curr_indent = curr.len() - curr.trim_start().len();
                let next_indent = next.len() - next.trim_start().len();
                !next.trim().is_empty() && next_indent < curr_indent
            } else {
                false
            }
        }
    }
}

/// The name of the function that contains `line` — the nearest preceding
/// function signature found by scanning backward through `content`.
///
/// The search is driven by `profile`:
/// - `profile.fn_decl` (if set) is the regex whose first capture group is the
///   function name.
/// - `profile.scope_stop_prefixes` list declaration-starting strings that act
///   as hard boundaries.
///
/// If `profile` or `profile.fn_decl` is `None`, returns `None` (plain text, no
/// scope detection).
pub fn fn_of(
    content: Option<&[&str]>,
    line: Option<u32>,
    profile: Option<&LangProfile>,
) -> Option<String> {
    let profile = profile?;
    let fn_re = profile.fn_decl.as_ref()?;
    let content = content?;
    let line = line? as usize;

    let stop_prefixes = profile.scope_stop_prefixes;

    for i in (0..line).rev() {
        let raw = content.get(i).copied().unwrap_or("");
        let l = raw.trim();
        if l.contains('(')
            && let Some(caps) = fn_re.captures(l)
        {
            return Some(caps[1].to_string());
        }
        if Some(i) != line.checked_sub(1)
            && (profile.is_block_close(l) || stop_prefixes.iter().any(|&pf| l.starts_with(pf)))
        {
            return None;
        }
    }
    None
}

/// Return `true` if any line in `content` declares a function named `fn_name`,
/// using `profile.fn_decl`. If no profile or `fn_decl` exists, returns `false`.
pub fn fn_of_exists(
    content: Option<&[&str]>,
    fn_name: &str,
    profile: Option<&LangProfile>,
) -> bool {
    let Some(profile) = profile else {
        return false;
    };
    let Some(fn_re) = profile.fn_decl.as_ref() else {
        return false;
    };
    let Some(content) = content else {
        return false;
    };
    content.iter().any(|l| {
        let trimmed = l.trim();
        trimmed.contains(fn_name)
            && trimmed.contains('(')
            && fn_re
                .captures(trimmed)
                .is_some_and(|c| c.get(1).is_some_and(|m| m.as_str() == fn_name))
    })
}

/// Returns the slice of lines corresponding to the body of function `fn_name`
/// in `content`, bounded by the opening signature down to the enclosing block
/// close.
pub fn fn_body_lines<'a>(
    content: Option<&'a [&'a str]>,
    fn_name: &str,
    profile: Option<&LangProfile>,
) -> Option<&'a [&'a str]> {
    let profile = profile?;
    let fn_re = profile.fn_decl.as_ref()?;
    let content = content?;
    let stop_prefixes = profile.scope_stop_prefixes;

    let start_idx = content.iter().position(|l| {
        let trimmed = l.trim();
        trimmed.contains(fn_name)
            && trimmed.contains('(')
            && fn_re
                .captures(trimmed)
                .is_some_and(|c| c.get(1).is_some_and(|m| m.as_str() == fn_name))
    })?;

    let mut depth = 0i32;
    let mut end_idx = content.len();
    for (i, &line) in content[start_idx..].iter().enumerate() {
        let l = line.trim();
        if profile.is_block_open(l) {
            depth += 1;
        }
        if profile.is_block_close(l) {
            depth -= 1;
            if depth <= 0 && i > 0 {
                end_idx = start_idx + i + 1;
                break;
            }
        } else if depth <= 0 && i > 0 && stop_prefixes.iter().any(|&pf| l.starts_with(pf)) {
            end_idx = start_idx + i;
            break;
        }
    }
    Some(&content[start_idx..end_idx])
}

#[cfg(test)]
mod tests;
