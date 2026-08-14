//! Unit tests for diff identity shift transformations.

use super::*;
use crate::parse::{AddLine, DelLine, Hunk, LineNum, Row};

fn hunk(rows: Vec<Row>) -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        section: String::new(),
        rows,
        above_ctx: 0,
        below_ctx: 0,
    }
}

#[test]
fn test_max_shift_down_rows() {
    let h = hunk(vec![
        Row::Del(DelLine::new(LineNum::new(10).unwrap(), "old line")),
        Row::Add(AddLine::new(LineNum::new(20).unwrap(), "line 1")),
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    }")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "")),
        Row::Add(AddLine::new(
            LineNum::new(23).unwrap(),
            "    pub fn next() {}",
        )),
        Row::Add(AddLine::new(LineNum::new(24).unwrap(), "    }")),
    ]);

    let mut new_content = vec![String::new(); 30];
    new_content[19] = "line 1".to_string();
    new_content[20] = "    }".to_string();
    new_content[21] = "".to_string();
    new_content[22] = "    pub fn next() {}".to_string();
    new_content[23] = "    }".to_string();
    new_content[24] = "    }".to_string();
    new_content[25] = "".to_string();

    let max_rows = max_shift_down_rows(&h, 2, 6, Some(&new_content));
    assert_eq!(
        max_rows, 2,
        "Should be able to shift 2 rows (closing brace and blank line)"
    );
}

#[test]
fn test_apply_shift_down_identity() {
    let mut h = hunk(vec![
        Row::Del(DelLine::new(LineNum::new(10).unwrap(), "old line")),
        Row::Add(AddLine::new(LineNum::new(20).unwrap(), "line 1")),
        Row::Add(AddLine::new(LineNum::new(21).unwrap(), "    }")),
        Row::Add(AddLine::new(LineNum::new(22).unwrap(), "")),
        Row::Add(AddLine::new(
            LineNum::new(23).unwrap(),
            "    pub fn next() {}",
        )),
    ]);

    apply_shift_down_identity(&mut h, 2, 5, 2);

    assert_eq!(h.rows.len(), 5);
    assert_eq!(h.rows[1].new_line(), Some(20));
    assert_eq!(h.rows[2].new_line(), Some(23));
    assert_eq!(h.rows[3].new_line(), Some(24));
    assert_eq!(h.rows[3].text(), "    }");
    assert_eq!(h.rows[4].new_line(), Some(25));
    assert_eq!(h.rows[4].text(), "");
}
