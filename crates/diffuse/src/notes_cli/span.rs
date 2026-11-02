//! Line span parsing, bounds checking, and coordinate relocation algorithms.

use std::process;

use diffuse::{notes::DiffCoord, prelude::*};

/// Parse a line or inclusive range `N` or `N-M` into a `(start, end)` span.
pub fn parse_line_span(spec: &str, ctx: &str) -> (u32, u32) {
    let (s_str, e_str) = match spec.split_once('-') {
        Some((a, b)) => (a, b),
        None => (spec, spec),
    };
    let s: u32 = s_str.parse().unwrap_or_else(|_| {
        eprintln!("diffuse: bad line number '{s_str}' in {ctx}");
        process::exit(1);
    });
    let e: u32 = e_str.parse().unwrap_or_else(|_| {
        eprintln!("diffuse: bad line number '{e_str}' in {ctx}");
        process::exit(1);
    });
    if s == 0 || e == 0 {
        eprintln!("diffuse: line coordinates must be 1 or greater in {ctx}");
        process::exit(1);
    }
    (s.min(e), s.max(e))
}

/// A plain synthetic context row used as a note's provisional stored row
/// while the definitive rows are located in the pipeline output.
pub(crate) fn provisional_ctx_row(old: Option<u32>, new: Option<u32>, text: &str) -> OutputRow {
    OutputRow {
        kind: RowKind::Ctx,
        mov_side: None,
        old_line: old,
        new_line: new,
        runs: vec![OutputRun::new(RunKind::Ctx, text.to_string())],
        wholesale: false,
        group: None,
    }
}

/// Relocate a coordinate span onto actual output rows: the window of rows
/// whose numbers cover `old_span` and `new_span`. The two spans must be
/// equally long (validated by the caller). Returns `None` when the rows do
/// not cover the span (count or boundary mismatch).
///
/// The span is located on its key side: the new side when `new_span` is
/// given, otherwise the old side. When both spans are given they act as a
/// two-sided exact coordinate match: the located boundary rows must carry
/// the requested old numbers.
pub(crate) fn relocate_span(
    rows: &[&OutputRow],
    old_span: Option<(u32, u32)>,
    new_span: Option<(u32, u32)>,
) -> Option<Vec<OutputRow>> {
    let (key_is_old, span, both) = match (old_span, new_span) {
        (Some(_), Some(new)) => (false, new, true),
        (None, Some(new)) => (false, new, false),
        (Some(old), None) => (true, old, false),
        (None, None) => return None,
    };
    let (start, end) = span;
    if start > end {
        return None;
    }

    let first_idx = if both {
        let (os, _) = old_span.unwrap();
        rows.iter()
            .position(|r| r.old_line == Some(os) && r.new_line == Some(start))?
    } else if key_is_old {
        rows.iter().position(|r| r.old_line == Some(start))?
    } else {
        rows.iter().position(|r| r.new_line == Some(start))?
    };

    let last_idx = if both {
        let (_, oe) = old_span.unwrap();
        rows[first_idx..]
            .iter()
            .position(|r| r.old_line == Some(oe) && r.new_line == Some(end))
            .map(|offset| first_idx + offset)?
    } else if key_is_old {
        rows[first_idx..]
            .iter()
            .position(|r| r.old_line == Some(end))
            .map(|offset| first_idx + offset)?
    } else {
        rows[first_idx..]
            .iter()
            .position(|r| r.new_line == Some(end))
            .map(|offset| first_idx + offset)?
    };

    let start_coord = DiffCoord::from_row(rows[first_idx])?;
    let end_coord = DiffCoord::from_row(rows[last_idx])?;

    let window: Vec<&OutputRow> = rows[first_idx..=last_idx]
        .iter()
        .copied()
        .filter(|r| DiffCoord::row_in_span(r, start_coord, end_coord))
        .collect();

    // Verify all requested lines on the key side (or both sides) are present and in
    // order
    if key_is_old {
        let old_lines: Vec<u32> = window.iter().filter_map(|r| r.old_line).collect();
        if old_lines != (start..=end).collect::<Vec<_>>() {
            return None;
        }
    } else {
        let new_lines: Vec<u32> = window.iter().filter_map(|r| r.new_line).collect();
        if new_lines != (start..=end).collect::<Vec<_>>() {
            return None;
        }
        if both {
            let (os, oe) = old_span.unwrap();
            let old_lines: Vec<u32> = window.iter().filter_map(|r| r.old_line).collect();
            if old_lines != (os..=oe).collect::<Vec<_>>() {
                return None;
            }
        }
    }

    Some(window.into_iter().cloned().collect())
}

/// Exit when a file is not present in the reviewed content snapshots.
pub fn missing_file_exit(path: &str) -> ! {
    eprintln!("diffuse: file '{path}' is not part of the current diff");
    process::exit(1);
}

/// Reject spans running past the tracked content lines.
pub fn check_span_bounds(sp: (u32, u32), lines: &[&str], path: &str) {
    if sp.0 as usize > lines.len() || sp.1 as usize > lines.len() {
        eprintln!(
            "diffuse: target range {}-{} exceeds the {} lines of '{path}'",
            sp.0,
            sp.1,
            lines.len()
        );
        process::exit(1);
    }
}
