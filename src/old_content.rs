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
    for line in lines {
        if line.starts_with("diff --git ") {
            cur = if line.contains(path) { Some("") } else { None };
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
            while out.len() < oi - 1 {
                out.push(String::new());
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if rest.starts_with('-') {
                continue; // the `--- a/file` header
            }
            while out.len() < oi {
                out.push(String::new());
            }
            out[oi - 1] = rest.to_string();
            oi += 1;
        } else if line.starts_with('+') {
            // skip
        } else if let Some(rest) = line.strip_prefix(' ') {
            if out.len() >= oi && out.get(oi - 1).is_none() {
                out[oi - 1] = rest.to_string();
            }
            oi += 1;
        } else if line.is_empty() {
            if out.len() >= oi && out.get(oi - 1).is_none() {
                out[oi - 1] = String::new();
            }
            oi += 1;
        }
    }
    out
}
