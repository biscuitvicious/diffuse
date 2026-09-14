use std::collections::HashMap;

use super::*;
use crate::{
    lcs::word_spans,
    merge::{RunKind, display_text},
    parse::{AddLine, DelLine, DiffFile, LineNum, Row, RowKind},
};

#[test]
fn test_fence_delimiter_line_preserves_comment_prefix_color() {
    let profile = lang_profile("rs");
    let regions = vec![FenceRegion {
        start: 2,
        end: 3,
        lang: Some("rs".to_string()),
    }];
    let tokens = classify_ctx_line("/// ```ignore", Some(1), profile, &regions);
    assert_eq!(
        tokens.len(),
        2,
        "delimiter line produces 2 tokens (prefix + fence)"
    );
    assert_eq!(
        tokens[0].color, "syntaxComment",
        "comment prefix is syntaxComment"
    );
    assert_eq!(tokens[0].text, "/// ");
    assert_eq!(tokens[1].color, "muted", "fence is muted");
    assert_eq!(tokens[1].text, "```ignore");
}

#[test]
fn test_mod_comment_row_preserves_comment_color_after_subword_fill() {
    let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,1 +1,1 @@\n-// The lowest layer the lower-walk may descend to: a directory found with\n+// The deepest layer the below-walk may descend to: a directory found with\n";
    let mut new_content = HashMap::new();
    new_content.insert(
        "test.rs".to_string(),
        "// The deepest layer the below-walk may descend to: a directory found with\n".to_string(),
    );
    let output = run_pipeline(diff, Some(new_content));
    let row = &output.files[0].hunks[0].rows[0];
    let common_after = row.runs.iter().find(|r| r.text.contains("layer the"));
    assert!(common_after.is_some(), "common run 'layer the' exists");
    assert_eq!(
        common_after.unwrap().color.as_deref(),
        Some("syntaxComment"),
        "text after fill preserves syntaxComment color"
    );
}

#[test]
fn test_comment_reflow_del_span_positioning() {
    // Row 1078: `lowers` is deleted before `non-top-upper layers` is added, and
    // trailing `layer's markers —` lands at the end
    let old_1078 = "// Walk the lowers top → bottom, honoring each layer's markers —";
    let new_1078 = "// Walk the non-top-upper layers top → bottom, honoring each";
    let r_1078 = Row::Mod {
        old_line: DelLine::new(LineNum::new(1078).unwrap(), old_1078),
        new_line: AddLine::with_spans(
            LineNum::new(1078).unwrap(),
            new_1078,
            word_spans(old_1078, new_1078),
        ),
    };
    let runs_1078 = merged_runs(&r_1078);
    let non_common_1078: Vec<(RunKind, &str)> = runs_1078
        .iter()
        .filter(|r| r.kind != RunKind::Common)
        .map(|r| (r.kind, r.text))
        .collect();
    assert_eq!(
        non_common_1078,
        vec![
            (RunKind::Del, "lowers"),
            (RunKind::Add, "non-top-upper layers"),
            (RunKind::Del, " layer's markers —"),
        ],
        "1078: lowers deletes before non-top-upper layers adds, layer's markers strikes at end"
    );

    // Row 1079: `layer's markers — ` adds at start, `upper`->`layer` pairs,
    // `whiteouts...` strikes at end of `carries` (not inside `ca...rries`)
    let old_1079 = "// a frozen (snapshotted) upper carries whiteouts and opaque dirs";
    let new_1079 = "// layer's markers — a frozen (snapshotted) layer carries";
    let r_1079 = Row::Mod {
        old_line: DelLine::new(LineNum::new(1079).unwrap(), old_1079),
        new_line: AddLine::with_spans(
            LineNum::new(1079).unwrap(),
            new_1079,
            word_spans(old_1079, new_1079),
        ),
    };
    let runs_1079 = merged_runs(&r_1079);
    let non_common_1079: Vec<(RunKind, &str)> = runs_1079
        .iter()
        .filter(|r| r.kind != RunKind::Common)
        .map(|r| (r.kind, r.text))
        .collect();
    assert_eq!(
        non_common_1079,
        vec![
            (RunKind::Add, "layer's markers — "),
            (RunKind::Del, "upper"),
            (RunKind::Add, "layer"),
            (RunKind::Del, " whiteouts and opaque dirs"),
        ],
        "1079: layer's markers adds at start, upper->layer pairs, whiteouts strikes at end of carries"
    );
}

