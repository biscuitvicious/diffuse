//! CLI routing integration tests: piped/redirected invocations must never
//! fall into the extension JSON contract, and non-interactive output must
//! never try to open the interactive TUI.

use std::{
    fs,
    process::{Command, Stdio},
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
    std::io::Write::write_all(&mut stdin, diff.as_bytes()).unwrap();
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
                assert!(!l.text.is_empty());
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

    let start = std::time::Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--no-tui")
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut first_line = String::new();
    std::io::BufRead::read_line(&mut stdout, &mut first_line).unwrap();
    let time_to_first_file = start.elapsed();

    assert!(
        !first_line.is_empty(),
        "must yield first file line progressively"
    );
    assert!(
        time_to_first_file < std::time::Duration::from_millis(500),
        "time to first file ({time_to_first_file:?}) must be under 500ms"
    );

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
        stdout.contains("file_a.rs") || stdout.contains("modified"),
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

    use std::io::Write as _;
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

    use std::io::Write as _;
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

    use std::io::Write;
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

    use std::io::Write;
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
