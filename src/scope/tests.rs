//! Tests for scope and brace-anchor helpers: `is_tail_before_closing` and
//! `fn_of`.

use crate::scope::{fn_of, is_tail_before_closing};

#[test]
fn tail_before_closing_brace() {
    let content: Vec<String> = vec!["        None".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(1), true, Some(&content), None),
        "the fn's closing brace after the tail expression"
    );
    let inner: Vec<String> = vec!["        if x {".into(), "        }".into()];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(&inner), None),
        "an inner block's brace at the same indent is NOT the tail"
    );
    let no_brace: Vec<String> = vec!["        None".into(), "        let x = 1;".into()];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(&no_brace), None),
        "no closing brace → not a tail"
    );
    // A null line, or content without that line, is never a tail.
    assert!(!is_tail_before_closing(None, true, Some(&content), None));
    assert!(!is_tail_before_closing(Some(5), true, Some(&content), None));
    // The new side reads the new content.
    let new_content: Vec<String> = vec!["        Some(result)".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(1), false, None, Some(&new_content)),
        "new-side tail check"
    );
}

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
    assert_eq!(
        fn_of(Some(&content), Some(5)),
        Some("lookup".to_string()),
        "the nearest preceding fn, not the one at the line itself"
    );
    assert_eq!(fn_of(Some(&content), Some(0)), None);
    assert_eq!(fn_of(Some(&content), Some(1)), None);
    assert_eq!(fn_of(None, Some(2)), None);
    assert_eq!(fn_of(Some(&content), None), None);
    let bare: Vec<String> = vec!["let x = 1;".into()];
    assert_eq!(fn_of(Some(&bare), Some(0)), None);
}