#[test]
fn test_wire_reconstruct_and_reverse() {
    let diff = "diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,1 +10,2 @@ fn old() {
-old line one
+new line one
+new line two
";
    let mut new_content = HashMap::new();
    new_content.insert(
        "src/a.rs".to_string(),
        "new line one\nnew line two".to_string(),
    );

    let wire = run_pipeline(diff, Some(new_content));
    let reconstructed_files = wire.reconstruct();
    assert_eq!(reconstructed_files.len(), 1);
    assert_eq!(reconstructed_files[0].path, "src/a.rs");

    // Involution on wire output: wire.reverse().reverse() == wire
    let wire_rev = wire.reverse();
    assert_eq!(wire_rev.reverse(), wire);

    // Reconstruct from reverse wire matches reversed AST files
    let rev_files_from_wire = wire_rev.reconstruct();
    let rev_files_direct: Vec<DiffFile> =
        reconstructed_files.iter().map(DiffFile::reverse).collect();
    assert_eq!(rev_files_from_wire[0].path, rev_files_direct[0].path);
    assert_eq!(
        rev_files_from_wire[0].hunks.len(),
        rev_files_direct[0].hunks.len()
    );
    for (h1, h2) in rev_files_from_wire[0]
        .hunks
        .iter()
        .zip(rev_files_direct[0].hunks.iter())
    {
        assert_eq!(h1.old_start, h2.old_start);
        assert_eq!(h1.new_start, h2.new_start);
        assert_eq!(h1.rows.len(), h2.rows.len());
        for (r1, r2) in h1.rows.iter().zip(h2.rows.iter()) {
            assert_eq!(r1.kind(), r2.kind());
            assert_eq!(r1.text(), r2.text());
            assert_eq!(r1.new_line(), r2.new_line());
            assert_eq!(r1.old_line(), r2.old_line());
        }
    }
}

#[test]
fn test_run_pipeline_full_with_old_contents() {
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -250,5 +250,5 @@\n                     origin: Mount::Overlay {\n-                    resolved_layers: unresolved,\n+                    layers: unresolved,\n";
    let mut new_content = HashMap::new();
    new_content.insert(
        "src/lib.rs".to_string(),
        "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        layers: unresolved,\n    })\n}".to_string(),
    );
    let mut old_content = HashMap::new();
    old_content.insert(
        "src/lib.rs".to_string(),
        "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        resolved_layers: unresolved,\n    })\n}".to_string(),
    );
    let out = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let mod_row = out.files[0].hunks[0]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Mod);
    assert!(
        mod_row.is_some(),
        "resolved_layers and layers pair into a Mod row"
    );
    let runs = &mod_row.unwrap().runs;
    let del_run = runs.iter().find(|r| r.kind == RunKind::Del);
    assert_eq!(del_run.map(|r| r.text.as_ref()), Some("resolved_"));
}

