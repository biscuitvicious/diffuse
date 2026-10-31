//! CLI routing integration tests: piped/redirected invocations must never
//! fall into the extension JSON contract, and non-interactive output must
//! never try to open the interactive TUI.

use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use diffuse_git::testing::{commit_files, init_repo};

/// `diffuse < /dev/null | cat` with no git args and no JSON payload must
/// route to the standalone reviewer (git repo resolution), never to the
/// JSON contract — which would silently swallow the empty stdin and emit an
/// empty JSON document with exit 0.
#[test]
fn empty_piped_stdin_routes_to_standalone_reviewer() {
    // A provably NON-git dir: the standalone path fails with the repo error
    // (exit 2); the JSON path would exit 0 with `{"files":[]}`.
    let dir = tempfile::tempdir().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take()); // close stdin immediately: EOF, no payload
    let out = child.wait_with_output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "standalone routing must exit 2 on a non-repo dir (stdout: {})",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not a git repository"),
        "stderr must report the repo error, got: {err}"
    );
}

#[test]
fn non_git_directory_exits_with_stderr_error_without_tui() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "must exit 2 on non-repo dir when invoked directly"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not a git repository"),
        "stderr must report 'not a git repository', got: {err}"
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must remain completely empty (no TUI escape codes), got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[cfg(unix)]
#[test]
fn non_git_directory_in_pty_terminal_exits_with_stderr_without_tui() {
    let pty = nix::pty::openpty(None, None).expect("openpty failed");
    // master and slave are OwnedFd; keep master alive so the slave end stays
    // open, then convert slave to File for use as stdin/stdout.
    let _master = pty.master;
    let slave = File::from(pty.slave);

    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .stdin(slave.try_clone().unwrap())
        .stdout(slave)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "must exit 2 on non-repo dir even when stdin/stdout are a tty"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not a git repository"),
        "stderr must report 'not a git repository', got: {err}"
    );
}

/// `diffuse | prog` on a real repo: piped stdout must produce the full ANSI
/// render — never the JSON contract and never the alternate-screen TUI.
#[test]
fn piped_stdout_renders_not_json_and_not_tui() {
    let dir = setup_repo();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();

    assert_eq!(out.status.code(), Some(0), "render must succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.trim_start().starts_with('{'),
        "must not emit the JSON contract, got: {:.200}",
        stdout
    );
    assert!(
        stdout.contains("\x1b["),
        "must emit an ANSI-styled render, got: {:.200}",
        stdout
    );
    assert!(
        !stdout.contains("?1049"),
        "must not enter the alternate screen (no TUI when piped)"
    );
}

fn setup_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path()).unwrap();
    commit_files(&repo, "init", &[("a.txt", "one\ntwo\n")]).unwrap();
    fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\nworld\n").unwrap();
    dir
}

#[test]
fn jsonl_streaming_emits_valid_json_lines() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-one\n+ONE\ndiff --git a/b.txt b/b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1,1 +1,1 @@\n-two\n+TWO\n";
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--jsonl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    Write::write_all(&mut stdin, diff.as_bytes()).unwrap();
    drop(stdin);

    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty(), "must emit JSON lines");

    for line in &lines {
        let rec: diffuse::core::wire::jsonl::WireRecord =
            serde_json::from_str(line).expect("valid WireRecord JSON line");
        match rec {
            diffuse::core::wire::jsonl::WireRecord::FileHeader(h) => {
                assert!(!h.path.is_empty());
            }
            diffuse::core::wire::jsonl::WireRecord::HunkHeader(_) => {}
            diffuse::core::wire::jsonl::WireRecord::Line(l) => {
                assert!(!l.text().is_empty());
            }
            diffuse::core::wire::jsonl::WireRecord::FileFooter(f) => {
                assert!(!f.path.is_empty());
            }
        }
    }

    let decoded =
        diffuse::core::wire::jsonl::decode_jsonl_str(&stdout).expect("valid JSONL stream");
    assert_eq!(decoded.len(), 2, "must decode exactly two files");
    assert_eq!(decoded[0].path, "a.txt");
    assert_eq!(decoded[1].path, "b.txt");
}

#[test]
fn non_tui_mode_handles_broken_pipe_cleanly() {
    let dir = setup_repo();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--no-tui")
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // Drop stdout pipe reader immediately to simulate `diffuse | head -n 1`
    drop(child.stdout.take());
    let _ = child.wait();
}

#[test]
fn large_changeset_progressive_streaming_performance() {
    let dir = tempfile::tempdir().unwrap();

    let repo = init_repo(dir.path()).unwrap();

    // Create 60 files
    let file_contents: Vec<(String, String)> = (0..60)
        .map(|i| {
            (
                format!("file_{i}.rs"),
                format!("fn func_{i}() {{\n    println!(\"hello {i}\");\n}}\n"),
            )
        })
        .collect();
    let file_refs: Vec<(&str, &str)> = file_contents
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    commit_files(&repo, "init commit", &file_refs).unwrap();

    // Modify all 60 files
    for i in 0..60 {
        fs::write(
            dir.path().join(format!("file_{i}.rs")),
            format!("fn func_{i}() {{\n    println!(\"modified {i}\");\n}}\n"),
        )
        .unwrap();
    }

    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--no-tui")
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first_line = String::new();
    BufRead::read_line(&mut stdout, &mut first_line).unwrap();
    let time_to_first_file = start.elapsed();

    assert!(
        !first_line.is_empty(),
        "must yield first file line progressively"
    );
    assert!(
        time_to_first_file < Duration::from_millis(500),
        "time to first file ({time_to_first_file:?}) must be under 500ms"
    );

    io::copy(&mut stdout, &mut io::sink()).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
}

