//! CLI routing integration tests: piped/redirected invocations must never
//! fall into the extension JSON contract, and non-interactive output must
//! never try to open the interactive TUI.

use std::{
    env, fs,
    path::PathBuf,
    process::{self, Command, Stdio},
};

/// `diffuse < /dev/null | cat` with no git args and no JSON payload must
/// route to the standalone reviewer (git repo resolution), never to the
/// JSON contract — which would silently swallow the empty stdin and emit an
/// empty JSON document with exit 0.
#[test]
fn empty_piped_stdin_routes_to_standalone_reviewer() {
    // A provably NON-git dir: the standalone path fails with the repo error
    // (exit 2); the JSON path would exit 0 with `{"files":[]}`.
    let dir = env::temp_dir().join(format!("diffuse-cli-route-{}", process::id()));
    fs::create_dir_all(&dir).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take()); // close stdin immediately: EOF, no payload
    let out = child.wait_with_output().unwrap();
    let _ = fs::remove_dir_all(&dir);

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
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let _ = fs::remove_dir_all(&dir);

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

static REPO_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

fn setup_repo() -> PathBuf {
    let id = REPO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("diffuse-cli-repo-{}-{}", process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&dir)
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
    fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
    fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(dir.join("b.txt"), "hello\nworld\n").unwrap();
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
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "must emit exactly two JSON lines");

    for line in lines {
        let val: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
        assert!(val.get("path").is_some());
        assert!(val.get("counts").is_some());
        assert!(val.get("hunks").is_some());
    }
}

#[test]
fn non_tui_mode_handles_broken_pipe_cleanly() {
    let dir = setup_repo();
    let mut child = Command::new(env!("CARGO_BIN_EXE_diffuse"))
        .arg("--no-tui")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // Drop stdout pipe reader immediately to simulate `diffuse | head -n 1`
    drop(child.stdout.take());
    let _ = child.wait();

    let _ = fs::remove_dir_all(&dir);
}