#[test]
fn test_cargo_config_toml_runner_pairing() {
    let diff = r#"diff --git a/.cargo/config.toml b/.cargo/config.toml
index 759adaf..fbe3b20 100644
--- a/.cargo/config.toml
+++ b/.cargo/config.toml
@@ -1,5 +1,9 @@
 [env]
 CARGO_WORKSPACE_DIR = { value = "", relative = true }
+RUST_TEST_THREADS = { value = "4" }
+
+[alias]
+regen = "run -p linux --bin mirage-registry-gen --target x86_64-unknown-linux-gnu -- src/registry/generated.rs"
 
 [build]
 target = [ "x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu" ]
@@ -7,23 +11,30 @@ target = [ "x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu" ]
 [target.x86_64-unknown-linux-musl]
 linker = "clang"
 rustflags = [ "-C", "target-feature=+crt-static", "-C", "link-arg=-fuse-ld=mold" ]
-runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then echo "Running on x86_64: $*"; "$@"; else echo "WARNING: Skipping runner on non-x86_64 arch (got $(uname -m))"; exit 1; fi', "--"]
+runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then "$@"; fi', "--"]
 
 [target.x86_64-unknown-linux-gnu]
 linker = "clang"
 rustflags = ["-C", "link-arg=-fuse-ld=mold"]
-runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then echo "Running on x86_64: $*"; "$@"; else echo "WARNING: Skipping runner on non-x86_64 arch (got $(uname -m))"; exit 1; fi', "--"]
+runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then "$@"; fi', "--"]
 
 [target.aarch64-unknown-linux-musl.env]
 RUST_TEST_THREADS = "1"
 
 [target.aarch64-unknown-linux-musl]
 linker = "aarch64-linux-gnu-gcc"
-rustflags = ["-C", "target-feature=+crt-static"]
-runner = "qemu-aarch64-static"
+rustflags = ["-C", "target-feature=+crt-static", "--cfg", 'remote_exec="adb"']
+runner = ["sh", "-c", 'BIN=${1##*/}; if [ -n "$MIRAGE_REMOTE_HOST" ] && [ ! "${BIN#e2e-}" = "$BIN" ]; then $CARGO_WORKSPACE_DIR/scripts/android-test-runner "$@"; else qemu-aarch64-static "$@"; fi', "--"]
 
 [target.aarch64-linux-android]
 # Linker is provided via CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER in the environment
 # to keep this file agnostic of the local build environment.
-rustflags = []
-runner = ["sh", "-c", 'if [ "$(uname -m)" = "aarch64" ]; then "$@"; fi', "--"]
+rustflags = ["--cfg", 'remote_exec="adb"']
+runner = "scripts/android-test-runner"
+
+[target.x86_64-linux-android]
+# Linker is provided via CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER in the environment
+# to keep this file agnostic of the local build environment.
+rustflags = ["--cfg", 'remote_exec="adb"']
+runner = "scripts/android-test-runner"
+"#;
    let mut new_content = HashMap::new();
    new_content.insert(
        ".cargo/config.toml".to_string(),
        r#"[env]
CARGO_WORKSPACE_DIR = { value = "", relative = true }
RUST_TEST_THREADS = { value = "4" }

[alias]
regen = "run -p linux --bin mirage-registry-gen --target x86_64-unknown-linux-gnu -- src/registry/generated.rs"

[build]
target = [ "x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu" ]

[target.x86_64-unknown-linux-musl]
linker = "clang"
rustflags = [ "-C", "target-feature=+crt-static", "-C", "link-arg=-fuse-ld=mold" ]
runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then "$@"; fi', "--"]

[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
runner = ["sh", "-c", 'if [ "$(uname -m)" = "x86_64" ]; then "$@"; fi', "--"]

[target.aarch64-unknown-linux-musl.env]
RUST_TEST_THREADS = "1"

[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-gnu-gcc"
rustflags = ["-C", "target-feature=+crt-static", "--cfg", 'remote_exec="adb"']
runner = ["sh", "-c", 'BIN=${1##*/}; if [ -n "$MIRAGE_REMOTE_HOST" ] && [ ! "${BIN#e2e-}" = "$BIN" ]; then $CARGO_WORKSPACE_DIR/scripts/android-test-runner "$@"; else qemu-aarch64-static "$@"; fi', "--"]

[target.aarch64-linux-android]
# Linker is provided via CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER in the environment
# to keep this file agnostic of the local build environment.
rustflags = ["--cfg", 'remote_exec="adb"']
runner = "scripts/android-test-runner"

[target.x86_64-linux-android]
# Linker is provided via CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER in the environment
# to keep this file agnostic of the local build environment.
rustflags = ["--cfg", 'remote_exec="adb"']
runner = "scripts/android-test-runner"
"#.to_string(),
    );

    let output = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let rows = &output.files[0].rows;

    // 1. Assert that there is NO floating del under [env] (between lines 1 and
    //    6)
    for r in &rows[..6] {
        assert_ne!(
            r.kind,
            RowKind::Del,
            "no del rows should float into the top [env] section"
        );
    }

    // 2. Assert that runner in x86_64-unknown-linux-musl is paired as 1-to-1
    //    Mod (old 10 -> new 14)
    let musl_runner = rows.iter().find(|r| r.new_line == Some(14));
    assert!(musl_runner.is_some());
    assert_eq!(musl_runner.unwrap().kind, RowKind::Mod);
    assert_eq!(musl_runner.unwrap().old_line, Some(10));

    // 3. Assert that runner in x86_64-unknown-linux-gnu is paired as 1-to-1 Mod
    //    (old 15 -> new 19)
    let gnu_runner = rows.iter().find(|r| r.new_line == Some(19));
    assert!(gnu_runner.is_some());
    assert_eq!(gnu_runner.unwrap().kind, RowKind::Mod);
    assert_eq!(gnu_runner.unwrap().old_line, Some(15));

    // 4. Assert that runner in aarch64-unknown-linux-musl is paired as 1-to-1
    //    Mod (old 23 -> new 27)
    let aarch64_musl_runner = rows.iter().find(|r| r.new_line == Some(27));
    assert!(aarch64_musl_runner.is_some());
    assert_eq!(aarch64_musl_runner.unwrap().kind, RowKind::Mod);
    assert_eq!(aarch64_musl_runner.unwrap().old_line, Some(23));

    // 5. Assert that runner in aarch64-linux-android is paired as 1-to-1 Mod
    //    (old 29 -> new 33)
    let android_runner = rows.iter().find(|r| r.new_line == Some(33));
    assert!(android_runner.is_some());
    assert_eq!(android_runner.unwrap().kind, RowKind::Mod);
    assert_eq!(android_runner.unwrap().old_line, Some(29));

    // 6. Assert that rustflags in aarch64-linux-android is paired as 1-to-1 Mod
    //    (old 28 -> new 32)
    let android_rustflags = rows.iter().find(|r| r.new_line == Some(32));
    assert!(android_rustflags.is_some());
    assert_eq!(android_rustflags.unwrap().kind, RowKind::Mod);
    assert_eq!(android_rustflags.unwrap().old_line, Some(28));
}

#[test]
fn test_visible_context_rows_populate_text() {
    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn foo() {
-    let x = 1;
+    let x = 2;
 }
"#;
    let new_content_str = "fn foo() {\n    let x = 2;\n}";
    let mut new_content = HashMap::new();
    new_content.insert("src/lib.rs".to_string(), new_content_str.to_string());

    let output = run_pipeline(diff, Some(new_content));
    assert_eq!(output.files.len(), 1);
    let file = &output.files[0];
    assert!(
        !file.new_content.is_empty(),
        "file.new_content must not be empty"
    );

    let ctx_rows: Vec<_> = file
        .rows
        .iter()
        .filter(|r| r.kind == RowKind::Ctx)
        .collect();
    assert_eq!(
        ctx_rows.len(),
        2,
        "must have visible context rows: 'fn foo() {{' and '}}'"
    );
    assert_eq!(ctx_rows[0].text, "fn foo() {");
    assert_eq!(ctx_rows[1].text, "}");
}

#[test]
fn test_run_pipeline_with_no_contents_autonomous() {
    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,4 @@
 fn foo() {
-    let x = 1;
+    let x = 2;
     return x;
 }
"#;
    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let file = &output.files[0];
    assert_eq!(file.path, "src/lib.rs");
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].rows.len(), 4); // Ctx, Mod, Ctx, Ctx
    assert_eq!(file.new_content.len(), 4);
    assert_eq!(file.old_content.len(), 4);
}

