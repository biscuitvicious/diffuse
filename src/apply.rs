//! Apply a unified diff to a file's old contents → the new contents.
//!
//! The Phase 6 foundation (PLAN.md §4): the **apply-oracle** — proving a
//! diff is a semantic program, not just a render. `apply_diff` is the
//! mechanical primitive: feed it the per-file diff slice + the OLD file's
//! lines, get the NEW file's lines. The oracle asserts
//! `apply(file_diff, old) == new` for every fixture that carries the old
//! side — validating the fixture is a real, applicable diff and locking
//! the engine's notion of a diff's semantics.
//!
//! The algorithm is the standard unified-diff apply: parse the hunks
//! (`@@ -oldStart,oldCount +newStart,newCount @@`), then for each hunk
//! delete `oldCount` lines at `oldStart-1` and insert the hunk's new block
//! (the context lines + the `+` lines, in order; the `-` lines dropped).
//! Hunks apply in **reverse old-start order** so an earlier hunk's offset
//! is unaffected by a later hunk's net line change.

/// Apply a single file's unified-diff slice to its old contents.
///
/// `file_diff` is the slice of the full diff for ONE path — from the
/// `diff --git a/<p> b/<p>` header through the next `diff --git` or the
/// end. (The full diff is multi-file; apply is per-path.) `old` is the
/// old file's lines (no trailing line-count adjustment — `split('\n')`
/// semantics, as the engine uses elsewhere).
pub fn apply_file_diff(file_diff: &str, old: &[String]) -> Vec<String> {
    let mut out: Vec<String> = old.to_vec();
    let lines: Vec<&str> = file_diff.split('\n').collect();
    let n = lines.len();
    let mut i = 0;
    while i < n && !lines[i].starts_with("@@") {
        i += 1;
    }
    // Collect the hunks (old_start, old_count, body lines).
    let mut hunks: Vec<(usize, usize, Vec<&str>)> = Vec::new();
    while i < n {
        let m = match parse_hunk_header(lines[i]) {
            Some(m) => m,
            None => {
                i += 1;
                continue;
            }
        };
        let (old_start, old_count) = (m.0, m.1);
        i += 1;
        let mut body: Vec<&str> = Vec::new();
        // A hunk body line ALWAYS starts with ' ', '+', '-', or '\'.
        while i < n && !lines[i].is_empty() && is_body_line(lines[i]) {
            body.push(lines[i]);
            i += 1;
        }
        hunks.push((old_start, old_count, body));
    }
    // Apply in reverse old-start order so earlier offsets are unaffected by
    // later hunks' net line changes.
    hunks.sort_by_key(|h| std::cmp::Reverse(h.0));
    for (old_start, old_count, body) in hunks {
        if old_start == 0 {
            // A new file's `@@ -0,0 +1,N @@` — nothing to delete; insert at 0.
            out.splice(0..0, body.iter().filter_map(new_block_line));
            continue;
        }
        let lo = old_start - 1;
        let hi = (lo + old_count).min(out.len());
        out.splice(lo..hi, body.iter().filter_map(new_block_line));
    }
    out
}

/// Parse `@@ -oldStart,oldCount +newStart,newCount @@` → (oldStart, oldCount).
/// `oldCount` defaults to 1 when the `,N` is omitted (git's convention).
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let seg = rest.split(' ').next()?;
    let seg = seg.strip_prefix('-')?;
    let mut parts = seg.split(',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = parts.next().map(|s| s.parse().unwrap_or(1)).unwrap_or(1);
    Some((start, count))
}

/// A hunk body line: starts with ' ', '+', '-', or '\' (the no-newline marker).
fn is_body_line(s: &str) -> bool {
    matches!(
        s.as_bytes().first(),
        Some(b' ') | Some(b'+') | Some(b'-') | Some(b'\\')
    )
}

/// The new block's lines from a body line: context (` `) and add (`+`),
/// in order; del (`-`) and the no-newline marker (`\`) dropped. The `++`/`--`
/// file headers never appear inside a hunk body (they sit before the first
/// `@@`), so a leading `+`/`-` is always a real add/del.
fn new_block_line(line: &&str) -> Option<String> {
    let l = *line;
    if let Some(rest) = l.strip_prefix('+') {
        Some(rest.to_string())
    } else if l.starts_with('-') || l.starts_with('\\') {
        None
    } else {
        l.strip_prefix(' ').map(String::from)
    }
}

#[cfg(test)]
mod tests {
    use super::apply_file_diff;

    fn s(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn apply_a_simple_hunk() {
        let old = s(&["a", "b", "c", "d", "e"]);
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -2,2 +2,1 @@\n-b\n c\n";
        assert_eq!(apply_file_diff(diff, &old), s(&["a", "c", "d", "e"]));
    }

    #[test]
    fn apply_insert_and_delete() {
        let old = s(&["a", "b", "c"]);
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,2 @@\n a\n+B\n";
        assert_eq!(apply_file_diff(diff, &old), s(&["a", "B", "b", "c"]));
    }

    #[test]
    fn apply_two_hunks_reverse_order() {
        // Two non-overlapping hunks; the second's net change must not shift
        // the first's offset (applied reverse old-start order).
        let old = s(&["a", "b", "c", "d", "e"]);
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,2 @@\n a\n+A\n@@ -4,1 +5,2 @@\n d\n+D\n";
        assert_eq!(
            apply_file_diff(diff, &old),
            s(&["a", "A", "b", "c", "d", "D", "e"])
        );
    }

    #[test]
    fn apply_new_file_hunk_old_start_zero() {
        let diff = "diff --git a/x b/x\nnew file mode 100644\n--- /dev/null\n+++ b/x\n@@ -0,0 +1,2 @@\n+one\n+two\n";
        assert_eq!(apply_file_diff(diff, &[]), s(&["one", "two"]));
    }

    #[test]
    fn apply_drops_no_newline_marker() {
        let old = s(&["a", "b"]);
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -2,1 +2,1 @@\n-b\n+bee\n\\ No newline at end of file\n";
        assert_eq!(apply_file_diff(diff, &old), s(&["a", "bee"]));
    }
}
