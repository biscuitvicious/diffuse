//! Terminal theme resolution and color mode initialization.

use std::env;

use diffuse::prelude::DiffuseTheme;

/// Resolves the diffuse color theme based on CLI override, environment, and
/// terminal capability.
pub fn resolve_theme(cli_theme: Option<&str>, interactive: bool) -> DiffuseTheme {
    if let Some(name) = cli_theme {
        if name.trim().eq_ignore_ascii_case("auto") {
            return if interactive {
                DiffuseTheme::auto_detect_interactive()
            } else {
                DiffuseTheme::auto_detect()
            };
        }
        if let Some(theme) = DiffuseTheme::from_name(name) {
            return theme;
        }
        eprintln!("diffuse: unknown theme '{name}', falling back to default theme");
    }
    if interactive {
        DiffuseTheme::from_env_interactive()
    } else {
        DiffuseTheme::from_env()
    }
}

/// Configure global ANSI styling condition via `yansi` from CLI flags and
/// environment.
pub fn init_color_mode(color: &str, no_color: bool) -> bool {
    let no_color_env = env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if no_color || color == "never" || (color == "auto" && no_color_env) {
        yansi::disable();
        false
    } else {
        yansi::enable();
        true
    }
}