#[test]
fn leading_indent_shift_row_runs_satisfy_wire_invariants() {
    // The 12→20 space indent shift with a mid-line rewrite (merge/mod.rs:558
    // in the diffuse-history-mid corpus): the wire runs must concatenate to
    // the row's display text, and stripping the del runs must leave exactly
    // the new line.
    let diff = "\
diff --git a/src/merge/mod.rs b/src/merge/mod.rs
index 1111111..2222222 100644
--- a/src/merge/mod.rs
+++ b/src/merge/mod.rs
@@ -556,8 +556,8 @@
         }
             if same_kind && share > 0 {
-            last.text.push_str(&run.text);
+                    last.text.len() + run.text.len(),
             } else {
                 out.push(run.clone());
";
    let output = run_pipeline(diff, None);
    let mut checked = 0;
    for f in &output.files {
        for h in &f.hunks {
            for r in &h.rows {
                if r.kind != RowKind::Mod {
                    continue;
                }
                checked += 1;
                let concat: String = r.runs.iter().map(|x| x.text.as_ref()).collect();
                assert_eq!(
                    concat,
                    display_text(&Row::from(r)),
                    "runs must concatenate to the display text"
                );
                let kept: String = r
                    .runs
                    .iter()
                    .filter(|x| x.kind != RunKind::Del)
                    .map(|x| x.text.as_ref())
                    .collect();
                assert_eq!(kept, r.text, "strike-removed runs must be the new line");
            }
        }
    }
    assert_eq!(checked, 1, "the crafted diff yields exactly one mod row");
}

#[test]
fn test_moved_line_source_in_source_hunk_and_target_in_target_hunk() {
    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,2 @@
 fn alpha() {
-    moved_statement();
     remain_a();
 }
@@ -50,2 +49,3 @@
 fn beta() {
+    moved_statement();
     remain_b();
 }
"#;
    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let file = &output.files[0];
    assert_eq!(file.hunks.len(), 2);

    // Hunk 0 (source) has the MovSource row
    let h0_mov_src = file.hunks[0]
        .rows
        .iter()
        .find(|r| r.mov_side == Some(MovSide::Src));
    assert!(
        h0_mov_src.is_some(),
        "Hunk 0 (source) must contain the MovSide::Src row"
    );
    let src_row = h0_mov_src.unwrap();
    assert_eq!(src_row.old_line, Some(11));
    assert_eq!(src_row.new_line, None);
    assert_eq!(src_row.text, "    moved_statement();");

    // Hunk 0 must NOT contain MovSide::Tgt
    assert!(
        file.hunks[0]
            .rows
            .iter()
            .all(|r| r.mov_side != Some(MovSide::Tgt)),
        "Hunk 0 must not contain MovSide::Tgt"
    );

    // Hunk 1 (target) has the MovTarget row
    let h1_mov_tgt = file.hunks[1]
        .rows
        .iter()
        .find(|r| r.mov_side == Some(MovSide::Tgt));
    assert!(
        h1_mov_tgt.is_some(),
        "Hunk 1 (target) must contain the MovSide::Tgt row"
    );
    let tgt_row = h1_mov_tgt.unwrap();
    assert_eq!(tgt_row.new_line, Some(50));
    assert_eq!(tgt_row.text, "    moved_statement();");

    // Hunk 1 must NOT contain MovSide::Src
    assert!(
        file.hunks[1]
            .rows
            .iter()
            .all(|r| r.mov_side != Some(MovSide::Src)),
        "Hunk 1 must not contain MovSide::Src"
    );

    // In flattened file.rows: the source and target rows must be in separate
    // positions
    let src_idx = file
        .rows
        .iter()
        .position(|r| r.mov_side == Some(MovSide::Src))
        .expect("source row in file.rows");
    let tgt_idx = file
        .rows
        .iter()
        .position(|r| r.mov_side == Some(MovSide::Tgt))
        .expect("target row in file.rows");

    assert!(
        src_idx < tgt_idx,
        "Source row must appear before target row in file.rows"
    );
    // They must not be adjacent (they belong to separate hunks)
    assert!(
        tgt_idx - src_idx > 1,
        "Source and target rows must not be adjacent when separated by hunks: src at {src_idx}, tgt at {tgt_idx}"
    );
}

