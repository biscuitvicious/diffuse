//! Ports of the brace-anchor helpers' behavior from test/review.test.ts:
//! `isTailBeforeClosing` and `fnOf`. The TS suite exercises them through
//! `collapseHunkRows`'s brace-anchor; these are direct checks of the
//! helpers themselves.

use crate::tail::{fn_of, is_tail_before_closing};

// The tail's closing brace sits one indent level down from the fn's own
// body lines (`    }` after `        None` / `        Some(result)`) —
// not the inner block's brace after a nested statement.
#[test]
fn tail_before_closing_brace() {
    let content: Vec<String> = vec!["        None".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(0), true, Some(&content), None),
        "the fn's closing brace after the tail expression"
    );
    let inner: Vec<String> = vec!["        if x {".into(), "        }".into()];
    assert!(
        !is_tail_before_closing(Some(0), true, Some(&inner), None),
        "an inner block's brace at the same indent is NOT the tail"
    );
    let no_brace: Vec<String> = vec!["        None".into(), "        let x = 1;".into()];
    assert!(
        !is_tail_before_closing(Some(0), true, Some(&no_brace), None),
        "no closing brace → not a tail"
    );
    // A null line, or content without that line, is never a tail.
    assert!(!is_tail_before_closing(None, true, Some(&content), None));
    assert!(!is_tail_before_closing(Some(5), true, Some(&content), None));
    // The new side reads the new content.
    let new_content: Vec<String> = vec!["        Some(result)".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(0), false, None, Some(&new_content)),
        "new-side tail check"
    );
}

// The nearest PRECEDING `fn` signature owns the line (the walk starts at
// `line - 2`; the blank line between fn bodies is why the nearest fn is
// the right owner — the code itself does not stop at blanks).
#[test]
fn fn_of_finds_the_nearest_preceding_fn() {
    let content: Vec<String> = vec![
        "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {".into(),
        "        let _ = key;".into(),
        "        None".into(),
        "    }".into(),
        "".into(),
        "    pub fn insert(&self, key: &CacheKey) {".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(2)),
        Some("lookup".to_string()),
        "the tail belongs to lookup"
    );
    // The line AFTER the fn's closing brace (the blank) still resolves to
    // the nearest preceding signature — the walk does not stop at blanks.
    assert_eq!(
        fn_of(Some(&content), Some(5)),
        Some("lookup".to_string()),
        "the nearest preceding fn, not the one at the line itself"
    );
    // Nothing precedes the first two lines (the walk starts at line - 2).
    assert_eq!(fn_of(Some(&content), Some(0)), None);
    assert_eq!(fn_of(Some(&content), Some(1)), None);
    // No content / null line → null.
    assert_eq!(fn_of(None, Some(2)), None);
    assert_eq!(fn_of(Some(&content), None), None);
    // No preceding signature → null.
    let bare: Vec<String> = vec!["let x = 1;".into()];
    assert_eq!(fn_of(Some(&bare), Some(0)), None);
}
