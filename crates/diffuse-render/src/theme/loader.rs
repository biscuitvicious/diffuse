//! Dynamic theme loader: 3-tier resolution for user-supplied themes.
//!
//! Resolution order for a name-or-path argument:
//!
//! 1. **File path**: if the value names an existing file on disk, load and
//!    parse it.
//! 2. **Builtin name**: `"dark"`, `"default"`, `"black"`, `"light"`, `"white"`,
//!    `"auto"` resolve to the bundled assets via [`DiffuseTheme::from_name`].
//! 3. **XDG discovery**: search `$XDG_CONFIG_HOME/diffuse/themes/<name>.toml`
//!    and `~/.config/diffuse/themes/<name>.toml`.
//!
//! Loaded user palettes are heap-allocated and leaked into a stable
//! `'static` reference on first use, satisfying the zero-copy
//! [`DiffuseTheme::palette`] contract without introducing a runtime
//! allocation path in the renderer.

use std::{fs, io, path::PathBuf};

use thiserror::Error;

use super::{DiffuseTheme, ThemePalette, ThemeParseError, toml::parse_theme_str};

#[cfg(test)]
use super::toml::model::Mode;

// ── Error type ──────────────────────────────────────────────────────────────

/// Errors returned by [`load_theme`].
#[derive(Debug, Error)]
pub enum ThemeLoadError {
    /// No file, builtin, or XDG theme matched the supplied name.
    #[error("theme not found: '{name}'")]
    NotFound {
        /// The unresolved name or path.
        name: String,
    },
    /// The theme file was found but its TOML or schema is invalid.
    #[error("theme parse error in '{path}': {source}")]
    Parse {
        /// The file path that failed to parse.
        path: String,
        /// The underlying parse error.
        #[source]
        source: ThemeParseError,
    },
    /// An I/O error prevented reading the theme file.
    #[error("theme I/O error reading '{path}': {source}")]
    Io {
        /// The file path that could not be read.
        path: String,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Resolve and load a theme by name or path using the 3-tier strategy:
///
/// 1. **File path** — if `name_or_path` resolves to an existing file, parse it
///    directly.
/// 2. **Builtin name** — `"dark"`, `"default"`, `"black"`, `"light"`,
///    `"white"`, `"auto"` resolve to the bundled assets.
/// 3. **XDG discovery** — search `$XDG_CONFIG_HOME/diffuse/themes/<name>.toml`
///    and `~/.config/diffuse/themes/<name>.toml`.
///
/// # Errors
///
/// Returns [`ThemeLoadError`] when no matching theme is found, the TOML/schema
/// is invalid, or an I/O error prevents reading a candidate file.
pub fn load_theme(name_or_path: &str) -> Result<DiffuseTheme, ThemeLoadError> {
    // Tier 1: explicit file path.
    let path = std::path::Path::new(name_or_path);
    if path.exists() {
        return load_from_path(path);
    }

    // Tier 2: builtin name (also handles "auto").
    if let Some(theme) = DiffuseTheme::from_name(name_or_path) {
        return Ok(theme);
    }

    // Tier 3: XDG config discovery.
    for candidate in xdg_theme_candidates(name_or_path) {
        if candidate.exists() {
            return load_from_path(&candidate);
        }
    }

    Err(ThemeLoadError::NotFound {
        name: name_or_path.to_owned(),
    })
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Parse and materialize a `DiffuseTheme` from a TOML file on disk.
///
/// The compiled [`ThemePalette`] is heap-allocated and leaked into a stable
/// `'static` reference so that the returned [`DiffuseTheme`] satisfies its
/// `palette: &'static ThemePalette` contract without retaining a borrow into
/// a local `ParsedTheme`.
fn load_from_path(path: &std::path::Path) -> Result<DiffuseTheme, ThemeLoadError> {
    let path_str = path.display().to_string();
    let source = fs::read_to_string(path).map_err(|source| ThemeLoadError::Io {
        path: path_str.clone(),
        source,
    })?;
    let parsed = parse_theme_str(&source).map_err(|source| ThemeLoadError::Parse {
        path: path_str,
        source,
    })?;
    Ok(materialize(parsed))
}

/// Promote a [`super::toml::ParsedTheme`] into a [`DiffuseTheme`] by leaking
/// its palette into a stable `'static` reference.
fn materialize(parsed: super::toml::ParsedTheme) -> DiffuseTheme {
    let name: &'static str = Box::leak(parsed.name.into_boxed_str());
    let palette: &'static ThemePalette = Box::leak(Box::new(parsed.palette));
    DiffuseTheme {
        name,
        mode: parsed.mode,
        palette,
    }
}

/// Enumerate XDG config candidate paths for a theme name, in search priority
/// order:
///
/// 1. `$XDG_CONFIG_HOME/diffuse/themes/<name>.toml`
/// 2. `~/.config/diffuse/themes/<name>.toml`
fn xdg_theme_candidates(name: &str) -> Vec<PathBuf> {
    let file_name = format!("{name}.toml");
    let mut candidates = Vec::with_capacity(2);

    if let Some(xdg_home) = std::env::var_os("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg_home);
        p.push("diffuse");
        p.push("themes");
        p.push(&file_name);
        candidates.push(p);
    }

    if let Some(home) = home_dir() {
        let mut p = home;
        p.push(".config");
        p.push("diffuse");
        p.push("themes");
        p.push(&file_name);
        candidates.push(p);
    }

    candidates
}

/// Portable home directory lookup: `$HOME` on all platforms.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::{io::Write, sync::Mutex};

    use super::*;

    /// Serializes env-touching tests: process env is global, so parallel
    /// XDG tests would otherwise observe each other's variables.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // Minimal valid TOML theme that inherits the dark base and overrides one
    // color to confirm the override round-trips.
    const MINIMAL_DARK_TOML: &str = r#"
        inherits = "dark"
        name = "test-custom"
        [ui]
        accent = 200
    "#;

    const MINIMAL_LIGHT_TOML: &str = r#"
        inherits = "light"
        name = "test-custom-light"
    "#;

    const MALFORMED_TOML: &str = "not [ valid [ toml";

    // ── Tier 1: file path ────────────────────────────────────────────────────

    #[test]
    fn test_load_theme_from_file_dark_base() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(MINIMAL_DARK_TOML.as_bytes()).unwrap();
        let theme = load_theme(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(theme.name, "test-custom");
        assert_eq!(theme.mode, Mode::Dark);
    }

    #[test]
    fn test_load_theme_from_file_light_base() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(MINIMAL_LIGHT_TOML.as_bytes()).unwrap();
        let theme = load_theme(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(theme.name, "test-custom-light");
        assert_eq!(theme.mode, Mode::Light);
    }

    #[test]
    fn test_load_theme_malformed_toml_returns_parse_error() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(MALFORMED_TOML.as_bytes()).unwrap();
        let err = load_theme(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(
            matches!(err, ThemeLoadError::Parse { .. }),
            "expected Parse error, got: {err}"
        );
    }

    // ── Tier 2: builtin names ────────────────────────────────────────────────

    #[test]
    fn test_load_theme_builtin_dark() {
        let theme = load_theme("dark").unwrap();
        assert_eq!(theme.mode, Mode::Dark);
    }

    #[test]
    fn test_load_theme_builtin_default() {
        let theme = load_theme("default").unwrap();
        assert_eq!(theme.mode, Mode::Dark);
    }

    #[test]
    fn test_load_theme_builtin_light() {
        let theme = load_theme("light").unwrap();
        assert_eq!(theme.mode, Mode::Light);
    }

    // ── Tier 3: not found ────────────────────────────────────────────────────

    #[test]
    fn test_load_theme_not_found_returns_error() {
        // A name that is neither a file on disk, a builtin, nor resolvable via
        // XDG (we keep the test hermetic by using an impossible name).
        let err = load_theme("__diffuse_nonexistent_theme_xyz__").unwrap_err();
        assert!(
            matches!(err, ThemeLoadError::NotFound { .. }),
            "expected NotFound error, got: {err}"
        );
        assert!(
            err.to_string()
                .contains("__diffuse_nonexistent_theme_xyz__")
        );
    }

    // ── XDG discovery ────────────────────────────────────────────────────────

    #[test]
    fn test_load_theme_via_xdg_config_home() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let dir = tempfile::TempDir::new().unwrap();
        let theme_dir = dir.path().join("diffuse/themes");
        std::fs::create_dir_all(&theme_dir).unwrap();
        let theme_path = theme_dir.join("my-theme.toml");
        std::fs::write(&theme_path, MINIMAL_DARK_TOML).unwrap();

        // Point XDG_CONFIG_HOME at the temp dir for this test.
        // Safety: serialized behind ENV_MUTEX; no other test touches
        // XDG_CONFIG_HOME.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let result = load_theme("my-theme");
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let theme = result.unwrap();
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(theme.name, "test-custom");
    }

    #[test]
    fn test_load_theme_xdg_falls_back_to_home_config() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // With XDG_CONFIG_HOME unset, discovery falls back to
        // $HOME/.config/diffuse/themes/<name>.toml.
        let dir = tempfile::TempDir::new().unwrap();
        let theme_dir = dir.path().join(".config/diffuse/themes");
        std::fs::create_dir_all(&theme_dir).unwrap();
        std::fs::write(theme_dir.join("home-theme.toml"), MINIMAL_LIGHT_TOML).unwrap();

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("HOME", dir.path());
        }
        let result = load_theme("home-theme");
        unsafe {
            std::env::remove_var("HOME");
        }

        let theme = result.unwrap();
        assert_eq!(theme.mode, Mode::Light);
        assert_eq!(theme.name, "test-custom-light");
    }

    // ── Error display ────────────────────────────────────────────────────────

    #[test]
    fn test_theme_load_error_not_found_message() {
        let err = ThemeLoadError::NotFound {
            name: "solarized".to_string(),
        };
        assert_eq!(err.to_string(), "theme not found: 'solarized'");
    }
}
