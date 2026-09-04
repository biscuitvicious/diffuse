//! Reconstruct the old file from the diff (the context + `-` lines), so
//! the brace-anchor pairing can read the del's fn on the old side. Port of
//! the golden test's `oldContentFromDiff` — the fixture carries only the
//! NEW content; the old side is derived from the diff's hunks.

/// Reconstruct the old side of `path` from the diff (context + del lines;
/// gaps become empty lines). `-U3`-captured diffs carry the surrounding
/// context, which is what makes the fn's closing brace recoverable.
pub fn old_content_from_diff(diff: &str, path: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let lines = diff.split('\n');
    let mut cur: Option<&str> = None; // the diff section's file path, if ours
    let mut oi: usize = 0;
    let b_suffix = format!(" b/{path}");
    let a_prefix = format!("diff --git a/{path} ");
    for line in lines {
        if line.starts_with("diff --git ") {
            if cur.is_some() {
                break;
            }
            cur = if line.ends_with(&b_suffix) || line.starts_with(&a_prefix) {
                Some("")
            } else {
                None
            };
            continue;
        }
        let Some(_) = cur else { continue };
        if let Some(rest) = line.strip_prefix("@@ ") {
            let m = rest
                .split(' ')
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse::<usize>().ok());
            oi = m.unwrap_or(1);
            // A NEW file's hunk is `@@ -0,0 +1,N @@` — old start 0. The JS
            // `oi - 1` is a number (-1 is fine); usize must saturate (no
            // grow for a new file — the TS keeps its old side empty).
            while out.len() < oi.saturating_sub(1) {
                out.push(String::new());
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if rest.starts_with("--") {
                continue; // the `--- a/file` header (three dashes)
            }
            // NOTE: a del whose CONTENT starts with a dash (`-- item`, a
            // markdown list bullet `- item`) is a real del — only the
            // three-dash header is skipped. A single `rest.starts_with('-')`
            // here would silently drop it (the PLAN.md `--golden` bullet).
            // A del at oi == 0 (a malformed `@@ -0,0` hunk) must not wrap
            // the index — the JS writes a "-1" property (harmless); usize
            // would bounds-check-panic in release. Skip, still advance oi.
            if oi > 0 {
                while out.len() < oi {
                    out.push(String::new());
                }
                out[oi - 1] = rest.to_string();
            }
            oi += 1;
        } else if line.starts_with('+') {
            // skip
        } else if let Some(rest) = line.strip_prefix(' ') {
            // A context line (one prefix space + the content) — written only
            // when the slot is past the array's end (the TS's
            // `out[oi - 1] === undefined`): a hunk's first context lines may
            // repeat old lines already seen, and the second occurrence must
            // not overwrite. A `>= oi` check would never fire and would drop
            // the line — the brace-anchor reads it. (`oi - 1` for a new
            // file's `-0,0` hunk would underflow — skip instead.)
            if oi > 0 && out.len() == oi - 1 {
                out.push(rest.to_string());
            }
            oi += 1;
        } else if line.is_empty() {
            if oi > 0 && out.len() == oi - 1 {
                out.push(String::new());
            }
            oi += 1;
        }
    }
    out
}

/// Reconstruct the old side of a file directly from its parsed hunks without
/// needing to re-scan and split raw diff strings.
pub fn old_content_from_hunks(hunks: &[crate::parse::Hunk]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in hunks {
        if h.old_start == 0 {
            continue;
        }
        let oi = h.old_start as usize;
        while out.len() < oi.saturating_sub(1) {
            out.push(String::new());
        }
        for r in &h.rows {
            match r {
                crate::parse::Row::Del(del) => {
                    let num = del.num.get() as usize;
                    if num > 0 {
                        while out.len() < num {
                            out.push(String::new());
                        }
                        out[num - 1] = del.text.clone();
                    }
                }
                crate::parse::Row::Ctx(ctx) => {
                    let num = ctx.old_pos.get() as usize;
                    if num > 0 {
                        while out.len() < num {
                            out.push(String::new());
                        }
                        if out[num - 1].is_empty() {
                            out[num - 1] = ctx.text.clone();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Reconstruct the new side of a file directly from its parsed hunks.
pub fn new_content_from_hunks(hunks: &[crate::parse::Hunk]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in hunks {
        if h.new_start == 0 {
            continue;
        }
        let ni = h.new_start as usize;
        while out.len() < ni.saturating_sub(1) {
            out.push(String::new());
        }
        for r in &h.rows {
            match r {
                crate::parse::Row::Add(add) => {
                    let num = add.num.get() as usize;
                    if num > 0 {
                        while out.len() < num {
                            out.push(String::new());
                        }
                        out[num - 1] = add.text.clone();
                    }
                }
                crate::parse::Row::Ctx(ctx) => {
                    let num = ctx.new_pos.get() as usize;
                    if num > 0 {
                        while out.len() < num {
                            out.push(String::new());
                        }
                        if out[num - 1].is_empty() {
                            out[num - 1] = ctx.text.clone();
                        }
                    }
                }
                crate::parse::Row::Mod { new_line, .. } | crate::parse::Row::MovTgt(new_line) => {
                    let num = new_line.num.get() as usize;
                    if num > 0 {
                        while out.len() < num {
                            out.push(String::new());
                        }
                        out[num - 1] = new_line.text.clone();
                    }
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