#[test]
fn test_pipeline_pure_rename_renders_entire_file_as_context() {
    let diff = "diff --git a/old.rs b/new.rs\nsimilarity index 100%\nrename from old.rs\nrename to new.rs\n";
    let mut new_content = HashMap::new();
    new_content.insert(
        "new.rs".to_string(),
        "fn alpha() {\n    println!(\"hello\");\n}\n".to_string(),
    );
    let output = run_pipeline(diff, Some(new_content));
    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.path, "new.rs");
    assert_eq!(f.old_path, Some("old.rs".to_string()));
    assert_eq!(f.status, "renamed");
    assert_eq!(f.counts.adds, 0);
    assert_eq!(f.counts.dels, 0);
    assert_eq!(f.counts.mods, 0);
    assert_eq!(f.counts.movs, 0);
    assert_eq!(f.hunks.len(), 1);
    assert_eq!(f.hunks[0].rows.len(), 3);
    assert_eq!(f.hunks[0].rows[0].kind, RowKind::Ctx);
    assert_eq!(f.hunks[0].rows[0].text, "fn alpha() {");
    assert_eq!(f.hunks[0].rows[0].new_line, Some(1));
    assert_eq!(f.hunks[0].rows[0].old_line, Some(1));
    assert_eq!(f.hunks[0].rows[1].kind, RowKind::Ctx);
    assert_eq!(f.hunks[0].rows[1].text, "    println!(\"hello\");");
    assert_eq!(f.hunks[0].rows[2].kind, RowKind::Ctx);
    assert_eq!(f.hunks[0].rows[2].text, "}");
    assert_eq!(f.rows.len(), 3);
}

#[test]
fn test_head_commit_hunk_render() {
    let diff = r#"diff --git a/crates/diffuse-core/src/moves/mod.rs b/crates/diffuse-core/src/moves/mod.rs
--- a/crates/diffuse-core/src/moves/mod.rs
+++ b/crates/diffuse-core/src/moves/mod.rs
@@ -143,18 +147,65 @@ pub fn mark_moves(f: &mut DiffFile) {
             if matches!(r, Row::Del(_)) {
                 run.push((hi, ri));
             } else {
-                // flush the run
-                if !run.is_empty() && !moved_dels.contains(&run[0]) {
-                    unmark_positions
-                        .extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));
+                if !run.is_empty() {
+                    let moved_in_run: Vec<RowPos> = run
+                        .iter()
+                        .copied()
+                        .filter(|pos| moved_dels.contains(pos))
+                        .collect();
+                    if run.len() > 1
+                        && (moved_in_run.len() < 2
+                            || !moved_dels.contains(&run[0])
+                            || moved_in_run.len() < run.len())
+                    {
+                        let prefix_len = run
+                            .iter()
+                            .take_while(|pos| moved_dels.contains(pos))
+                            .count();
+                        if prefix_len < 2 || prefix_len < moved_in_run.len() {
+                            unmark_positions.extend(moved_in_run);
+                        } else {
+                            unmark_positions.extend(
+                                run.iter()
+                                    .skip(prefix_len)
+                                    .copied()
+                                    .filter(|pos| moved_dels.contains(pos)),
+                            );
+                        }
+                    }
+                }
                 run.clear();
             }
         }