/// `diffuse -C <dir>` from an external non-git directory must change directory
/// to `<dir>` and successfully discover and review the target git repository.
#[test]
fn test_c_flag_changes_directory() {
    let repo_dir = setup_repo();
    let outside_dir = tempfile::tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-C")
        .arg(repo_dir.path())
        .arg("--no-tui")
        .current_dir(outside_dir.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse -C <dir> must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("a.txt") || stdout.contains("b.txt"),
        "stdout must contain diff from target repo, got: {stdout}"
    );
}

/// Multiple `-C` flags must evaluate sequentially (e.g., `-C <parent> -C
/// <child>`).
#[test]
fn test_multiple_c_flags_evaluate_sequentially() {
    let parent = tempfile::tempdir().unwrap();
    let child_repo = parent.path().join("sub_repo");
    fs::create_dir_all(&child_repo).unwrap();

    let repo = init_repo(&child_repo).unwrap();
    commit_files(&repo, "nested init", &[("nested.rs", "fn nested() {}\n")]).unwrap();
    fs::write(child_repo.join("nested.rs"), "fn nested_mod() {}\n").unwrap();

    let outside = tempfile::tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-C")
        .arg(parent.path())
        .arg("-C")
        .arg("sub_repo")
        .arg("--no-tui")
        .current_dir(outside.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "sequential -C must succeed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("nested.rs"),
        "stdout must contain diff for nested.rs, got: {stdout}"
    );
}

/// `diffuse review [git_args...] --pager` explicit subcommand runs the
/// reviewer.
#[test]
fn test_cli_review_subcommand() {
    let repo_dir = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("review")
        .arg("--pager")
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&repo_dir);

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse review --pager must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\x1b["),
        "stdout must contain ANSI colored diff"
    );
}

/// `diffuse --pager` and `diffuse -p` should activate pager mode.
#[test]
fn test_cli_pager_flag() {
    let repo_dir = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-p")
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&repo_dir);

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse -p must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\x1b["),
        "stdout must contain ANSI colored diff"
    );
}

/// `diffuse parse --json` should parse diff and emit JSON token stream.
#[test]
fn test_cli_parse_subcommand_json() {
    let diff_input = r#"diff --git a/hello.rs b/hello.rs
index 0000000..1111111 100644
--- a/hello.rs
+++ b/hello.rs
@@ -1 +1 @@
-println!("old");
+println!("new");
"#;

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("parse")
        .arg("--json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(diff_input.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse parse --json must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim().starts_with('{'),
        "must emit JSON object, got: {stdout}"
    );
    assert!(stdout.contains("\"files\""), "must contain files field");
}

