//! Unified diff parser for streaming and batch changesets.

use crate::{
    file::{
        DiffFile, FileStatus, hunk_path, is_valid_diff_file, parse_diff_git_paths, unquote_git_path,
    },
    hunk::{Hunk, parse_hunk_header},
    row::{AddLine, CtxLine, DelLine, Row},
};

/// The bare zero-hunk content section a suppressed path carries: complete
/// `---`/`+++` file headers, no hunks.
pub fn zero_hunk_section(path: &str) -> String {
    format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n")
}

/// Parse a sequence of diff lines progressively, yielding each `DiffFile`
/// and its raw unified diff text as soon as it has been completely parsed.
pub fn parse_diff_lines_with_raw<I, S>(lines: I, mut on_file: impl FnMut(DiffFile, String))
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut cur: Option<DiffFile> = None;
    let mut cur_raw = String::new();
    let mut oi: u32 = 1;
    let mut ni: u32 = 1;

    for line_ref in lines {
        let line = line_ref.as_ref();
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(c) = cur.take()
                && is_valid_diff_file(&c)
            {
                on_file(c, std::mem::take(&mut cur_raw));
            } else {
                cur_raw.clear();
            }
            cur_raw.push_str(line);
            cur_raw.push('\n');
            let (_p_a, p_b) = parse_diff_git_paths(rest);
            cur = Some(DiffFile {
                path: p_b.unwrap_or_default(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: FileStatus::Modified,
                is_binary: false,
                has_file_headers: false,
                old_bytes: None,
                new_bytes: None,
                new_line_count: 0,
                old_line_count: 0,
                new_content: Vec::new(),
                old_content: Vec::new(),
                hunks: Vec::new(),
                raw_hunks: Vec::new(),
                no_content: false,
            });
        } else if cur.is_none() {
            continue;
        } else {
            cur_raw.push_str(line);
            cur_raw.push('\n');
        }

        if let Some(mode) = line.strip_prefix("old mode ") {
            if let Some(c) = cur.as_mut() {
                c.old_mode = Some(mode.trim().to_string());
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            if let Some(c) = cur.as_mut() {
                c.new_mode = Some(mode.trim().to_string());
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if let Some(rest) = line.strip_prefix("mode change ") {
            if let Some(c) = cur.as_mut() {
                if let Some((old_m, new_m)) = rest.split_once("->") {
                    c.old_mode = Some(old_m.trim().to_string());
                    c.new_mode = Some(new_m.trim().to_string());
                } else {
                    c.new_mode = Some(rest.trim().to_string());
                }
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if line.starts_with("new file mode ") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Added;
            }
        } else if line.starts_with("deleted file mode ") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Deleted;
            }
        } else if line.starts_with("untracked file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Untracked;
            }
        } else if line.starts_with("unmodified file mode") || line == "unmodified file" {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Unmodified;
            }
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            if let Some(c) = cur.as_mut() {
                c.old_path = Some(unquote_git_path(rest));
                c.status = FileStatus::Renamed;
            }
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(c) = cur.as_mut() {
                c.path = unquote_git_path(rest);
                c.status = FileStatus::Renamed;
            }
        } else if line.starts_with("GIT binary patch")
            || line.starts_with("Binary files ")
            || line == "Binary files differ"
        {
            if let Some(c) = cur.as_mut() {
                c.is_binary = true;
                if line.contains(" /dev/null and ") {
                    c.status = FileStatus::Added;
                } else if line.contains(" and /dev/null differ") {
                    c.status = FileStatus::Deleted;
                }
                c.hunks.clear(); // binary: nothing to review
                c.raw_hunks.clear();
            }
        } else if line.starts_with("+++ ") || line.starts_with("--- ") {
            if let Some(c) = cur.as_mut() {
                c.has_file_headers = true;
            }
            let is_new = line.starts_with("+++ ");
            let p = hunk_path(if is_new { "+++" } else { "---" }, line);
            if is_new {
                if let (Some(c), Some(p)) = (cur.as_mut(), p) {
                    c.path = p;
                }
            } else if let (Some(c), Some(p)) = (cur.as_mut(), p) {
                // Deleted files have `+++ /dev/null` (no new path) — the old
                // path is the only path; renames carry it via `rename to`.
                if c.path.is_empty() {
                    c.path = p;
                }
            }
        } else if line.starts_with("@@ ") {
            if let Some((old_start, old_count, new_start, new_count, section_str)) =
                parse_hunk_header(line)
            {
                oi = old_start;
                ni = new_start;
                if let Some(c) = cur.as_mut() {
                    let section = section_str.trim().to_string();
                    c.hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        section: section.clone(),
                        rows: Vec::new(),
                        above_ctx: 0,
                        below_ctx: 0,
                    });
                    c.raw_hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        section,
                        rows: Vec::new(),
                        above_ctx: 0,
                        below_ctx: 0,
                    });
                }
            }
        } else if let Some(c) = cur.as_mut() {
            // Row lines (`+`, `-`, ` `) append to the hunk they belong to —
            // the same hunk object `cur.hunks.last_mut()` that was pushed at
            // the `@@` header (the TS holds one reference; Rust appends in
            // place).
            let hunk = match c.hunks.last_mut() {
                Some(h) => h,
                None => continue,
            };
            let raw_hunk = match c.raw_hunks.last_mut() {
                Some(h) => h,
                None => continue,
            };
            if let Some(rest) = line.strip_prefix('+') {
                let row = Row::Add(AddLine::new(ni, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                ni += 1;
                hunk.new_count = hunk.new_count.max(ni.saturating_sub(hunk.new_start));
                raw_hunk.new_count = hunk.new_count;
            } else if let Some(rest) = line.strip_prefix('-') {
                let row = Row::Del(DelLine::new(oi, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                oi += 1;
                hunk.old_count = hunk.old_count.max(oi.saturating_sub(hunk.old_start));
                raw_hunk.old_count = hunk.old_count;
            } else if let Some(rest) = line.strip_prefix(' ') {
                let row = Row::Ctx(CtxLine::new(oi, ni, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                oi += 1;
                ni += 1;
                hunk.old_count = hunk.old_count.max(oi.saturating_sub(hunk.old_start));
                hunk.new_count = hunk.new_count.max(ni.saturating_sub(hunk.new_start));
                raw_hunk.old_count = hunk.old_count;
                raw_hunk.new_count = hunk.new_count;
            } else if line.is_empty()
                && ni < hunk.new_start + hunk.new_count
                && oi < hunk.old_start + hunk.old_count
            {
                let old_num = oi.max(1);
                let new_num = ni.max(1);
                let row = Row::Ctx(CtxLine::new(old_num, new_num, ""));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                oi += 1;
                ni += 1;
                hunk.old_count = hunk.old_count.max(oi.saturating_sub(hunk.old_start));
                hunk.new_count = hunk.new_count.max(ni.saturating_sub(hunk.new_start));
                raw_hunk.old_count = hunk.old_count;
                raw_hunk.new_count = hunk.new_count;
            }
        }
    }
    if let Some(c) = cur.take()
        && is_valid_diff_file(&c)
    {
        on_file(c, cur_raw);
    }
}

/// Parse a sequence of diff lines progressively, yielding each `DiffFile`
/// as soon as it has been completely parsed.
pub fn parse_diff_lines<I, S>(lines: I, mut on_file: impl FnMut(DiffFile))
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parse_diff_lines_with_raw(lines, |f, _raw| on_file(f));
}

/// Parse a `git diff` progressively from any `io::BufRead` (such as `stdin`),
/// yielding each `DiffFile` and its raw unified diff text to a callback as soon
/// as it has been completely parsed.
pub fn parse_diff_with_raw_from_reader<R: std::io::BufRead>(
    reader: R,
    on_file: impl FnMut(DiffFile, String),
) {
    parse_diff_lines_with_raw(reader.lines().map_while(Result::ok), on_file);
}

/// Parse a `git diff` progressively, yielding each `DiffFile` to a callback
/// as soon as it has been completely parsed.
pub fn parse_diff_for_each(raw: &str, on_file: impl FnMut(DiffFile)) {
    parse_diff_lines(raw.split('\n'), on_file);
}

/// Parse a `git diff` progressively from any `io::BufRead` (such as `stdin`),
/// yielding each `DiffFile` to a callback as soon as it has been completely
/// parsed.
pub fn parse_diff_from_reader<R: std::io::BufRead>(reader: R, on_file: impl FnMut(DiffFile)) {
    parse_diff_lines(reader.lines().map_while(Result::ok), on_file);
}

/// Parse a `git diff` into files, hunks, and rows.
pub fn parse_diff(raw: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    parse_diff_for_each(raw, |f| files.push(f));
    files
}
