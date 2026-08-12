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
            if rest.starts_with("--") {
                continue; // the `--- a/file` header (three dashes)
            }
            // NOTE: a del whose CONTENT starts with a dash (`-- item`, a
            // markdown list bullet `- item`) is a real del — only the
            // three-dash header is skipped. A single `rest.starts_with('-')`
            // here would silently drop it (the PLAN.md `--golden` bullet).
            while out.len() < oi {
                out.push(String::new());
            }
            out[oi - 1] = rest.to_string();
            oi += 1;
        } else if line.starts_with('+') {
            // skip
        } else if let Some(rest) = line.strip_prefix(' ') {
            // A context line (one prefix space + the content) — written only
            // when the slot is past the array's end (the TS's
            // `out[oi - 1] === undefined`): a hunk's first context lines may
            // repeat old lines already seen, and the second occurrence must
            // not overwrite. A `>= oi` check would never fire and would drop
            // the line — the brace-anchor reads it.
            if out.len() == oi - 1 {
                out.push(rest.to_string());
            }
            oi += 1;
        } else if line.is_empty() {
            if out.len() == oi - 1 {
                out.push(String::new());
            }
            oi += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::old_content_from_diff;

    // A del whose CONTENT starts with a dash (`-- item` — a markdown list
    // bullet `- item`) is a real del; only the three-dash `--- a/file`
    // header is skipped. The diffuse-repo PLAN.md had a `- \`--golden\`…`
    // bullet that the port's `rest.starts_with('-')` check swallowed,
    // shifting every later old line by one.
    #[test]
    fn del_content_starting_with_dash_is_kept() {
        let diff = "diff --git a/PLAN.md b/PLAN.md\n--- a/PLAN.md\n+++ b/PLAN.md\n@@ -113,2 +114,7 @@\n-- `--golden` switches the output\n-  test, §5).\n+new\n";
        let oc = old_content_from_diff(diff, "PLAN.md");
        // 115 = old lines 1..=114 + the diff's trailing newline (an empty
        // context line — the TS behaves identically, verified against it).
        assert_eq!(oc.len(), 115, "lines 1..=114 present");
        assert_eq!(
            oc[112], "- `--golden` switches the output",
            "the dash-content del survives"
        );
        assert_eq!(oc[113], "  test, §5).");
    }

    // The context-line write fires when the slot is past the array's end
    // (the TS's `out[oi - 1] === undefined`).
    #[test]
    fn context_line_after_del_run() {
        let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -100,2 +100,2 @@\n-        None\n     }\n";
        let oc = old_content_from_diff(diff, "x.rs");
        assert_eq!(oc[99], "        None");
        assert_eq!(oc[100], "    }", "the context line after the del");
    }
}
