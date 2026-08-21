//! Tests for scope and brace-anchor helpers: `is_tail_before_closing` and
//! `fn_of`.

use crate::{
    scope::{fn_of, is_tail_before_closing},
    syntax::registry::lang_profile,
};

#[test]
fn tail_before_closing_brace() {
    let profile = lang_profile("rs");
    let content: Vec<String> = vec!["        None".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(1), true, Some(&content), None, profile),
        "the fn's closing brace after the tail expression"
    );
    let inner: Vec<String> = vec!["        if x {".into(), "        }".into()];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(&inner), None, profile),
        "an inner block's brace at the same indent is NOT the tail"
    );
    let no_brace: Vec<String> = vec!["        None".into(), "        let x = 1;".into()];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(&no_brace), None, profile),
        "no closing brace → not a tail"
    );
    // Without a profile, no syntax processing happens -> returns false.
    assert!(!is_tail_before_closing(
        Some(1),
        true,
        Some(&content),
        None,
        None
    ));
    // A null line, or content without that line, is never a tail.
    assert!(!is_tail_before_closing(
        None,
        true,
        Some(&content),
        None,
        profile
    ));
    assert!(!is_tail_before_closing(
        Some(5),
        true,
        Some(&content),
        None,
        profile
    ));
    // The new side reads the new content.
    let new_content: Vec<String> = vec!["        Some(result)".into(), "    }".into()];
    assert!(
        is_tail_before_closing(Some(1), false, None, Some(&new_content), profile),
        "new-side tail check"
    );
}

#[test]
fn fn_of_finds_the_nearest_preceding_fn() {
    let profile = lang_profile("rs");
    let content: Vec<String> = vec![
        "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {".into(),
        "        let _ = key;".into(),
        "        None".into(),
        "    }".into(),
        "".into(),
        "    pub fn insert(&self, key: &CacheKey) {".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(1), profile),
        Some("lookup".to_string()),
        "the signature itself belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(&content), Some(2), profile),
        Some("lookup".to_string()),
        "the body line belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(&content), Some(3), profile),
        Some("lookup".to_string()),
        "the tail belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(&content), Some(6), profile),
        Some("insert".to_string()),
        "the next signature belongs to insert"
    );
    assert_eq!(fn_of(Some(&content), Some(0), profile), None);
    assert_eq!(fn_of(Some(&content), Some(5), profile), None);
    assert_eq!(fn_of(None, Some(2), profile), None);
    assert_eq!(fn_of(Some(&content), None, profile), None);
    let bare: Vec<String> = vec!["let x = 1;".into()];
    assert_eq!(fn_of(Some(&bare), Some(0), profile), None);
}

#[test]
fn fn_of_python() {
    let profile = lang_profile("py");
    let content: Vec<String> = vec![
        "def greet(name):".into(),
        "    return f\"hello {name}\"".into(),
        "".into(),
        "class Foo:".into(),
        "    def bar(self):".into(),
        "        pass".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(1), profile),
        Some("greet".to_string()),
        "def line belongs to greet"
    );
    assert_eq!(
        fn_of(Some(&content), Some(2), profile),
        Some("greet".to_string()),
        "body line inside greet"
    );
    assert_eq!(
        fn_of(Some(&content), Some(5), profile),
        Some("bar".to_string()),
        "method signature bar"
    );
    assert_eq!(
        fn_of(Some(&content), Some(6), profile),
        Some("bar".to_string()),
        "body line inside method bar"
    );
}

#[test]
fn fn_of_typescript() {
    let profile = lang_profile("ts");
    let content: Vec<String> = vec![
        "export function buildIndex(items: Item[]): Index {".into(),
        "    return items.reduce((acc, x) => acc, {});".into(),
        "}".into(),
        "".into(),
        "async function fetchData(url: string) {".into(),
        "    const resp = await fetch(url);".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(1), profile),
        Some("buildIndex".to_string()),
        "body of buildIndex"
    );
    assert_eq!(
        fn_of(Some(&content), Some(5), profile),
        Some("fetchData".to_string()),
        "body of fetchData"
    );
}

#[test]
fn fn_of_go() {
    let profile = lang_profile("go");
    let content: Vec<String> = vec![
        "func (r *Repo) Lookup(key string) (string, bool) {".into(),
        "    v, ok := r.m[key]".into(),
        "    return v, ok".into(),
        "}".into(),
        "".into(),
        "func NewRepo() *Repo {".into(),
        "    return &Repo{m: make(map[string]string)}".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(2), profile),
        Some("Lookup".to_string()),
        "inside Lookup method"
    );
    assert_eq!(
        fn_of(Some(&content), Some(6), profile),
        Some("NewRepo".to_string()),
        "inside NewRepo"
    );
}

#[test]
fn fn_of_c() {
    let profile = lang_profile("c");
    let content: Vec<String> = vec![
        "int compute_checksum(const char *data, size_t len) {".into(),
        "    int sum = 0;".into(),
        "    return sum;".into(),
        "}".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(1), profile),
        Some("compute_checksum".to_string()),
        "inside compute_checksum in C"
    );
}

#[test]
fn fn_of_shell() {
    let profile = lang_profile("sh");
    let content: Vec<String> = vec![
        "function deploy() {".into(),
        "    echo \"deploying...\"".into(),
        "}".into(),
    ];
    assert_eq!(
        fn_of(Some(&content), Some(1), profile),
        Some("deploy".to_string()),
        "inside deploy function in shell"
    );
}