-        if !run.is_empty() && !moved_dels.contains(&run[0]) {
-            unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));
+        if !run.is_empty() {
+            let moved_in_run: Vec<RowPos> = run
+                .iter()
+                .copied()
+                .filter(|pos| moved_dels.contains(pos))
+                .collect();
+            if run.len() > 1
+                && (moved_in_run.len() < 2
+                    || !moved_dels.contains(&run[0])
+                    || moved_in_run.len() < run.len())
+            {
+                let prefix_len = run
+                    .iter()
+                    .take_while(|pos| moved_dels.contains(pos))
+                    .count();
+                if prefix_len < 2 || prefix_len < moved_in_run.len() {
+                    unmark_positions.extend(moved_in_run);
+                } else {
+                    unmark_positions.extend(
+                        run.iter()
+                            .skip(prefix_len)
+                            .copied()
+                            .filter(|pos| moved_dels.contains(pos)),
+                    );
+                }
+            }
         }
     }
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;

    let r150 = rows
        .iter()
        .find(|r| r.text.trim() == "if !run.is_empty() {")
        .expect("line 150 row");
    assert_eq!(
        r150.kind,
        crate::parse::RowKind::Mod,
        "line 150 should pair with the old if statement as Mod"
    );
    assert_eq!(
        r150.del_text.as_deref().map(str::trim),
        Some("if !run.is_empty() && !moved_dels.contains(&run[0]) {")
    );
    assert_eq!(
        r150.spans.del,
        vec![(35, 68)],
        "parentheses of !run.is_empty() must remain intact and not split"
    );
    assert_eq!(r150.spans.add, vec![]);

    let r156 = rows
        .iter()
        .find(|r| r.text.trim() == "if run.len() > 1")
        .expect("line 156 row");
    assert_eq!(
        r156.kind,
        crate::parse::RowKind::Add,
        "line 156 should be a clean pure Add"
    );
    assert_eq!(r156.del_text, None);

    let r166 = rows
        .iter()
        .find(|r| r.text.trim() == "unmark_positions.extend(moved_in_run);")
        .expect("line 166 row");
    assert_eq!(
        r166.kind,
        crate::parse::RowKind::Mod,
        "line 166 should pair with old receiver line 148 as Mod"
    );
    assert_eq!(
        r166.del_text.as_deref().map(str::trim),
        Some("unmark_positions")
    );

    let r_filter = rows
        .iter()
        .find(|r| r.text.trim() == ".filter(|pos| moved_dels.contains(pos)),")
        .expect("filter row");
    assert_eq!(r_filter.kind, crate::parse::RowKind::Mod);
    assert_eq!(
        r_filter.del_text.as_deref().map(str::trim),
        Some(".extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));")
    );
}

#[test]
fn test_pipeline_options_token_product_cap_controls_subword_granularity() {
    let diff = r#"diff --git a/test.txt b/test.txt
--- a/test.txt
+++ b/test.txt
@@ -1,1 +1,1 @@
-alpha beta gamma delta epsilon zeta eta theta iota kappa
+lambda mu nu xi omicron pi rho sigma tau upsilon
"#;

    let opts_low = PipelineOptions::default().with_token_product_cap(10);
    let out_low = run_pipeline_full(diff, None, None, opts_low);
    let row_low = &out_low.files[0].rows[0];
    assert_eq!(row_low.kind, crate::parse::RowKind::Del);

    let opts_high = PipelineOptions::default().with_token_product_cap(250_000);
    let out_high = run_pipeline_full(diff, None, None, opts_high);
    assert_eq!(out_high.files.len(), 1);
}

#[test]
fn test_pipeline_options_cluster_product_cap_controls_wholesale_fallback() {
    let mut diff = String::from(
        "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,15 +1,15 @@\n",
    );
    for i in 1..=15 {
        diff.push_str(&format!("-fn func_{i}() {{ let val = {i}; }}\n"));
    }
    for i in 1..=15 {
        diff.push_str(&format!("+fn func_{i}() {{ let val = {i} + 1; }}\n"));
    }

    let opts_low = PipelineOptions::default().with_cluster_product_cap(50);
    let out_low = run_pipeline_full(&diff, None, None, opts_low);
    // Under low cap (15 * 15 = 225 > 50), cluster falls back to wholesale pure
    // Del then pure Add
    assert!(
        out_low.files[0]
            .rows
            .iter()
            .all(|r| r.kind == crate::parse::RowKind::Del || r.kind == crate::parse::RowKind::Add)
    );

    let opts_high = PipelineOptions::default().with_cluster_product_cap(100_000);
    let out_high = run_pipeline_full(&diff, None, None, opts_high);
    // Under high cap, lines pair into Mod rows
    assert!(
        out_high.files[0]
            .rows
            .iter()
            .any(|r| r.kind == crate::parse::RowKind::Mod)
    );
}

#[test]
fn test_binary_file_preserves_new_bytes_and_calculates_size() {
    let diff = r#"diff --git a/logo.png b/logo.png
index 0000000..abcdef1 100644
Binary files differ
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.new_bytes = Some(4096);
    f.old_bytes = Some(2048);
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert!(out.is_binary);
    assert_eq!(out.new_bytes, Some(4096));
    assert_eq!(out.old_bytes, Some(2048));
    assert_eq!(out.new_byte_size(), Some(4096));
    assert_eq!(out.old_byte_size(), Some(2048));
    assert_eq!(out.display_size(), "2kB ➞ 4kB");
}

