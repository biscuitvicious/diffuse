//! Typed errors for unified diff parsing.
//!
//! The parser refuses hunks whose `@@` header claims a different number of
//! old/new lines than the rows their body actually carries. Header counts
//! are trusted extents: hunk merging, note anchoring, and wire headers all
//! derive spans from them, so a mismatch means the input is corrupt (a
//! truncated stream, a hand edit, or a buggy generator) and rendering it
//! would silently anchor content to wrong line numbers.

use super::file::DiffFile;

/// A hunk whose header counts disagree with its parsed rows.
///
/// Well-formed unified diffs always balance exactly: every `Ctx` row consumes
/// one old and one new line, every `Del` row one old line, and every `Add`
/// row one new line. A blank input line is tolerated as an empty context
/// row (the scar trailing-whitespace stripping leaves on empty ` ` lines),
/// and consumes one line on each side exactly like any other context row.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "diff hunk header claims {declared_old} old / {declared_new} new lines but the body carries {actual_old} old / {actual_new} new lines in '{path}' at @@ -{old_start} +{new_start} @@"
)]
pub struct HunkCountMismatch {
    /// Repository-relative path of the file carrying the corrupt hunk.
    pub path: String,
    /// Old-side start line of the corrupt hunk header.
    pub old_start: u32,
    /// New-side start line of the corrupt hunk header.
    pub new_start: u32,
    /// Old-side line count declared by the `@@` header.
    pub declared_old: u32,
    /// New-side line count declared by the `@@` header.
    pub declared_new: u32,
    /// Old-side lines the hunk body actually carries.
    pub actual_old: u32,
    /// New-side lines the hunk body actually carries.
    pub actual_new: u32,
}

/// Verify one hunk's declared counts against the rows its body parsed into.
///
/// Consumed extents derive from row line numbers (presence of `old_line`
/// counts an old-side row, presence of `new_line` a new-side row), so the
/// check holds for any row representation, pre- or post-collapse.
pub fn check_hunk(
    path: &str,
    old_start: u32,
    new_start: u32,
    declared_old: u32,
    declared_new: u32,
    rows_old: u32,
    rows_new: u32,
) -> Result<(), HunkCountMismatch> {
    if rows_old == declared_old && rows_new == declared_new {
        Ok(())
    } else {
        Err(HunkCountMismatch {
            path: path.to_string(),
            old_start,
            new_start,
            declared_old,
            declared_new,
            actual_old: rows_old,
            actual_new: rows_new,
        })
    }
}

/// Verify every hunk in a materialized file against its declared counts.
///
/// Used both while parsing (at each hunk close) and as a pipeline invariant
/// over programmatically constructed [`DiffFile`] values, which never pass
/// through the parser.
pub fn check_hunk_counts(file: &DiffFile) -> Result<(), HunkCountMismatch> {
    for h in &file.hunks {
        let actual_old = h.rows.iter().filter(|r| r.old_line().is_some()).count() as u32;
        let actual_new = h.rows.iter().filter(|r| r.new_line().is_some()).count() as u32;
        check_hunk(
            &file.path,
            h.old_start,
            h.new_start,
            h.old_count,
            h.new_count,
            actual_old,
            actual_new,
        )?;
    }
    Ok(())
}
