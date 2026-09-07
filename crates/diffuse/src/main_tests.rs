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

#[test]
fn test_init_color_mode_always() {
    let enabled = super::init_color_mode("always", false);
    assert!(enabled);
    assert!(yansi::is_enabled());
}

#[test]
fn test_init_color_mode_never() {
    let enabled = super::init_color_mode("never", false);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
}

#[test]
fn test_init_color_mode_no_color_flag() {
    let enabled = super::init_color_mode("always", true);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
}

#[test]
fn test_init_color_mode_auto_default() {
    unsafe { std::env::remove_var("NO_COLOR") };
    let enabled = super::init_color_mode("auto", false);
    assert!(enabled);
    assert!(yansi::is_enabled());
}

#[test]
fn test_init_color_mode_auto_with_no_color_env() {
    unsafe { std::env::set_var("NO_COLOR", "1") };
    let enabled = super::init_color_mode("auto", false);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
    unsafe { std::env::remove_var("NO_COLOR") };
}

#[test]
fn test_cli_args_no_color() {
    use clap::Parser;
    let args = super::Args::try_parse_from(["diffuse", "--no-color", "--pager"]).unwrap();
    assert!(args.no_color);
    assert!(args.pager);
}
