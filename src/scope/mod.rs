//! Function scope and block-ending brace anchor helpers for diff pairing.

use regex::Regex;
use std::sync::OnceLock;

/// True when the row is the last expression of its block: the next line
/// on its side is a closing brace at a LOWER indent (the fn's `}` after the
/// return `None`/`Some(result)` — not the inner if's `}`).
pub fn is_tail_before_closing(
    line: Option<u32>,
    old_side: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> bool {
    let content = if old_side { old_content } else { new_content };
    let line = match line {
        Some(l) => l as usize,
        None => return false,
    };
    let content = match content {
        Some(c) => c,
        None => return false,
    };
    if content.get(line).is_none() {
        return false;
    }
    // The closing brace sits one indent level down from the fn's own
    // body lines (`    }` after `        None` / `        Some(result)`) —
    // not the inner block's brace after a nested statement.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^\s{4}\}\s*$").expect("tail brace regex"));
    re.is_match(&content[line])
}

/// The name of the fn that contains `line` — the nearest preceding `fn`
/// signature. The blank line between fn bodies is the section boundary: the
/// `None` belongs to `lookup`, so it pairs with an add inside `lookup`, not
/// another fn's return expression.
pub fn fn_of(content: Option<&[String]>, line: Option<u32>) -> Option<String> {
    let content = content?;
    let line = line? as usize;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?fn\s+([A-Za-z0-9_]+)").expect("fn regex")
    });
    for i in (0..line.saturating_sub(1)).rev() {
        let raw = content.get(i).map_or("", |s| s.as_str());
        let l = raw.trim();
        if raw.starts_with('}')
            || l.starts_with("struct ")
            || l.starts_with("pub struct ")
            || l.starts_with("pub(super) struct ")
            || l.starts_with("enum ")
            || l.starts_with("pub enum ")
            || l.starts_with("impl ")
            || l.starts_with("pub impl ")
            || l.starts_with("type ")
            || l.starts_with("pub type ")
        {
            return None;
        }
        if let Some(caps) = re.captures(l) {
            return Some(caps[1].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests;