/// `diffuse parse --jsonl` should parse diff and emit JSON Lines.
#[test]
fn test_cli_parse_subcommand_jsonl() {
    let diff_input = r#"diff --git a/hello.rs b/hello.rs
index 0000000..1111111 100644
--- a/hello.rs
+++ b/hello.rs
@@ -1 +1 @@
-println!("old");
+println!("new");
"#;

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("parse")
        .arg("--jsonl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(diff_input.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse parse --jsonl must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty(), "must emit JSONL lines");
    assert!(
        lines[0].starts_with('{'),
        "must be JSON line, got: {}",
        lines[0]
    );
    let decoded = diffuse::core::wire::jsonl::decode_jsonl_str(&stdout).expect("decode jsonl");
    assert_eq!(decoded.len(), 1, "must decode 1 file");
    assert_eq!(decoded[0].path, "hello.rs");
}

/// `diffuse parse --diff <file>` should read diff from file and emit JSON.
#[test]
fn test_cli_parse_with_diff_file() {
    let dir = tempfile::tempdir().unwrap();
    let diff_file = dir.path().join("test.patch");
    fs::write(
        &diff_file,
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-1\n+2\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("parse")
        .arg("--diff")
        .arg(&diff_file)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse parse --diff must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().starts_with('{'));
    assert!(stdout.contains("\"a.txt\""));
}

/// `diffuse review HEAD~1 --pager` reviews specified commit range.
#[test]
fn test_cli_review_commit_range() {
    let repo = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("review")
        .arg("HEAD~1")
        .arg("-p")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse review HEAD~1 -p must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\x1b["));
}

/// `diffuse -U 3 -p` honors context flag.
#[test]
fn test_cli_context_flag() {
    let repo = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-U")
        .arg("3")
        .arg("-p")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse -U 3 -p must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `diffuse -m` emits valid GitHub-Flavored Markdown diff output.
#[test]
fn test_cli_markdown_flag() {
    let repo = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-m")
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse -m must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("### Diff Summary"),
        "stdout must contain summary table: {stdout}"
    );
    assert!(
        stdout.contains("<details open>"),
        "stdout must contain details: {stdout}"
    );
    assert!(
        stdout.contains("```diff"),
        "stdout must contain diff code blocks: {stdout}"
    );
}

/// `cat diff | diffuse -m` emits valid Markdown from piped stdin.
#[test]
fn test_cli_markdown_piped_diff() {
    let diff = "diff --git a/hello.rs b/hello.rs\n--- a/hello.rs\n+++ b/hello.rs\n@@ -1,1 +1,1 @@\n-hello\n+world\n";
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-m")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(diff.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "piped diffuse -m must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("### Diff Summary"));
    assert!(stdout.contains("`hello.rs`"));
    assert!(stdout.contains("-hello\n+world"));
}

/// `diffuse -m --color=always` emits ANSI color codes in Markdown output.
#[test]
fn test_cli_markdown_color_flag() {
    let repo_dir = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-m")
        .arg("--color=always")
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&repo_dir);

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\x1b["));
    assert!(stdout.contains("\x1b[0m"));
    assert!(stdout.contains("Diff Summary"));
}

/// `diffuse --raw` on a git repo emits flat green/red colorized output:
/// no JSON, no subword spans, plain unified-diff-style coloring.
#[test]
fn test_cli_raw_flag_emits_ansi_not_json() {
    let repo_dir = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--raw")
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse --raw must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Must have ANSI color codes
    assert!(
        stdout.contains("\x1b["),
        "raw output must contain ANSI codes, got: {:.200}",
        stdout
    );
    // Must NOT be JSON
    assert!(
        !stdout.trim_start().starts_with('{'),
        "raw output must not be JSON, got: {:.200}",
        stdout
    );
    // Raw mode uses flat green (120) for additions and red (203) for deletions
    assert!(
        stdout.contains("\x1b[38;5;120m") || stdout.contains("\x1b[38;5;203m"),
        "raw output must contain raw-mode fg colors, got: {:.200}",
        stdout
    );
}

/// `diffuse --raw` on piped diff input emits flat colorized output.
#[test]
fn test_cli_raw_flag_with_piped_diff() {
    let diff = "diff --git a/hello.rs b/hello.rs\n\
index 0000000..1111111 100644\n\
--- a/hello.rs\n\
+++ b/hello.rs\n\
@@ -1,2 +1,2 @@\n\
 fn main() {\n\
-    println!(\"hello\");\n\
+    println!(\"world\");\n\
 }\n";

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--raw")
        .arg("--color=always")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(diff.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "piped diffuse --raw must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\x1b[38;5;120m") || stdout.contains("\x1b[38;5;203m"),
        "raw output must use raw-mode colors, got: {:.300}",
        stdout
    );
    // Must contain hunk header
    assert!(
        stdout.contains("@@ -1,2 +1,2 @@"),
        "raw output must contain hunk header, got: {stdout}"
    );
    // Number gutter and change type symbols are styled: walk SGR
    // foreground state and assert the symbol renders while its kind color
    // is live (byte adjacency is not asserted — the optimizer legitimately
    // drops redundant repeats, carrying the color forward instead).
    assert!(
        gutter_symbol_live_color(&stdout, '+', 120) || gutter_symbol_live_color(&stdout, '-', 203),
        "change type symbols must be colorized, got: {stdout}"
    );
    assert!(!stdout.trim_start().starts_with('{'), "must not be JSON");
}

/// Scan `output` for a gutter line whose change symbol (`+`/`-`) renders
/// while 256-palette foreground `fg` is live. Tracks a tiny subset of SGR
/// (38;5;N, 39/0 resets) — enough to assert visual semantics without
/// depending on exact escape placement.
fn gutter_symbol_live_color(output: &str, symbol: char, fg: u8) -> bool {
    for raw_line in output.lines() {
        let stripped = strip_ansi_codes(raw_line);
        let Some(sym_idx) = stripped.find(symbol) else {
            continue;
        };
        // Only gutter symbols (near line start) count, not code content.
        if sym_idx > 6 {
            continue;
        }
        let mut live_fg: Option<u8> = None;
        let bytes = raw_line.as_bytes();
        let mut i = 0;
        let mut vis_idx = 0usize;
        while i < bytes.len() {
            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'm' {
                    let body = &raw_line[i + 2..j];
                    let params: Vec<u32> = body.split(';').filter_map(|w| w.parse().ok()).collect();
                    match params.as_slice() {
                        [0] | [] => live_fg = None,
                        [39] => live_fg = None,
                        [38, 5, n] => live_fg = Some(*n as u8),
                        _ => {}
                    }
                    i = j + 1;
                    continue;
                }
            }
            if vis_idx == sym_idx {
                return live_fg == Some(fg);
            }
            vis_idx += raw_line[i..].chars().next().map_or(1, |_c| 1);
            i += raw_line[i..].chars().next().map_or(1, |c| c.len_utf8());
        }
    }
    false
}

/// Strip `\x1b[...m` SGR sequences (only) for structural assertions.
fn strip_ansi_codes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' {
                i = j + 1;
                continue;
            }
        }
        let c = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `diffuse parse --raw` on a git repo emits raw ANSI diff.
