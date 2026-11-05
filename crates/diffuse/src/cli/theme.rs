//! Terminal theme resolution and color mode initialization.

use std::env;

use diffuse::{prelude::DiffuseTheme, theme::load_theme};

/// Where the resolved theme will be rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeTarget {
    /// The interactive TUI, which owns the terminal for its whole lifetime.
    Tui,
    /// One-shot pager output printed to the terminal the user is reading.
    Pager,
    /// Pipes, redirects, and machine-readable output.
    Batch,
}

/// Resolves the diffuse color theme based on CLI override, environment, and
/// terminal capability.
///
/// A CLI value resolves through the 3-tier loader: an existing file path
/// loads directly, builtin names (`dark`, `default`, `black`, `light`,
/// `white`) map to the bundled assets, and any other value searches the
/// XDG config directories (`$XDG_CONFIG_HOME/diffuse/themes/`,
/// `~/.config/diffuse/themes/<name>.toml`). Unresolvable or malformed
/// themes degrade gracefully to the default with an informative error.
///
/// `auto` consults the environment in every target; only [`ThemeTarget::Tui`]
/// and [`ThemeTarget::Pager`] may additionally ask the terminal for its
/// background, and the pager asks only while its output is the terminal.
pub fn resolve_theme(cli_theme: Option<&str>, target: ThemeTarget) -> DiffuseTheme {
    if let Some(spec) = cli_theme {
        if spec.trim().eq_ignore_ascii_case("auto") {
            return match target {
                ThemeTarget::Tui => DiffuseTheme::auto_detect_interactive(),
                ThemeTarget::Pager => DiffuseTheme::auto_detect_pager(),
                ThemeTarget::Batch => DiffuseTheme::auto_detect(),
            };
        }
        match load_theme(spec) {
            Ok(theme) => return theme,
            Err(err) => eprintln!("diffuse: {err}; falling back to default theme"),
        }
    }
    match target {
        ThemeTarget::Tui => DiffuseTheme::from_env_interactive(),
        ThemeTarget::Pager => DiffuseTheme::from_env_pager(),
        ThemeTarget::Batch => DiffuseTheme::from_env(),
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
