use std::env;

use clap::Parser;

use diffuse::DiffuseTheme;

use super::{Args, route_mode};

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
    unsafe { env::remove_var("NO_COLOR") };
    let enabled = super::init_color_mode("auto", false);
    assert!(enabled);
    assert!(yansi::is_enabled());
}

#[test]
fn test_init_color_mode_auto_with_no_color_env() {
    unsafe { env::set_var("NO_COLOR", "1") };
    let enabled = super::init_color_mode("auto", false);
    assert!(!enabled);
    assert!(!yansi::is_enabled());
    unsafe { env::remove_var("NO_COLOR") };
}

#[test]
fn test_cli_args_no_color() {
    let args = Args::try_parse_from(["diffuse", "--no-color", "--pager"]).unwrap();
    assert!(args.no_color);
    assert!(args.pager);
}

#[test]
fn test_cli_args_theme_flag() {
    let args = Args::try_parse_from(["diffuse", "--theme", "dark"]).unwrap();
    assert_eq!(args.theme.as_deref(), Some("dark"));
}

#[test]
fn test_resolve_theme_prefers_cli_flag() {
    unsafe { env::remove_var("DIFFUSE_THEME") };
    assert_eq!(super::resolve_theme(Some("dark")), DiffuseTheme::DEFAULT);
    assert_eq!(super::resolve_theme(None), DiffuseTheme::from_env());
}

#[test]
fn test_resolve_theme_falls_back_on_unknown_name() {
    unsafe { env::remove_var("DIFFUSE_THEME") };
    assert_eq!(
        super::resolve_theme(Some("bogus-theme")),
        DiffuseTheme::DEFAULT
    );
}

#[test]
fn test_cli_args_jobs_flags() {
    let args1 = Args::try_parse_from(["diffuse", "-j", "4"]).unwrap();
    assert_eq!(args1.jobs, Some(4));

    let args2 = Args::try_parse_from(["diffuse", "--jobs", "8"]).unwrap();
    assert_eq!(args2.jobs, Some(8));

    let args3 = Args::try_parse_from(["diffuse", "--threads", "2"]).unwrap();
    assert_eq!(args3.jobs, Some(2));

    let args4 = Args::try_parse_from(["diffuse", "--workers", "6"]).unwrap();
    assert_eq!(args4.jobs, Some(6));

    let args_default = Args::try_parse_from(["diffuse"]).unwrap();
    assert_eq!(args_default.jobs, None);
}
