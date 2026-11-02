use diffuse_parse::Hunk;
use gix::bstr::ByteSlice;

use crate::diff::split_lines;

/// Apply a hunk to an in-memory buffer, either forward (`reverse = false`)
/// or in reverse (`reverse = true`).
pub fn apply_hunk_to_buffer(buffer: &[u8], hunk: &Hunk, reverse: bool) -> Result<Vec<u8>, String> {
    let buffer_lines = split_lines(buffer);

    // Detect line endings from the original buffer (default to "\n").
    let eol = if buffer.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let (nominal_start, expected_texts, replacement_lines) = if !reverse {
        let nominal = hunk.old_start.saturating_sub(1) as usize;
        let mut exp = Vec::new();
        let mut rep = Vec::new();
        for row in &hunk.rows {
            if let Some(t) = row.old_text() {
                exp.push(t);
            }
            if let Some(t) = row.new_text() {
                rep.push(format!("{}{eol}", t));
            }
        }
        (nominal, exp, rep)
    } else {
        let nominal = hunk.new_start.saturating_sub(1) as usize;
        let mut exp = Vec::new();
        let mut rep = Vec::new();
        for row in &hunk.rows {
            if let Some(t) = row.new_text() {
                exp.push(t);
            }
            if let Some(t) = row.old_text() {
                rep.push(format!("{}{eol}", t));
            }
        }
        (nominal, exp, rep)
    };

    // If buffer is empty and we expect nothing, replacement lines form the entire
    // output.
    if buffer_lines.is_empty() && expected_texts.is_empty() {
        let mut out = Vec::new();
        for r in &replacement_lines {
            out.extend_from_slice(r.as_bytes());
        }
        return Ok(out);
    }

    let matched_start = find_hunk_match(&buffer_lines, nominal_start, &expected_texts)
        .ok_or_else(|| "hunk does not apply: context mismatch".to_string())?;

    let mut new_bytes = Vec::new();
    for &l in &buffer_lines[..matched_start] {
        new_bytes.extend_from_slice(l);
    }
    for r in &replacement_lines {
        new_bytes.extend_from_slice(r.as_bytes());
    }
    for &l in &buffer_lines[matched_start + expected_texts.len()..] {
        new_bytes.extend_from_slice(l);
    }

    Ok(new_bytes)
}

fn find_hunk_match(
    buffer_lines: &[&[u8]],
    nominal_start: usize,
    expected: &[&str],
) -> Option<usize> {
    if expected.is_empty() {
        return Some(nominal_start.min(buffer_lines.len()));
    }

    let matches_at = |start: usize| -> bool {
        if start + expected.len() > buffer_lines.len() {
            return false;
        }
        for (i, &exp) in expected.iter().enumerate() {
            let buf_line = buffer_lines[start + i].trim_end();
            if buf_line != exp.as_bytes().trim_end() {
                return false;
            }
        }
        true
    };

    if matches_at(nominal_start) {
        return Some(nominal_start);
    }

    let max_offset = buffer_lines.len();
    for offset in 1..=max_offset {
        if nominal_start >= offset && matches_at(nominal_start - offset) {
            return Some(nominal_start - offset);
        }
        if nominal_start + offset < buffer_lines.len() && matches_at(nominal_start + offset) {
            return Some(nominal_start + offset);
        }
    }

    None
}
