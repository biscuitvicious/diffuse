//! Tests for scope and brace-anchor helpers: `is_tail_before_closing` and
//! `fn_of`.

use crate::{
    scope::{fn_of, is_tail_before_closing},
    syntax::registry::lang_profile,
};

#[test]
fn tail_before_closing_brace() {
    let profile = lang_profile("rs");
    let content: &[&str] = &["        None", "    }"];
    assert!(
        is_tail_before_closing(Some(1), true, Some(content), None, profile),
        "the fn's closing brace after the tail expression"
    );
    let inner: &[&str] = &["        if x {", "        }"];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(inner), None, profile),
        "an inner block's brace at the same indent is NOT the tail"
    );
    let no_brace: &[&str] = &["        None", "        let x = 1;"];
    assert!(
        !is_tail_before_closing(Some(1), true, Some(no_brace), None, profile),
        "no closing brace → not a tail"
    );
    // Without a profile, no syntax processing happens -> returns false.
    assert!(!is_tail_before_closing(
        Some(1),
        true,
        Some(content),
        None,
        None
    ));
    // A null line, or content without that line, is never a tail.
    assert!(!is_tail_before_closing(
        None,
        true,
        Some(content),
        None,
        profile
    ));
    assert!(!is_tail_before_closing(
        Some(5),
        true,
        Some(content),
        None,
        profile
    ));
    // The new side reads the new content.
    let new_content: &[&str] = &["        Some(result)", "    }"];
    assert!(
        is_tail_before_closing(Some(1), false, None, Some(new_content), profile),
        "new-side tail check"
    );
}

#[test]
fn test_unindented_shell_and_two_space_tail() {
    let sh_profile = lang_profile("sh");
    // Unindented shell script:
    // if -f foo.txt; then
    // echo Foo!
    // fi
    let sh_content: &[&str] = &["if -f foo.txt; then", "echo Foo!", "fi"];
    assert!(
        is_tail_before_closing(Some(2), true, Some(sh_content), None, sh_profile),
        "echo Foo! before fi in unindented shell script must be recognized as tail"
    );

    // 2-space indented TypeScript:
    // function foo() {
    //   return 42;
    // }
    let ts_profile = lang_profile("ts");
    let ts_content: &[&str] = &["function foo() {", "  return 42;", "}"];
    assert!(
        is_tail_before_closing(Some(2), true, Some(ts_content), None, ts_profile),
        "2-space indented return before closing brace must be recognized as tail"
    );
}

#[test]
fn fn_of_finds_the_nearest_preceding_fn() {
    let profile = lang_profile("rs");
    let content: &[&str] = &[
        "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {",
        "        let _ = key;",
        "        None",
        "    }",
        "",
        "    pub fn insert(&self, key: &CacheKey) {",
    ];
    assert_eq!(
        fn_of(Some(content), Some(1), profile),
        Some("lookup".to_string()),
        "the signature itself belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(content), Some(2), profile),
        Some("lookup".to_string()),
        "the body line belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(content), Some(3), profile),
        Some("lookup".to_string()),
        "the tail belongs to lookup"
    );
    assert_eq!(
        fn_of(Some(content), Some(6), profile),
        Some("insert".to_string()),
        "the next signature belongs to insert"
    );
    assert_eq!(fn_of(Some(content), Some(0), profile), None);
    assert_eq!(fn_of(Some(content), Some(5), profile), None);
    assert_eq!(fn_of(None, Some(2), profile), None);
    assert_eq!(fn_of(Some(content), None, profile), None);
    let bare: &[&str] = &["let x = 1;"];
    assert_eq!(fn_of(Some(bare), Some(0), profile), None);
}

#[test]
fn fn_of_python() {
    let profile = lang_profile("py");
    let content: &[&str] = &[
        "def greet(name):",
        "    return f\"hello {name}\"",
        "",
        "class Foo:",
        "    def bar(self):",
        "        pass",
    ];
    assert_eq!(
        fn_of(Some(content), Some(1), profile),
        Some("greet".to_string()),
        "def line belongs to greet"
    );
    assert_eq!(
        fn_of(Some(content), Some(2), profile),
        Some("greet".to_string()),
        "body line inside greet"
    );
    assert_eq!(
        fn_of(Some(content), Some(5), profile),
        Some("bar".to_string()),
        "method signature bar"
    );
    assert_eq!(
        fn_of(Some(content), Some(6), profile),
        Some("bar".to_string()),
        "body line inside method bar"
    );
}

#[test]
fn fn_of_typescript() {
    let profile = lang_profile("ts");
    let content: &[&str] = &[
        "export function buildIndex(items: Item[]): Index {",
        "    return items.reduce((acc, x) => acc, {});",
        "}",
        "",
        "async function fetchData(url: string) {",
        "    const resp = await fetch(url);",
    ];
    assert_eq!(
        fn_of(Some(content), Some(1), profile),
        Some("buildIndex".to_string()),
        "body of buildIndex"
    );
    assert_eq!(
        fn_of(Some(content), Some(5), profile),
        Some("fetchData".to_string()),
        "body of fetchData"
    );
}

#[test]
fn fn_of_go() {
    let profile = lang_profile("go");
    let content: &[&str] = &[
        "func (r *Repo) Lookup(key string) (string, bool) {",
        "    v, ok := r.m[key]",
        "    return v, ok",
        "}",
        "",
        "func NewRepo() *Repo {",
        "    return &Repo{m: make(map[string]string)}",
    ];
    assert_eq!(
        fn_of(Some(content), Some(2), profile),
        Some("Lookup".to_string()),
        "inside Lookup method"
    );
    assert_eq!(
        fn_of(Some(content), Some(6), profile),
        Some("NewRepo".to_string()),
        "inside NewRepo"
    );
}

#[test]
fn fn_of_c() {
    let profile = lang_profile("c");
    let content: &[&str] = &[
        "int compute_checksum(const char *data, size_t len) {",
        "    int sum = 0;",
        "    return sum;",
        "}",
    ];
    assert_eq!(
        fn_of(Some(content), Some(1), profile),
        Some("compute_checksum".to_string()),
        "inside compute_checksum in C"
    );
}

#[test]
fn fn_of_shell() {
    let profile = lang_profile("sh");
    let content: &[&str] = &["function deploy() {", "    echo \"deploying...\"", "}"];
    assert_eq!(
        fn_of(Some(content), Some(1), profile),
        Some("deploy".to_string()),
        "inside deploy function in shell"
    );
}

#[test]
fn test_fn_body_lines() {
    let rs = lang_profile("rs");
    let content: &[&str] = &[
        "fn alpha() {",
        "    let x = 1;",
        "    let y = 2;",
        "}",
        "",
        "fn beta() {",
        "    let z = 3;",
        "}",
    ];
    let alpha_lines = crate::scope::fn_body_lines(Some(content), "alpha", rs);
    assert_eq!(alpha_lines, Some(&content[0..4]));
    let beta_lines = crate::scope::fn_body_lines(Some(content), "beta", rs);
    assert_eq!(beta_lines, Some(&content[5..8]));
    assert_eq!(
        crate::scope::fn_body_lines(Some(content), "nonexistent", rs),
        None
    );
}
