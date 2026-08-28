use super::route_mode;

#[test]
fn extension_json_payload_is_json_mode() {
    // The pi extension pipes a JSON payload with no git args.
    assert!(route_mode(
        false,
        false,
        None,
        &[],
        false,
        "{\"diff\":\"x\"}"
    ));
}

#[test]
fn piped_raw_diff_stays_json_mode() {
    // `git diff | diffuse` — a raw (non-JSON) payload piped in.
    assert!(route_mode(
        false,
        false,
        None,
        &[],
        false,
        "diff --git a b\n"
    ));
}

#[test]
fn empty_piped_stdin_is_standalone() {
    // `diffuse < /dev/null | cat` — no payload, must route to the
    // standalone reviewer, never the JSON contract.
    assert!(!route_mode(false, false, None, &[], false, ""));
}

#[test]
fn interactive_stdin_is_standalone() {
    assert!(!route_mode(false, false, None, &[], true, ""));
}

#[test]
fn explicit_json_flag_is_json_mode() {
    assert!(route_mode(true, false, None, &[], true, ""));
}

#[test]
fn explicit_jsonl_flag_is_json_mode() {
    assert!(route_mode(false, true, None, &[], true, ""));
}

#[test]
fn diff_file_flag_is_json_mode() {
    assert!(route_mode(false, false, Some("x.diff"), &[], true, ""));
}

#[test]
fn git_args_are_always_standalone() {
    // `diffuse HEAD < file` — git args win over piped stdin content.
    assert!(!route_mode(
        false,
        false,
        None,
        &["HEAD".into()],
        false,
        "noise"
    ));
}
