//! CLI routing integration tests: piped/redirected invocations must never
//! fall into the extension JSON contract, and non-interactive output must
//! never try to open the interactive TUI.

use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
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

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);

    // Create 60 files
    for i in 0..60 {
        fs::write(
            dir.path().join(format!("file_{i}.rs")),
            format!("fn func_{i}() {{\n    println!(\"hello {i}\");\n}}\n"),
        )
        .unwrap();
    }
    run(&["add", "."]);
    run(&["commit", "-qm", "init commit"]);

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

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&child_repo)
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    fs::write(child_repo.join("nested.rs"), "fn nested() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "nested init"]);
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
    // Number gutter and change type symbols are styled
    assert!(
        stdout.contains("\x1b[38;5;120m+") || stdout.contains("\x1b[38;5;203m-"),
        "change type symbols must be colorized, got: {stdout}"
    );
    assert!(!stdout.trim_start().starts_with('{'), "must not be JSON");
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

/// The JSON payload stdin contract (`{diff, contents, oldContents}` → wire
/// JSON) — the shape the pi extension's backend pipes into `diffuse --json`.
#[test]
fn test_json_payload_stdin_wire_contract() {
    let payload = r#"{
        "diff": "diff --git a/test.rs b/test.rs\nindex 0000000..1111111 100644\n--- a/test.rs\n+++ b/test.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "contents": { "test.rs": "new\n" },
        "oldContents": { "test.rs": "old\n" }
    }"#;

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
        .write_all(payload.as_bytes())
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

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    fs::write(dir.join("tracked.rs"), "fn tracked() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);

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
    use diffuse::core::notes::{
        NoteStatus, NotesDB, ReviewNote, Side, add_note, load_notes, save_notes,
    };

    let dir = tempfile::tempdir().unwrap();
    let notes_path = dir.path().join("notes.json");
    let mut db = NotesDB::new();

    let note_open = ReviewNote {
        id: "note-open".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 10,
        end: 10,
        text: "Fix this issue".into(),
        created_at: 1000,
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: Some("fn original() {}".into()),
    };
    let note_prog = ReviewNote {
        id: "note-prog".into(),
        file: "src/lib.rs".into(),
        side: Side::New,
        start: 20,
        end: 20,
        text: "Working on refactor".into(),
        created_at: 1001,
        line_hash: None,
        status: NoteStatus::InProgress,
        source_lines: None,
    };
    let note_res = ReviewNote {
        id: "note-res".into(),
        file: "src/main.rs".into(),
        side: Side::New,
        start: 30,
        end: 30,
        text: "Already resolved".into(),
        created_at: 1002,
        line_hash: None,
        status: NoteStatus::Resolved,
        source_lines: None,
    };

    add_note(&mut db, "repo1", note_open);
    add_note(&mut db, "repo1", note_prog);
    add_note(&mut db, "repo1", note_res);
    save_notes(&notes_path, &db).unwrap();

    let notes_file_arg = notes_path.to_str().unwrap();

    // 1. Default notes list: shows Open and InProgress, with resolved summary count
    let out = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
    assert!(stdout_show.contains("Fix this issue"));

    // 7. Reply with --resolve
    let out_reply = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::Resolved);
    assert_eq!(note.text, "I have addressed it\n\n> Fix this issue");

    // 8. Progress
    let out_mark_prog = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::InProgress);

    // 9. Open / Reopen
    let out_open = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
        .get("src/lib.rs")
        .unwrap()
        .iter()
        .find(|n| n.id == "note-open")
        .unwrap();
    assert_eq!(note.status, NoteStatus::Open);

    // 10. Status explicit
    let out_status = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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

    let foo_path = tmp.path().join("foo.rs");
    fs::write(
        &foo_path,
        "fn first() {}\nfn second() {}\nfn third() {}\nfn fourth() {}\n",
    )
    .unwrap();

    // 1. Add note with coordinates in target string and synthesized source lines
    let out1 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
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
    let notes = db.get("test-repo").unwrap().get("foo.rs").unwrap();
    assert_eq!(notes.len(), 1);
    let n1 = &notes[0];
    assert_eq!(n1.file, "foo.rs");
    assert_eq!(n1.start, 1);
    assert_eq!(n1.end, 2);
    assert_eq!(n1.status, NoteStatus::Open);
    assert_eq!(n1.text, "First two functions");
    assert_eq!(
        n1.source_lines,
        Some("fn first() {}\nfn second() {}".to_string())
    );

    // 2. Add note with positional start/end and explicit source lines
    let out2 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs")
        .arg("3")
        .arg("4")
        .arg("-m")
        .arg("Explicit source lines")
        .arg("--source")
        .arg("custom source lines")
        .arg("--status")
        .arg("in-progress")
        .output()
        .unwrap();
    assert_eq!(out2.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let notes = db.get("test-repo").unwrap().get("foo.rs").unwrap();
    assert_eq!(notes.len(), 2);
    let n2 = &notes[1];
    assert_eq!(n2.start, 3);
    assert_eq!(n2.end, 4);
    assert_eq!(n2.status, NoteStatus::InProgress);
    assert_eq!(n2.source_lines, Some("custom source lines".to_string()));

    // 3. Add note with --no-source
    let out3 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("foo.rs:2")
        .arg("-m")
        .arg("Without source")
        .arg("--no-source")
        .output()
        .unwrap();
    assert_eq!(out3.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let notes = db.get("test-repo").unwrap().get("foo.rs").unwrap();
    assert_eq!(notes.len(), 3);
    let n3 = &notes[2];
    assert_eq!(n3.source_lines, None);

    // 4. Add note on missing file (source lines not possible -> None)
    let out4 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
        .arg("test-repo")
        .arg("notes")
        .arg("add")
        .arg("missing.rs:10")
        .arg("-m")
        .arg("Missing file note")
        .output()
        .unwrap();
    assert_eq!(out4.status.code(), Some(0));
    let db = load_notes(&notes_path);
    let notes_missing = db.get("test-repo").unwrap().get("missing.rs").unwrap();
    assert_eq!(notes_missing.len(), 1);
    assert_eq!(notes_missing[0].source_lines, None);

    // 5. Using alias 'create' and --json
    let out5 = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(tmp.path())
        .arg("--notes-file")
        .arg(notes_file_arg)
        .arg("--repo")
        .arg("test-repo")
        .arg("notes")
        .arg("create")
        .arg("foo.rs:1")
        .arg("-m")
        .arg("JSON create")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out5.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out5.stdout).unwrap();
    assert_eq!(v["file"], "foo.rs");
    assert_eq!(v["start"], 1);
    assert_eq!(v["text"], "JSON create");
}
