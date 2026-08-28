use super::*;

#[test]
fn test_resolve_vim_single_line() {
    let (args, detached) = resolve_editor_args("vim", "src/main.rs", 42, None);
    assert_eq!(args, vec!["vim", "+42", "src/main.rs"]);
    assert!(!detached);
}

#[test]
fn test_resolve_vim_range() {
    let (args, detached) = resolve_editor_args("nvim", "src/main.rs", 42, Some((10, 15)));
    assert_eq!(
        args,
        vec!["nvim", "-c", "10G", "-c", "normal! V5j", "src/main.rs"]
    );
    assert!(!detached);
}

#[test]
fn test_resolve_code_detached() {
    let (args, detached) = resolve_editor_args("code", "src/main.rs", 100, None);
    assert_eq!(args, vec!["code", "--goto", "src/main.rs:100:1"]);
    assert!(detached);
}

#[test]
fn test_resolve_helix() {
    let (args, detached) = resolve_editor_args("hx", "src/lib.rs", 25, None);
    assert_eq!(args, vec!["hx", "src/lib.rs:25"]);
    assert!(!detached);
}
