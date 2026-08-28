//! External Editor Invocation & Pattern Matching for diffuse TUI.

use std::{
    env,
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorRequest {
    pub file: String,
    pub line: u32,
    pub range: Option<(u32, u32)>,
}

pub struct EditorPattern {
    pub names: &'static [&'static str],
    pub open: &'static [&'static str],
    pub range: Option<&'static [&'static str]>,
    pub detached: bool,
}

pub const EDITOR_PATTERNS: &[EditorPattern] = &[
    EditorPattern {
        names: &["vim", "vi", "nvim"],
        open: &["{bin}", "+{line}", "{file}"],
        range: Some(&[
            "{bin}",
            "-c",
            "{start}G",
            "-c",
            "normal! V{count}j",
            "{file}",
        ]),
        detached: false,
    },
    EditorPattern {
        names: &["nano", "micro", "pico", "emacs"],
        open: &["{bin}", "+{line}", "{file}"],
        range: None,
        detached: false,
    },
    EditorPattern {
        names: &["helix", "hx", "zed", "fresh"],
        open: &["{bin}", "{file}:{line}"],
        range: None,
        detached: false,
    },
    EditorPattern {
        names: &["code", "codium", "cursor"],
        open: &["{bin}", "--goto", "{file}:{line}:1"],
        range: None,
        detached: true,
    },
    EditorPattern {
        names: &["antigravity"],
        open: &["{bin}", "{file}:{line}"],
        range: None,
        detached: true,
    },
];

pub fn editor_binary() -> String {
    env::var("REVIEW_EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

pub fn resolve_editor_args(
    bin: &str,
    file: &str,
    line: u32,
    range: Option<(u32, u32)>,
) -> (Vec<String>, bool) {
    let base_bin = bin
        .split_whitespace()
        .next()
        .unwrap_or(bin)
        .rsplit('/')
        .next()
        .unwrap_or(bin)
        .to_lowercase();

    let pattern = EDITOR_PATTERNS
        .iter()
        .find(|p| p.names.iter().any(|&n| n == base_bin));

    let line_str = line.max(1).to_string();
    let (start_str, end_str, count_str) = match range {
        Some((s, e)) => {
            let lo = s.min(e).max(1);
            let hi = s.max(e).max(1);
            (lo.to_string(), hi.to_string(), (hi - lo).to_string())
        }
        None => (line_str.clone(), line_str.clone(), "0".to_string()),
    };

    let (tpl, detached) = if let Some(pat) = pattern {
        let t = if let (Some(_), Some(r)) = (range, pat.range) {
            r
        } else {
            pat.open
        };
        (t.to_vec(), pat.detached)
    } else {
        (vec!["{bin}", "+{line}", "{file}"], false)
    };

    let argv = tpl
        .into_iter()
        .map(|tok| {
            tok.replace("{bin}", bin)
                .replace("{file}", file)
                .replace("{line}", &line_str)
                .replace("{start}", &start_str)
                .replace("{end}", &end_str)
                .replace("{count}", &count_str)
        })
        .collect();

    (argv, detached)
}

/// Open a file in the configured editor at the specified line or range.
pub fn open_editor(
    file: &str,
    line: u32,
    range: Option<(u32, u32)>,
    repo_root: &Path,
) -> std::io::Result<bool> {
    let bin = editor_binary();
    let abs_path = if Path::new(file).is_absolute() {
        file.to_string()
    } else {
        repo_root.join(file).to_string_lossy().to_string()
    };

    let (argv, detached) = resolve_editor_args(&bin, &abs_path, line, range);
    if argv.is_empty() {
        return Ok(false);
    }

    if detached {
        let mut cmd = Command::new(&argv[0]);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        cmd.current_dir(repo_root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(true)
    } else {
        let mut cmd = Command::new(&argv[0]);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        let status = cmd
            .current_dir(repo_root)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        Ok(status.success())
    }
}

#[cfg(test)]
mod tests;