#[test]
fn test_cli_parse_raw_subcommand() {
    let repo_dir = setup_repo();
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .args(["parse", "--raw"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\x1b["));
    assert!(!stdout.trim_start().starts_with('{'));
}

/// Setting `DIFFUSE_LOG_FILE` writes structured tracing log events to that
/// file.
#[test]
fn test_cli_diffuse_log_file() {
    let repo_dir = setup_repo();
    let log_file = tempfile::NamedTempFile::new().unwrap();
    let log_path = log_file.path().to_path_buf();

    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-p")
        .env("DIFFUSE_LOG_FILE", &log_path)
        .env("DIFFUSE_LOG", "trace")
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    // The log file was created/opened
    assert!(log_path.exists());
}

/// Raw diff stream stdin contract (diff → wire JSON) piped into `diffuse
/// --json`.
#[test]
fn test_diff_stream_stdin_wire_contract() {
    let diff = "diff --git a/test.rs b/test.rs\nindex 0000000..1111111 100644\n--- a/test.rs\n+++ b/test.rs\n@@ -1 +1 @@\n-old\n+new\n";

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(diff.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "diffuse --json must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let wire: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .expect("stdout must be wire JSON");
    assert_eq!(wire["files"].as_array().unwrap().len(), 1);
    assert_eq!(wire["files"][0]["path"], "test.rs");
    assert_eq!(wire["files"][0]["hunks"].as_array().unwrap().len(), 1);
}

#[test]
fn diffuse_staged_flag_excludes_untracked_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "init", &[("tracked.rs", "fn tracked() {}\n")]).unwrap();

    // Create untracked file
    fs::write(dir.join("untracked.rs"), "fn untracked() {}\n").unwrap();

    // 1. With --staged
    let out_staged = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-p")
        .arg("--staged")
        .current_dir(dir)
        .output()
        .unwrap();

    assert_eq!(
        out_staged.status.code(),
        Some(0),
        "diffuse -p --staged must exit 0, stderr: {}",
        String::from_utf8_lossy(&out_staged.stderr)
    );
    let stdout_staged = String::from_utf8_lossy(&out_staged.stdout);
    assert!(
        !stdout_staged.contains("untracked.rs"),
        "diffuse --staged stdout must NOT contain untracked files, got: {stdout_staged}"
    );

    // 2. With --cached
    let out_cached = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-p")
        .arg("--cached")
        .current_dir(dir)
        .output()
        .unwrap();

    assert_eq!(
        out_cached.status.code(),
        Some(0),
        "diffuse -p --cached must exit 0, stderr: {}",
        String::from_utf8_lossy(&out_cached.stderr)
    );
    let stdout_cached = String::from_utf8_lossy(&out_cached.stdout);
    assert!(
        !stdout_cached.contains("untracked.rs"),
        "diffuse --cached stdout must NOT contain untracked files, got: {stdout_cached}"
    );

    // 3. With -s (short flag parsed by Clap)
    let out_short_s = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("-p")
        .arg("-s")
        .current_dir(dir)
        .output()
        .unwrap();

    assert_eq!(
        out_short_s.status.code(),
        Some(0),
        "diffuse -p -s must exit 0, stderr: {}",
        String::from_utf8_lossy(&out_short_s.stderr)
    );
    let stdout_short_s = String::from_utf8_lossy(&out_short_s.stdout);
    assert!(
        !stdout_short_s.contains("untracked.rs"),
        "diffuse -p -s stdout must NOT contain untracked files, got: {stdout_short_s}"
    );

    // 4. With review --staged subcommand parsed by Clap
    let out_review_staged = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("review")
        .arg("-p")
        .arg("--staged")
        .current_dir(dir)
        .output()
        .unwrap();

    assert_eq!(
        out_review_staged.status.code(),
        Some(0),
        "diffuse review -p --staged must exit 0, stderr: {}",
        String::from_utf8_lossy(&out_review_staged.stderr)
    );
    let stdout_review_staged = String::from_utf8_lossy(&out_review_staged.stdout);
    assert!(
        !stdout_review_staged.contains("untracked.rs"),
        "diffuse review -p --staged stdout must NOT contain untracked files, got: {stdout_review_staged}"
    );
}

#[test]
fn test_cli_notes_subcommand_suite() {
    use diffuse::core::{
        merge::RunKind,
        notes::{NoteStatus, NotesDB, ReviewNote, add_note, load_notes, save_notes},
        parse::RowKind,
        wire::{OutputRow, OutputRun},
    };

    let dir = tempfile::tempdir().unwrap();
    let notes_path = dir.path().join("notes.json");
    let mut db = NotesDB::new();
    let source_row = OutputRow {
        kind: RowKind::Ctx,
        mov_side: None,
        old_line: None,
        new_line: Some(10),
        runs: vec![OutputRun::new(RunKind::Ctx, "fn original() {}".to_string())],
        wholesale: false,
        group: None,
    };

    let note_open = ReviewNote {
        id: "note-open".into(),
        file: "src/lib.rs".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(10),
        ),
        text: "Fix this issue".into(),
        created_at: 1000,
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: vec![source_row],
    };
    let note_prog = ReviewNote {
        id: "note-prog".into(),
        file: "src/lib.rs".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(20),
        ),
        text: "Working on refactor".into(),
        created_at: 1001,
        content_hash: None,
        status: NoteStatus::InProgress,
        original_source_rows: Vec::new(),
    };
    let note_res = ReviewNote {
        id: "note-res".into(),
        file: "src/main.rs".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(30),
        ),
        text: "Already resolved".into(),
        created_at: 1002,
        content_hash: None,
        status: NoteStatus::Resolved,
        original_source_rows: Vec::new(),
    };

    add_note(&mut db, "repo1", "", note_open);
    add_note(&mut db, "repo1", "", note_prog);
    add_note(&mut db, "repo1", "", note_res);
    save_notes(&notes_path, &db).unwrap();

    let notes_file_arg = notes_path.to_str().unwrap();

    // 1. Default notes list: shows Open and InProgress, with resolved summary count
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Fix this issue"));
    assert!(stdout.contains("Working on refactor"));
    assert!(!stdout.contains("Already resolved"));
    assert!(stdout.contains("1 resolved"));

    // 2. List --resolved: shows only resolved notes
    let out_res = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("list")
        .arg("--resolved")
        .output()
        .unwrap();
    assert_eq!(out_res.status.code(), Some(0));
    let stdout_res = String::from_utf8_lossy(&out_res.stdout);
    assert!(!stdout_res.contains("Fix this issue"));
    assert!(stdout_res.contains("Already resolved"));

    // 3. List --in-progress: shows only in-progress notes
    let out_prog = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("list")
        .arg("--in-progress")
        .output()
        .unwrap();
    assert_eq!(out_prog.status.code(), Some(0));
    let stdout_prog = String::from_utf8_lossy(&out_prog.stdout);
    assert!(stdout_prog.contains("Working on refactor"));
    assert!(!stdout_prog.contains("Fix this issue"));
    assert!(!stdout_prog.contains("Already resolved"));

    // 4. List --all: shows all notes
    let out_all = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("list")
        .arg("--all")
        .output()
        .unwrap();
    assert_eq!(out_all.status.code(), Some(0));
    let stdout_all = String::from_utf8_lossy(&out_all.stdout);
    assert!(stdout_all.contains("Fix this issue"));
    assert!(stdout_all.contains("Working on refactor"));
    assert!(stdout_all.contains("Already resolved"));

    // 5. List --json: JSON payload with notes array and resolvedCount
    let out_json = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out_json.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out_json.stdout).unwrap();
    assert_eq!(v["resolvedCount"], 1);
    assert_eq!(v["notes"].as_array().unwrap().len(), 2);

    // 6. Show with --show-source
    let out_show = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("show")
        .arg("note-open")
        .arg("--show-source")
        .output()
        .unwrap();
    assert_eq!(out_show.status.code(), Some(0));
    let stdout_show = String::from_utf8_lossy(&out_show.stdout);
    assert!(stdout_show.contains("Note:     note-open"));
    assert!(stdout_show.contains("fn original() {}"));
    assert!(
        !stdout_show.contains('│'),
        "source lines must not have indentation '│': {stdout_show}"
    );
    assert!(stdout_show.contains("Fix this issue"));

    // 7. Reply with --resolve
    let out_reply = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("reply")
        .arg("note-open")
        .arg("-m")
        .arg("I have addressed it")
        .arg("--resolve")
        .output()
        .unwrap();
    assert_eq!(out_reply.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let note = db
        .get("repo1")
        .unwrap()
        .get("")
        .unwrap()
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::Resolved);
    assert_eq!(note.text, "I have addressed it\n\n> Fix this issue");

    // 8. Progress
    let out_mark_prog = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("progress")
        .arg("note-open")
        .output()
        .unwrap();
    assert_eq!(out_mark_prog.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let note = db
        .get("repo1")
        .unwrap()
        .get("")
        .unwrap()
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::InProgress);

    // 9. Open / Reopen
    let out_open = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("open")
        .arg("note-open")
        .output()
        .unwrap();
    assert_eq!(out_open.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let note = db
        .get("repo1")
        .unwrap()
        .get("")
        .unwrap()
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::Open);

    // 10. Status explicit
    let out_status = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("notes")
        .arg("status")
        .arg("note-open")
        .arg("resolved")
        .output()
        .unwrap();
    assert_eq!(out_status.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let note = db
        .get("repo1")
        .unwrap()
        .get("")
        .unwrap()
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::Resolved);
}