#[test]
fn test_binary_file_falls_back_to_fs_metadata_for_size() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("image.bin");
    std::fs::write(&file_path, vec![0u8; 8192]).unwrap();

    let diff = "diff --git a/image.bin b/image.bin\nBinary files differ\n";
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    let opts = PipelineOptions::default().with_repo(Some(dir.path().to_str().unwrap().to_string()));
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert!(out.is_binary);
    assert_eq!(out.new_bytes, Some(8192));
    assert_eq!(out.new_byte_size(), Some(8192));
    assert_eq!(out.display_size(), "? ➞ 8kB");
}

#[test]
fn test_empty_file_size_determined_as_some_zero() {
    let diff = r#"diff --git a/.gitkeep b/.gitkeep
new file mode 100644
--- /dev/null
+++ b/.gitkeep
@@ -0,0 +0,0 @@
"#;
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, Some(""), None, None, &opts);
    assert_eq!(out.new_bytes, Some(0));
    assert_eq!(out.old_bytes, None);
    assert_eq!(out.new_byte_size(), Some(0));
    assert_eq!(out.old_byte_size(), None);
    assert_eq!(out.display_size(), "× ➞ 0B");
}

#[test]
fn test_deleted_file_has_none_new_size_and_preserves_old_size() {
    let diff = r#"diff --git a/deleted.txt b/deleted.txt
deleted file mode 100644
--- a/deleted.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-hello
"#;
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, Some("hello\n"), None, &opts);
    assert_eq!(out.new_bytes, None);
    assert_eq!(out.old_bytes, Some(6));
    assert_eq!(out.new_byte_size(), None);
    assert_eq!(out.old_byte_size(), Some(6));
    assert_eq!(out.display_size(), "6B ➞ ×");
    assert_eq!(out.display_status(), "6B ➞ ×");
}

#[test]
fn test_empty_file_modified_to_content_shows_zero_to_new_size() {
    let diff = r#"diff --git a/.gitkeep b/.gitkeep
--- a/.gitkeep
+++ b/.gitkeep
@@ -0,0 +1,1 @@
+keep
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.old_bytes = Some(0);
    f.new_bytes = Some(5);
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, Some("keep\n"), Some(""), None, &opts);
    assert_eq!(out.old_bytes, Some(0));
    assert_eq!(out.new_bytes, Some(5));
    assert_eq!(out.display_size(), "0B ➞ 5B");
}

#[test]
fn test_unknown_size_does_not_show_zero_prefix() {
    let diff = r#"diff --git a/binary.dat b/binary.dat
Binary files differ
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.old_bytes = None; // could not be determined
    f.new_bytes = Some(1024);
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert_eq!(out.old_bytes, None);
    assert_eq!(out.new_bytes, Some(1024));
    assert_eq!(out.display_size(), "? ➞ 1kB");
}

#[test]
fn test_modified_unknown_new_size_does_not_show_old_size() {
    let diff = r#"diff --git a/binary.dat b/binary.dat
Binary files differ
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.old_bytes = Some(1024);
    f.new_bytes = None; // could not be determined
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert_eq!(out.old_bytes, Some(1024));
    assert_eq!(out.new_bytes, None);
    assert_eq!(out.display_size(), "1kB ➞ ?");
}

#[test]
fn test_modified_both_sizes_unknown() {
    let diff = r#"diff --git a/binary.dat b/binary.dat
Binary files differ
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.old_bytes = None;
    f.new_bytes = None;
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert_eq!(out.old_bytes, None);
    assert_eq!(out.new_bytes, None);
    assert_eq!(out.display_size(), "");
    assert_eq!(out.display_status(), "binary");
}

#[test]
fn test_file_size_not_guessed_by_summing_lines() {
    let diff = r#"diff --git a/code.rs b/code.rs
--- a/code.rs
+++ b/code.rs
@@ -5,2 +5,2 @@
-old line
+new line
"#;
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    let opts = PipelineOptions::default();
    // Neither old_content nor new_content provided, and no git repo blob size:
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert_eq!(out.old_bytes, None);
    assert_eq!(out.display_size(), "");
}

#[test]
fn test_added_and_deleted_both_sizes_unknown_renders_no_size() {
    let diff_add = r#"diff --git a/new.rs b/new.rs
new file mode 100644
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,1 @@
+hello
"#;
    let f_add = crate::parse::parse_diff(diff_add)
        .into_iter()
        .next()
        .unwrap();
    let opts = PipelineOptions::default();
    let out_add = process_single_diff_file(f_add, None, None, None, &opts);
    assert_eq!(out_add.old_bytes, None);
    assert_eq!(out_add.new_bytes, None);
    assert_eq!(out_add.display_size(), "");
    assert_eq!(out_add.display_status(), "added");

    let diff_del = r#"diff --git a/old.rs b/old.rs
deleted file mode 100644
--- a/old.rs
+++ /dev/null
@@ -1,1 +0,0 @@
-hello
"#;
    let f_del = crate::parse::parse_diff(diff_del)
        .into_iter()
        .next()
        .unwrap();
    let out_del = process_single_diff_file(f_del, None, None, None, &opts);
    assert_eq!(out_del.old_bytes, None);
    assert_eq!(out_del.new_bytes, None);
    assert_eq!(out_del.display_size(), "");
    assert_eq!(out_del.display_status(), "deleted");
}

