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

use crate::parse::{Hunk, Row};
use std::cmp::Reverse;

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
    hunks.sort_by_key(|h| Reverse(h.0));
    for (old_start, old_count, body) in hunks {
        let mut new_lines = Vec::new();
        let mut cursor = if old_start == 0 { 0 } else { old_start - 1 };
        for line in &body {
            if let Some(ctx) = line.strip_prefix(' ') {
                if cursor < out.len() {
                    assert_eq!(
                        &out[cursor],
                        ctx,
                        "apply_file_diff: context mismatch at line {}",
                        cursor + 1
                    );
                }
                new_lines.push(ctx.to_string());
                cursor += 1;
            } else if let Some(add) = line.strip_prefix('+') {
                new_lines.push(add.to_string());
            } else if let Some(del) = line.strip_prefix('-') {
                if cursor < out.len() {
                    assert_eq!(
                        &out[cursor],
                        del,
                        "apply_file_diff: deletion mismatch at line {}",
                        cursor + 1
                    );
                }
                cursor += 1;
            }
        }
        if old_start == 0 {
            assert_eq!(
                cursor, 0,
                "apply_file_diff: old_start is 0 but consumed {cursor} lines"
            );
            out.splice(0..0, new_lines);
        } else {
            let lo = old_start - 1;
            let consumed = cursor - lo;
            assert_eq!(
                consumed, old_count,
                "apply_file_diff: hunk old_count ({old_count}) != consumed lines ({consumed}) at old_start {old_start}"
            );
            let hi = (lo + old_count).min(out.len());
            out.splice(lo..hi, new_lines);
        }
    }
    out
}

/// Apply a parsed file's hunks and rows (at any stage of transformation in
/// Phase 6) to its old contents → producing the new contents.
pub fn apply_hunk_rows(hunks: &[Hunk], old: &[String]) -> Vec<String> {
    let mut out: Vec<String> = old.to_vec();
    let mut sorted_hunks: Vec<&Hunk> = hunks.iter().collect();
    sorted_hunks.sort_by_key(|h| Reverse(h.old_start as usize));
    for h in sorted_hunks {
        let old_start = h.old_start as usize;
        let old_count = h.old_count as usize;
        let mut new_lines = Vec::new();
        let mut cursor = if old_start == 0 { 0 } else { old_start - 1 };
        for r in &h.rows {
            match r {
                Row::Ctx(ctx_line) => {
                    if cursor < out.len() {
                        assert_eq!(
                            &out[cursor],
                            &ctx_line.text,
                            "Ctx line mismatch at line {}: expected {:?}, got {:?}",
                            cursor + 1,
                            ctx_line.text,
                            out[cursor]
                        );
                    }
                    new_lines.push(ctx_line.text.clone());
                    cursor += 1;
                }
                Row::Add(add_line) => {
                    new_lines.push(add_line.text.clone());
                }
                Row::Del(del_line) => {
                    if cursor < out.len() {
                        assert_eq!(
                            &out[cursor],
                            &del_line.text,
                            "Del line mismatch at line {}: expected {:?}, got {:?}",
                            cursor + 1,
                            del_line.text,
                            out[cursor]
                        );
                    }
                    cursor += 1;
                }
                Row::Mod { old_line, new_line } | Row::Mov { old_line, new_line } => {
                    for line in old_line.text.split('\n') {
                        if cursor < out.len() {
                            assert_eq!(
                                &out[cursor],
                                line,
                                "Mod del line mismatch at line {}: expected {:?}, got {:?}",
                                cursor + 1,
                                line,
                                out[cursor]
                            );
                        }
                        cursor += 1;
                    }
                    for line in new_line.text.split('\n') {
                        new_lines.push(line.to_string());
                    }
                }
                Row::Sep(_) => {}
            }
        }
        if old_start == 0 {
            assert_eq!(cursor, 0, "old_start is 0 but consumed {cursor} lines");
            out.splice(0..0, new_lines);
        } else {
            let lo = old_start - 1;
            let consumed = cursor - lo;
            assert_eq!(
                consumed, old_count,
                "hunk old_count ({old_count}) != consumed lines ({consumed}) at old_start {old_start}"
            );
            let hi = (lo + old_count).min(out.len());
            out.splice(lo..hi, new_lines);
        }
    }
    out
}

/// Single-stream mapping: transforms `old` lines via a sequence of rows into
/// `new` lines. `read(old_file).map(diff).collect() === new_file`
pub fn apply_stream(rows: &[Row], old: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    for r in rows {
        match r {
            Row::Ctx(ctx_line) => {
                if cursor < old.len() {
                    assert_eq!(
                        &old[cursor],
                        &ctx_line.text,
                        "apply_stream: context mismatch at line {}",
                        cursor + 1
                    );
                }
                out.push(ctx_line.text.clone());
                cursor += 1;
            }
            Row::Add(add_line) => {
                out.push(add_line.text.clone());
            }
            Row::Del(del_line) => {
                if cursor < old.len() {
                    assert_eq!(
                        &old[cursor],
                        &del_line.text,
                        "apply_stream: deletion mismatch at line {}",
                        cursor + 1
                    );
                }
                cursor += 1;
            }
            Row::Mod { old_line, new_line } | Row::Mov { old_line, new_line } => {
                for line in old_line.text.split('\n') {
                    if cursor < old.len() {
                        assert_eq!(
                            &old[cursor],
                            line,
                            "apply_stream: mod del mismatch at line {}",
                            cursor + 1
                        );
                    }
                    cursor += 1;
                }
                for line in new_line.text.split('\n') {
                    out.push(line.to_string());
                }
            }
            Row::Sep(_) => {}
        }
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

#[cfg(test)]
mod tests;