#[test]
fn test_cli_notes_add_subcommand() {
    use diffuse::core::notes::{NoteStatus, load_notes};

    let tmp = tempfile::tempdir().unwrap();
    let notes_path = tmp.path().join("notes.json");
    let notes_file_arg = notes_path.to_str().unwrap();

    // A real repository with an uncommitted modification: the create flow
    // anchors notes against the reviewed diff rows.
    let repo = init_repo(tmp.path()).unwrap();
    let _ = commit_files(
        &repo,
        "commit 1",
        &[(
            "foo.rs",
            "fn first() {}\nfn second() {}\nfn third() {}\nfn fourth() {}\n",
        )],
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("foo.rs"),
        "fn first() {}\nfn second() renamed() {}\nfn third() {}\n",
    )
    .unwrap();

    // 1. Create a note with coordinates in the target string: the stored rows are
    //    the actual diff rows the note was created against.
    let out1 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs:1-2")
        .arg("-m")
        .arg("First two functions")
        .output()
        .unwrap();
    assert_eq!(
        out1.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let db = load_notes(&notes_path);
    let repo_map = db.get("test-repo").unwrap();
    let rev_map = repo_map.values().next().unwrap();
    let notes = rev_map.get("foo.rs").unwrap();
    assert_eq!(notes.len(), 1);
    let n1 = &notes[0];
    assert_eq!(n1.file, "foo.rs");
    assert_eq!(n1.start(), 1);
    assert_eq!(n1.end(), 2);
    assert_eq!(n1.status, NoteStatus::Open);
    assert_eq!(n1.text, "First two functions");
    assert_eq!(n1.original_source_rows.len(), 2);
    assert_eq!(n1.original_source_rows[0].text(), "fn first() {}");
    assert_eq!(
        n1.original_source_rows[1].text(),
        "fn second() renamed() {}"
    );
    // The context row keeps only its new-side number; the modification row
    // carries both sides.
    assert!(n1.original_source_rows[0].old_line.is_none());
    assert_eq!(n1.original_source_rows[1].old_line, Some(2));
    assert!(n1.content_hash.is_some());

    // 2. Create a note with explicit --new and --status
    let out2 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs")
        .arg("--new")
        .arg("3")
        .arg("-m")
        .arg("Third function")
        .arg("--status")
        .arg("in-progress")
        .output()
        .unwrap();
    assert_eq!(out2.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let repo_map = db.get("test-repo").unwrap();
    let rev_map = repo_map.values().next().unwrap();
    let notes = rev_map.get("foo.rs").unwrap();
    assert_eq!(notes.len(), 2);
    let n2 = &notes[1];
    assert_eq!(n2.start(), 3);
    assert_eq!(n2.end(), 3);
    assert_eq!(n2.status, NoteStatus::InProgress);

    // 3. Create a note on the pre-image side of a deletion
    std::fs::write(
        tmp.path().join("foo.rs"),
        "fn first() {}\nfn second() renamed() {}\n",
    )
    .unwrap();
    let out3 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs")
        .arg("--old")
        .arg("3")
        .arg("-m")
        .arg("Deleted third function")
        .output()
        .unwrap();
    assert_eq!(
        out3.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    let db = load_notes(&notes_path);
    let repo_map = db.get("test-repo").unwrap();
    let rev_map = repo_map.values().next().unwrap();
    let notes = rev_map.get("foo.rs").unwrap();
    assert_eq!(notes.len(), 3);
    let n3 = &notes[2];
    assert_eq!(n3.start(), 3);
    assert!(n3.original_source_rows[0].old_line.is_some());
    assert_eq!(n3.original_source_rows[0].text(), "fn third() {}");

    // 4. A both-side anchor must locate the same diff rows
    let out4 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs")
        .arg("--old")
        .arg("7")
        .arg("--new")
        .arg("1")
        .arg("-m")
        .arg("Conflicting sides")
        .output()
        .unwrap();
    assert_eq!(out4.status.code(), Some(1));

    // 5. Missing files are rejected
    let out5 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("missing.rs:10")
        .arg("-m")
        .arg("Missing file note")
        .output()
        .unwrap();
    assert_eq!(out5.status.code(), Some(1));

    // 6. Using alias 'create' and --json
    let out6 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("create")
        .arg("foo.rs:1")
        .arg("-m")
        .arg("JSON create")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out6.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out6.stdout).unwrap();
    assert_eq!(v["file"], "foo.rs");
    assert_eq!(v["text"], "JSON create");
    assert!(v["location"]["coord"].is_object());

    // 7. Using -r / --rev to create and list notes against a commit range
    let c2 = commit_files(
        &repo,
        "commit 2",
        &[("foo.rs", "fn first() {}\nfn second() {}\n")],
    )
    .unwrap();
    let c3 = commit_files(
        &repo,
        "commit 3",
        &[("foo.rs", "fn first() {}\nfn second() {}\nfn third() {}\n")],
    )
    .unwrap();
    let c1_sha = repo.rev_to_full_hash("HEAD~2").unwrap();
    let c2_sha = c2.to_string();
    let c3_sha = c3.to_string();

    let out7 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("create")
        .arg("foo.rs:1")
        .arg("-m")
        .arg("Range note")
        .arg("-r")
        .arg(format!("{c1_sha}..{c3_sha}"))
        .output()
        .unwrap();
    assert_eq!(
        out7.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out7.stderr)
    );

    let db = load_notes(&notes_path);
    let repo_map = db.get("test-repo").unwrap();
    let expected_rev_key = format!("{c1_sha}..{c2_sha}");
    assert!(
        repo_map.contains_key(&expected_rev_key),
        "expected rev key {expected_rev_key} in test-repo, found: {:?}",
        repo_map.keys().collect::<Vec<_>>()
    );

    // Query back using list -r
    let out_list = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("-r")
        .arg(format!("{c1_sha}..{c3_sha}"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out_list.status.code(), Some(0));
    let list_v: serde_json::Value = serde_json::from_slice(&out_list.stdout).unwrap();
    assert_eq!(list_v["notes"].as_array().unwrap().len(), 1);
    assert_eq!(list_v["notes"][0]["text"], "Range note");

    // Query back using diffuse notes <c1>..<c3> (positional range arg, default list
    // action)
    let out_pos = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg(format!("{c1_sha}..{c3_sha}"))
        .output()
        .unwrap();
    assert_eq!(out_pos.status.code(), Some(0));
    let stdout_pos = String::from_utf8_lossy(&out_pos.stdout);
    assert!(stdout_pos.contains("Range note"));
    assert!(v["contentHash"].is_string());
    assert!(!v["originalSourceRows"].as_array().unwrap().is_empty());
}

#[test]
fn test_notes_list_revisions_and_pathspecs() {
    use diffuse::core::notes::{NoteStatus, ReviewNote, add_note, load_notes, save_notes};

    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path()).unwrap();

    // Commit 1 (c1)
    let c1 = commit_files(
        &repo,
        "commit 1",
        &[("src/a.txt", "hello\nworld\n"), ("src/b.txt", "foo\nbar\n")],
    )
    .unwrap()
    .to_string();

    // Commit 2 (c2)
    let c2 = commit_files(
        &repo,
        "commit 2",
        &[
            ("src/a.txt", "hello\nworld\nextra\n"),
            ("doc/readme.txt", "documentation\n"),
        ],
    )
    .unwrap()
    .to_string();

    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut db = load_notes(&notes_path);

    // Notes on c1
    let note_c1_a = ReviewNote {
        id: "c1-a".into(),
        file: "src/a.txt".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(1),
        ),
        text: "Note on c1 file a".into(),
        created_at: 100,
        status: NoteStatus::Open,
        ..Default::default()
    };
    let note_c1_b = ReviewNote {
        id: "c1-b".into(),
        file: "src/b.txt".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(1),
        ),
        text: "Note on c1 file b".into(),
        created_at: 101,
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db, "test-repo", &c1, note_c1_a);
    add_note(&mut db, "test-repo", &c1, note_c1_b);

    // Notes on c2
    let note_c2_a = ReviewNote {
        id: "c2-a".into(),
        file: "src/a.txt".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(1),
        ),
        text: "Note on c2 file a".into(),
        created_at: 200,
        status: NoteStatus::Open,
        ..Default::default()
    };
    let note_c2_doc = ReviewNote {
        id: "c2-doc".into(),
        file: "doc/readme.txt".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(1),
        ),
        text: "Note on c2 doc".into(),
        created_at: 201,
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db, "test-repo", &c2, note_c2_a);
    add_note(&mut db, "test-repo", &c2, note_c2_doc);
    save_notes(&notes_path, &db).unwrap();

    let notes_file_arg = notes_path.to_str().unwrap();

    // 1. Default `diffuse notes list`: scoped to working tree HEAD (c2)
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Note on c2 file a"));
    assert!(stdout.contains("Note on c2 doc"));
    assert!(!stdout.contains("Note on c1 file a"));
    assert!(!stdout.contains("Note on c1 file b"));

    // 2. `diffuse notes list HEAD~1`: scoped to c1
    let out_c1 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("HEAD~1")
        .output()
        .unwrap();
    assert_eq!(out_c1.status.code(), Some(0));
    let stdout_c1 = String::from_utf8_lossy(&out_c1.stdout);
    assert!(stdout_c1.contains("Note on c1 file a"));
    assert!(stdout_c1.contains("Note on c1 file b"));
    assert!(!stdout_c1.contains("Note on c2 file a"));
    assert!(!stdout_c1.contains("Note on c2 doc"));

    // 3. `diffuse notes list -- src/a.txt`: scoped to working tree HEAD (c2),
    //    filtered to src/a.txt
    let out_file = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("--")
        .arg("src/a.txt")
        .output()
        .unwrap();
    assert_eq!(out_file.status.code(), Some(0));
    let stdout_file = String::from_utf8_lossy(&out_file.stdout);
    assert!(stdout_file.contains("Note on c2 file a"));
    assert!(!stdout_file.contains("Note on c2 doc"));
    assert!(!stdout_file.contains("Note on c1 file a"));
    // 4. `diffuse notes list HEAD~1 -- src/b.txt`: scoped to c1 and filtered to
    //    src/b.txt
    let out_c1_b = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("HEAD~1")
        .arg("--")
        .arg("src/b.txt")
        .output()
        .unwrap();
    assert_eq!(out_c1_b.status.code(), Some(0));
    let stdout_c1_b = String::from_utf8_lossy(&out_c1_b.stdout);
    assert!(stdout_c1_b.contains("Note on c1 file b"));
    assert!(!stdout_c1_b.contains("Note on c1 file a"));
    assert!(!stdout_c1_b.contains("Note on c2 file a"));

    // 5. `diffuse notes list HEAD~1 -- src/a.txt`: using -- separator syntax
    let out_dash = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("HEAD~1")
        .arg("--")
        .arg("src/a.txt")
        .output()
        .unwrap();
    assert_eq!(out_dash.status.code(), Some(0));
    let stdout_dash = String::from_utf8_lossy(&out_dash.stdout);
    assert!(stdout_dash.contains("Note on c1 file a"));
    assert!(!stdout_dash.contains("Note on c1 file b"));

    // 6. Shorthand `diffuse notes HEAD~1`: defaults to list action
    let out_short_rev = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("HEAD~1")
        .output()
        .unwrap();
    assert_eq!(out_short_rev.status.code(), Some(0));
    let stdout_short_rev = String::from_utf8_lossy(&out_short_rev.stdout);
    assert!(stdout_short_rev.contains("Note on c1 file a"));
    assert!(stdout_short_rev.contains("Note on c1 file b"));
    assert!(!stdout_short_rev.contains("Note on c2 file a"));

    // 7. Shorthand `diffuse notes -- doc/readme.txt`: defaults to list with path
    //    filter
    let out_short_path = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("--")
        .arg("doc/readme.txt")
        .output()
        .unwrap();
    assert_eq!(out_short_path.status.code(), Some(0));
    let stdout_short_path = String::from_utf8_lossy(&out_short_path.stdout);
    assert!(stdout_short_path.contains("Note on c2 doc"));
    assert!(!stdout_short_path.contains("Note on c2 file a"));
    assert!(!stdout_short_path.contains("Note on c1"));

    // 8. `diffuse notes list --staged`: scopes to working tree HEAD (c2)
    let out_staged = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("test-repo")
        .arg("notes")
        .arg("list")
        .arg("--staged")
        .output()
        .unwrap();
    assert_eq!(out_staged.status.code(), Some(0));
    let stdout_staged = String::from_utf8_lossy(&out_staged.stdout);
    assert!(stdout_staged.contains("Note on c2 file a"));
    assert!(stdout_staged.contains("Note on c2 doc"));
    assert!(!stdout_staged.contains("Note on c1"));
}