#[test]
fn test_formatted_size_identical_renders_single_size() {
    let diff = r#"diff --git a/file.rs b/file.rs
--- a/file.rs
+++ b/file.rs
@@ -1,1 +1,1 @@
-old content
+new content!
"#;
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    // 15110 bytes (14.755 kB) and 15115 bytes (14.760 kB) both format to
    // "14.8kB"
    f.old_bytes = Some(15110);
    f.new_bytes = Some(15115);
    let opts = PipelineOptions::default();
    let out = process_single_diff_file(f, None, None, None, &opts);
    assert_eq!(crate::wire::format_byte_size(15110), "14.8kB");
    assert_eq!(crate::wire::format_byte_size(15115), "14.8kB");
    assert_eq!(out.old_bytes, Some(15110));
    assert_eq!(out.new_bytes, Some(15115));
    // Must render single formatted size "14.8kB", never "14.8kB -> 14.8kB"
    assert_eq!(out.display_size(), "14.8kB");
    assert_eq!(out.display_status(), "14.8kB");

    // Exact equal byte sizes also render single formatted size
    let mut f_exact = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f_exact.old_bytes = Some(2048);
    f_exact.new_bytes = Some(2048);
    let out_exact = process_single_diff_file(f_exact, None, None, None, &opts);
    assert_eq!(out_exact.display_size(), "2kB");
    assert_eq!(out_exact.display_status(), "2kB");
}

#[test]
fn test_lazy_collapsed_deleted_file_skips_pipeline_and_renders_all_deletes_on_expansion() {
    let diff = r#"diff --git a/deleted.rs b/deleted.rs
deleted file mode 100644
index 1111111..0000000
--- a/deleted.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-line 1
-line 2
-line 3
"#;
    let old_content = "line 1\nline 2\nline 3\n";
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    assert!(is_initially_collapsed(&f));

    let lazy_opts = PipelineOptions::default().with_lazy_collapsed(true);
    let stub = process_single_diff_file(f, None, Some(old_content), Some(diff), &lazy_opts);

    assert!(!stub.is_loaded(), "lazy stub should not be loaded");
    assert_eq!(stub.counts.dels, 3);
    assert_eq!(stub.counts.adds, 0);
    assert!(stub.hunks.is_empty(), "hunks should skip processing");
    assert!(stub.raw_source.is_some());

    // Render on demand from the stored raw_source
    let source = stub.raw_source.as_ref().unwrap();
    let rendered =
        render_file_from_source(source, None, &[], 4, &lazy_opts).expect("should render on demand");

    assert!(rendered.is_loaded(), "rendered file must be loaded");
    assert_eq!(rendered.counts.dels, 3);
    assert_eq!(rendered.hunks.len(), 1);
    assert_eq!(rendered.hunks[0].rows.len(), 3);
    for row in &rendered.hunks[0].rows {
        assert_eq!(row.kind, RowKind::Del);
    }
}

#[test]
fn test_lazy_collapsed_untracked_file_skips_pipeline_and_renders_on_expansion() {
    let diff = r#"diff --git a/untracked.txt b/untracked.txt
untracked file mode 100644
--- /dev/null
+++ b/untracked.txt
@@ -0,0 +1,2 @@
+new line 1
+new line 2
"#;
    let new_content = "new line 1\nnew line 2\n";
    let mut f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    f.status = crate::parse::FileStatus::Untracked;
    assert!(is_initially_collapsed(&f));

    let lazy_opts = PipelineOptions::default().with_lazy_collapsed(true);
    let stub = process_single_diff_file(f, Some(new_content), None, Some(diff), &lazy_opts);

    assert!(
        !stub.is_loaded(),
        "untracked lazy stub should not be loaded"
    );
    assert_eq!(stub.counts.adds, 2);
    assert_eq!(stub.counts.dels, 0);
    assert!(stub.hunks.is_empty());

    let source = stub.raw_source.as_ref().unwrap();
    let rendered =
        render_file_from_source(source, None, &[], 4, &lazy_opts).expect("should render on demand");

    assert!(rendered.is_loaded());
    assert_eq!(rendered.counts.adds, 2);
    assert_eq!(rendered.hunks.len(), 1);
    assert_eq!(rendered.hunks[0].rows.len(), 2);
}