#[test]
fn test_cli_notes_markdown_highlighting() {
    use diffuse::core::notes::{NoteStatus, NotesDB, ReviewNote, add_note, save_notes};

    let dir = tempfile::tempdir().unwrap();
    let notes_path = dir.path().join("notes.json");
    let mut db = NotesDB::new();
    let note = ReviewNote {
        id: "note-md".into(),
        file: "src/main.rs".into(),
        location: diffuse::core::notes::NoteLocation::line(
            diffuse::core::notes::DiffCoord::new_side(42),
        ),
        text:
            "# Important\nPlease inspect **this** change and `call_fn()`:\n```rust\nlet x = 1;\n```"
                .into(),
        created_at: 1000,
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    };
    add_note(&mut db, "repo1", "", note);
    save_notes(&notes_path, &db).unwrap();

    let notes_file_arg = notes_path.to_str().unwrap();

    // 1. Without color: plain markdown preserved verbatim with 2-space indentation
    let out_plain = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("--color=never")
        .arg("notes")
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(out_plain.status.code(), Some(0));
    let stdout_plain = String::from_utf8_lossy(&out_plain.stdout);
    assert!(stdout_plain.contains("  # Important"));
    assert!(stdout_plain.contains("  Please inspect **this** change and `call_fn()`:"));
    assert!(stdout_plain.contains("  ```rust"));
    assert!(stdout_plain.contains("  let x = 1;"));
    assert!(stdout_plain.contains("  ```"));
    assert!(!stdout_plain.contains("\x1b["));

    // 2. With --color=always: ANSI SGR highlighting applied to markdown tokens and
    //    fences
    let out_color = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("--color=always")
        .arg("notes")
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(out_color.status.code(), Some(0));
    let stdout_color = String::from_utf8_lossy(&out_color.stdout);
    assert!(stdout_color.contains("  "));
    assert!(stdout_color.contains("Important"));
    assert!(stdout_color.contains("\x1b[1m"));
    assert!(stdout_color.contains("this"));
    assert!(stdout_color.contains("call_fn()"));
    assert!(stdout_color.contains("let"));

    // 3. With show command and --color=always
    let out_show = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(dir.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo-key")
        .arg("repo1")
        .arg("--color=always")
        .arg("notes")
        .arg("show")
        .arg("note-md")
        .output()
        .unwrap();
    assert_eq!(out_show.status.code(), Some(0));
    let stdout_show = String::from_utf8_lossy(&out_show.stdout);
    assert!(stdout_show.contains("Text:\n"));
    assert!(stdout_show.contains("Important"));
    assert!(stdout_show.contains("\x1b[1m"));
}

/// Regression: one-shot runs must never emit the OSC 11 background query
/// (`ESC ] 11 ; ?`). The terminal answers with its background color as
/// *input*; when the 30ms read times out, the late response leaks into the
/// shell as echoed garbage and phantom typed text (`11;rgb:...`).
/// Only the interactive TUI (which owns the terminal for its whole
/// lifetime) may query. Covers both the reported `diffuse -p` and the bare
/// early-exit path (which never reaches TUI takeover).
#[cfg(unix)]
#[test]
fn pty_one_shot_run_never_emits_osc11_background_query() {
    use std::io::Read;

    for extra_args in [&[] as &[&str], &["-p"] as &[&str]] {
        let pty = nix::pty::openpty(None, None).expect("openpty failed");
        let mut master = File::from(pty.master);
        let slave = File::from(pty.slave);

        // Scrub every env indicator so detection can only succeed via query;
        // force a query-capable TERM so the test is not vacuous.
        let dir = tempfile::tempdir().unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
            .current_dir(dir.path())
            .args(extra_args)
            .stdin(slave.try_clone().unwrap())
            .stdout(slave)
            .stderr(Stdio::piped())
            .env("TERM", "xterm")
            .env_remove("DIFFUSE_THEME")
            .env_remove("COLORFGBG")
            .env_remove("TERM_BACKGROUND")
            .spawn()
            .unwrap();

        // Drain the pty master on a side thread. Break on EOF *or* read error:
        // Linux reports EIO instead of EOF once the slave side closes.
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 256];
            loop {
                match master.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            buf
        });

        let out = child.wait_with_output().unwrap();
        let pty_bytes = reader.join().expect("pty reader thread panicked");

        assert_eq!(
            out.status.code(),
            Some(2),
            "must exit 2 on non-repo dir (args {extra_args:?}, stderr: {})",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !pty_bytes.windows(5).any(|w| w == b"]11;?"),
            "one-shot run {extra_args:?} must not emit the OSC 11 query, pty saw: {pty_bytes:?}"
        );
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Golden acceptance test suites store historical corpus snapshots
                // which must not be inspected as live workspace code.
                if entry.file_name() != "golden" {
                    collect_rs_files(&path, out);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// Regression and architectural guard: verify that zero subprocess git
/// invocations exist anywhere across any Rust source file in the workspace.
#[test]
fn assert_zero_git_subprocess_invocations_in_workspace() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let mut rs_files = Vec::new();

    for dir_name in ["crates", "tests"] {
        collect_rs_files(&workspace_root.join(dir_name), &mut rs_files);
    }

    let mut violations = Vec::new();
    for path in rs_files {
        let content = fs::read_to_string(&path).unwrap();
        for (idx, line) in content.lines().enumerate() {
            if line.contains("Command::new(\"git\")") || line.contains("Command::new(\"git.exe\")")
            {
                violations.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Detected forbidden git subprocess invocations:\n{}",
        violations.join("\n")
    );
}
