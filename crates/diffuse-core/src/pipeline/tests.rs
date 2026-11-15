use std::{collections::HashMap, fs};

use diffuse_syntax::color::TokenColor;

use super::*;
use crate::{
    lcs::word_spans,
    merge::{RunKind, display_text},
    parse::{AddLine, DelLine, DiffFile, Row, RowKind},
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
        tokens[0].color,
        TokenColor::Comment,
        "comment prefix is com"
    );
    assert_eq!(tokens[0].text, "/// ");
    assert_eq!(tokens[1].color, TokenColor::FenceBorder, "fence is fbo");
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
        common_after.unwrap().color,
        TokenColor::Comment,
        "text after fill preserves com color"
    );
}

#[test]
fn test_comment_reflow_del_span_positioning() {
    // Row 1078: `lowers` is deleted before `non-top-upper layers` is added, and
    // trailing `layer's markers —` lands at the end
    let old_1078 = "// Walk the lowers top → bottom, honoring each layer's markers —";
    let new_1078 = "// Walk the non-top-upper layers top → bottom, honoring each";
    let r_1078 = Row::mod_with_spans(
        DelLine::new(1078, old_1078),
        AddLine::new(1078, new_1078),
        word_spans(old_1078, new_1078),
    );
    let runs_1078 = crate::merge::merged_runs(&r_1078);
    let non_common_1078: Vec<(RunKind, &str)> = runs_1078
        .iter()
        .filter(|r| r.kind != RunKind::Ctx)
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
    let r_1079 = Row::mod_with_spans(
        DelLine::new(1079, old_1079),
        AddLine::new(1079, new_1079),
        word_spans(old_1079, new_1079),
    );
    let runs_1079 = crate::merge::merged_runs(&r_1079);
    let non_common_1079: Vec<(RunKind, &str)> = runs_1079
        .iter()
        .filter(|r| r.kind != RunKind::Ctx)
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
    let wire_rev = wire.clone().reverse();
    assert_eq!(wire_rev.clone().reverse(), wire);

    // Reconstruct from reverse wire matches reversed AST files
    let rev_files_from_wire = wire_rev.reconstruct();
    let rev_files_direct: Vec<DiffFile> = reconstructed_files
        .into_iter()
        .map(DiffFile::reverse)
        .collect();
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

    // 1. Assert that there is NO floating del under [env] (between lines 1 and 6)
    for r in &rows[..6] {
        assert_ne!(
            r.kind,
            RowKind::Del,
            "no del rows should float into the top [env] section"
        );
    }

    // 2. Assert that runner in x86_64-unknown-linux-musl is paired as 1-to-1 Mod
    //    (old 10 -> new 14)
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

    // 4. Assert that runner in aarch64-unknown-linux-musl is paired as 1-to-1 Mod
    //    (old 23 -> new 27)
    let aarch64_musl_runner = rows.iter().find(|r| r.new_line == Some(27));
    assert!(aarch64_musl_runner.is_some());
    assert_eq!(aarch64_musl_runner.unwrap().kind, RowKind::Mod);
    assert_eq!(aarch64_musl_runner.unwrap().old_line, Some(23));

    // 5. Assert that runner in aarch64-linux-android is paired as 1-to-1 Mod (old
    //    29 -> new 33)
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
    assert_eq!(ctx_rows[0].text(), "fn foo() {");
    assert_eq!(ctx_rows[1].text(), "}");
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
                assert_eq!(kept, r.text(), "strike-removed runs must be the new line");
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
    assert_eq!(src_row.text(), "    moved_statement();");

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
    assert_eq!(tgt_row.text(), "    moved_statement();");

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
    assert_eq!(f.hunks[0].rows[0].text(), "fn alpha() {");
    assert_eq!(f.hunks[0].rows[0].new_line, Some(1));
    assert_eq!(f.hunks[0].rows[0].old_line, Some(1));
    assert_eq!(f.hunks[0].rows[1].kind, RowKind::Ctx);
    assert_eq!(f.hunks[0].rows[1].text(), "    println!(\"hello\");");
    assert_eq!(f.hunks[0].rows[2].kind, RowKind::Ctx);
    assert_eq!(f.hunks[0].rows[2].text(), "}");
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
        .find(|r| r.text().trim() == "if !run.is_empty() {")
        .expect("line 150 row");
    assert_eq!(
        r150.kind,
        crate::parse::RowKind::Mod,
        "line 150 should pair with the old if statement as Mod"
    );
    assert_eq!(
        r150.old_text().trim(),
        "if !run.is_empty() && !moved_dels.contains(&run[0]) {"
    );
    assert_eq!(
        r150.spans().del,
        vec![(35, 68)],
        "parentheses of !run.is_empty() must remain intact and not split"
    );
    assert_eq!(r150.spans().add, vec![]);

    let r156 = rows
        .iter()
        .find(|r| r.text().trim() == "if run.len() > 1")
        .expect("line 156 row");
    assert_eq!(
        r156.kind,
        crate::parse::RowKind::Add,
        "line 156 should be a clean pure Add"
    );
    assert_eq!(r156.old_text(), "");

    let r166 = rows
        .iter()
        .find(|r| r.text().trim() == "unmark_positions.extend(moved_in_run);")
        .expect("line 166 row");
    assert_eq!(
        r166.kind,
        crate::parse::RowKind::Mod,
        "line 166 should pair with old receiver line 148 as Mod"
    );
    assert_eq!(r166.old_text().trim(), "unmark_positions");

    let r_filter = rows
        .iter()
        .find(|r| r.text().trim() == ".filter(|pos| moved_dels.contains(pos)),")
        .expect("filter row");
    assert_eq!(r_filter.kind, crate::parse::RowKind::Mod);
    assert_eq!(
        r_filter.old_text().trim(),
        ".extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));"
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
    fs::write(&file_path, vec![0u8; 8192]).unwrap();

    let diff = "diff --git a/image.bin b/image.bin\nBinary files differ\n";
    let f = crate::parse::parse_diff(diff).into_iter().next().unwrap();
    let opts =
        PipelineOptions::default().with_repo_key(Some(dir.path().to_str().unwrap().to_string()));
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
    assert_eq!(out.display_size(), "0B");
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
    assert_eq!(out.display_size(), "6B");
    assert_eq!(out.display_status(), "deleted, 6B");
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
    assert_eq!(crate::wire::format_byte_size(15110), "15kB");
    assert_eq!(crate::wire::format_byte_size(15115), "15kB");
    assert_eq!(out.old_bytes, Some(15110));
    assert_eq!(out.new_bytes, Some(15115));
    // Must render single formatted size "15kB", never "15kB -> 15kB"
    assert_eq!(out.display_size(), "15kB");
    assert_eq!(out.display_status(), "15kB");

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
    let stub = process_single_diff_file(
        f,
        None,
        Some(old_content),
        Some(Arc::new(diff.to_string())),
        &lazy_opts,
    );

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
    let stub = process_single_diff_file(
        f,
        Some(new_content),
        None,
        Some(Arc::new(diff.to_string())),
        &lazy_opts,
    );

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

#[test]
fn test_pipeline_notes_counts_with_resolved_repo() {
    use crate::notes::add_note;

    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old one\n+new one\n old two\n";
    let mut db = crate::notes::NotesDB::new();
    for (i, text) in ["note one", "note two"].iter().enumerate() {
        add_note(
            &mut db,
            "myrepo",
            "",
            ReviewNote {
                id: format!("n{i}"),
                file: "a.txt".to_string(),
                location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
                text: text.to_string(),
                created_at: 0,
                content_hash: None,
                ..Default::default()
            },
        );
    }
    // A note on a file absent from the diff must not leak into counts.
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "other".to_string(),
            file: "unrelated.rs".to_string(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "other".to_string(),
            created_at: 0,
            content_hash: None,
            ..Default::default()
        },
    );

    // The streaming TUI passes the *resolved* repo name (dir basename or
    // --repo-key) — with it, per-file counts.notes and the global sum populate.
    let opts = PipelineOptions::default().with_notes(Some(db), Some("myrepo".to_string()), None);
    let out = run_pipeline_full(diff, None, None, opts);
    assert_eq!(out.files.len(), 1);
    assert_eq!(
        out.files[0].counts.notes, 2,
        "file counts.notes must reflect attached notes"
    );
    assert_eq!(out.files[0].notes.len(), 2);
    let total = out.total_counts();
    assert_eq!(
        total.notes, 2,
        "PipelineOutput total_counts must sum per-file notes"
    );

    // Without a repo key the pipeline cannot match notes — the header falls
    // back to zero (this is why the TUI must pass the resolved repo).
    let out_no_repo = run_pipeline_full(diff, None, None, PipelineOptions::default());
    assert_eq!(out_no_repo.files[0].counts.notes, 0);
    assert_eq!(out_no_repo.total_counts().notes, 0);
}

#[test]
fn test_pipeline_note_status_filtering_and_counts() {
    use crate::notes::{NoteStatus, ReviewNote, add_note};

    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old one\n+new one\n old two\n";
    let mut db = crate::notes::NotesDB::new();
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "n_open".to_string(),
            file: "a.txt".to_string(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "open note".to_string(),
            status: NoteStatus::Open,
            ..Default::default()
        },
    );
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "n_in_progress".to_string(),
            file: "a.txt".to_string(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "in progress note".to_string(),
            status: NoteStatus::InProgress,
            ..Default::default()
        },
    );
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "n_resolved".to_string(),
            file: "a.txt".to_string(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "resolved note".to_string(),
            status: NoteStatus::Resolved,
            ..Default::default()
        },
    );

    // Default: show_resolved = false. Only active (Open + InProgress) notes
    // returned.
    let opts =
        PipelineOptions::default().with_notes(Some(db.clone()), Some("myrepo".to_string()), None);
    let out = run_pipeline_full(diff, None, None, opts);
    assert_eq!(out.files.len(), 1);
    let counts = out.files[0].counts;
    assert_eq!(counts.notes, 1, "open notes count");
    assert_eq!(counts.in_progress_notes, 1, "in-progress notes count");
    assert_eq!(counts.resolved_notes, 1, "resolved notes count");
    assert_eq!(
        out.files[0].notes.len(),
        2,
        "only active notes in default output"
    );
    assert_eq!(out.files[0].notes[0].status, NoteStatus::Open);
    assert_eq!(out.files[0].notes[1].status, NoteStatus::InProgress);

    let total = out.total_counts();
    assert_eq!(total.notes, 1);
    assert_eq!(total.in_progress_notes, 1);
    assert_eq!(total.resolved_notes, 1);

    // With show_resolved = true: all notes returned.
    let opts_all = PipelineOptions::default()
        .with_notes(Some(db), Some("myrepo".to_string()), None)
        .with_show_resolved(true);
    let out_all = run_pipeline_full(diff, None, None, opts_all);
    assert_eq!(
        out_all.files[0].notes.len(),
        3,
        "all notes including resolved"
    );
    assert_eq!(out_all.files[0].notes[2].status, NoteStatus::Resolved);
}

#[test]
fn test_pipeline_preserves_all_notes_in_counts() {
    use crate::notes::{LineHash, ReviewNote, add_note};

    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old one\n+new one\n old two\n";
    let mut db = crate::notes::NotesDB::new();

    // 1. Note with a content_hash -> kept
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "clean".into(),
            file: "a.txt".into(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "clean note".into(),
            created_at: 0,
            content_hash: Some(LineHash::from_hex("0000bad1").unwrap()),
            ..Default::default()
        },
    );

    // 2. Note whose hash matches nothing -> kept (displayed on file banner)
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "mismatch".into(),
            file: "a.txt".into(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
            text: "mismatched note".into(),
            created_at: 0,
            content_hash: Some(LineHash::from_hex("deadbeef").unwrap()),
            ..Default::default()
        },
    );

    // 3. Nonexistent line note -> kept (displayed on file banner)
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "orphan".into(),
            file: "a.txt".into(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(9999)),
            text: "orphan note".into(),
            created_at: 0,
            content_hash: Some(LineHash::from_hex("deadbeef").unwrap()),
            ..Default::default()
        },
    );

    // 4. Note without a content_hash on an existing line -> kept (displayed on file
    //    banner)
    add_note(
        &mut db,
        "myrepo",
        "",
        ReviewNote {
            id: "unhashed".into(),
            file: "a.txt".into(),
            location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(2)),
            text: "unhashed note".into(),
            created_at: 0,
            content_hash: None,
            ..Default::default()
        },
    );

    let opts = PipelineOptions::default().with_notes(Some(db), Some("myrepo".into()), None);
    let out = run_pipeline_full(diff, None, None, opts);
    assert_eq!(out.files.len(), 1);
    assert_eq!(
        out.files[0].counts.notes, 4,
        "all notes (clean, mismatch, orphan, unhashed) must be retained in counts"
    );
    assert_eq!(out.files[0].notes.len(), 4);
    let note_ids: Vec<_> = out.files[0].notes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(note_ids, vec!["clean", "mismatch", "orphan", "unhashed"]);
    assert_eq!(out.total_counts().notes, 4);
}

#[test]
fn test_markdown_file_rows_multiline_fences_and_syntax() {
    let text = "Test *Markdown* **formatting:**\n\n`this is a code span` with a comment.\n\n* List\n* Two\n\n## Header\n\n```bash\nif -f hello.txt\n  cat hello.txt\nfi\n```\n\nDone!";
    let rows = markdown_file_rows(text);

    // 16 lines from document lines
    assert_eq!(rows.len(), 16);

    // Line 0: Test *Markdown* **formatting:**
    assert_eq!(rows[0].text(), "Test *Markdown* **formatting:**");
    assert!(
        rows[0]
            .runs
            .iter()
            .any(|r| r.color.is_default() && r.text == "Test ")
    );
    assert!(
        rows[0]
            .runs
            .iter()
            .any(|r| r.color == TokenColor::Italic && r.text == "Markdown")
    );
    assert!(
        rows[0]
            .runs
            .iter()
            .any(|r| r.color == TokenColor::Bold && r.text == "formatting:")
    );

    // Line 1: empty line between paragraphs
    assert!(rows[1].runs.is_empty());

    // Line 2: `this is a code span` with a comment.
    assert!(
        rows[2]
            .runs
            .iter()
            .any(|r| r.color == TokenColor::Code && r.text == "this is a code span")
    );
    assert!(
        rows[2]
            .runs
            .iter()
            .any(|r| r.color.is_default() && r.text == " with a comment.")
    );

    // Line 7: ## Header
    assert!(
        rows[7]
            .runs
            .iter()
            .any(|r| r.color == TokenColor::Delimiter && r.text == "##")
    );

    // Line 9: opening code fence ```bash -> fbo
    assert_eq!(rows[9].runs.len(), 1);
    assert_eq!(rows[9].runs[0].color, TokenColor::FenceBorder);
    assert_eq!(rows[9].runs[0].text, "```bash");

    // Line 10: bash syntax highlighted `if -f hello.txt`
    assert!(
        rows[10]
            .runs
            .iter()
            .any(|r| r.color == TokenColor::Keyword && r.text == "if")
    );

    // Line 13: closing code fence ``` -> fbo
    assert_eq!(rows[13].runs.len(), 1);
    assert_eq!(rows[13].runs[0].color, TokenColor::FenceBorder);
    assert_eq!(rows[13].runs[0].text, "```");

    // Line 15: Done!
    assert_eq!(rows[15].text(), "Done!");
    assert!(
        rows[15]
            .runs
            .iter()
            .any(|r| r.color.is_default() && r.text == "Done!")
    );
}

#[test]
fn test_render_plain_common_context_super_function() {
    let lines = [
        "plain text line",
        "**bold** and `code`",
        "```rust",
        "let x = 42;",
        "```",
    ];
    let md_profile = diffuse_syntax::registry::lang_profile("md");

    let rows = render_plain_common_context(&lines, md_profile);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].runs[0].color, TokenColor::Default);
    assert_eq!(rows[1].runs[1].color, TokenColor::Bold);
    assert_eq!(rows[3].runs[0].color, TokenColor::Keyword); // "let"
}

#[test]
fn test_synthesize_plain_common_context_hunk() {
    let lines = ["hello", "world"];
    let hunk = synthesize_plain_common_context_hunk(&lines);
    assert_eq!(hunk.old_start, 1);
    assert_eq!(hunk.old_count, 2);
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_count, 2);
    assert_eq!(hunk.rows.len(), 2);
    assert!(matches!(hunk.rows[0], crate::parse::Row::Ctx(_)));
    assert!(matches!(hunk.rows[1], crate::parse::Row::Ctx(_)));
}

// Render unification: a row carrying assemble's profiled merged runs must
// render the profiled split form (Del "10" / Add "5" / Common "..=" /
// Del "17"), while a row without cached runs keeps the legacy unprofiled
// recompute (wholesale Del "10..=17").
#[test]
fn test_colored_runs_prefers_cached_profiled_runs() {
    use crate::lcs::{word_spans_and_runs_with_profile, word_spans_with_profile};

    let profile = diffuse_syntax::registry::lang_profile("rs");
    let del = "    assert_eq!(f.hunks[0].rows.len(), 8); // lines 10..=17";
    let add = "    assert_eq!(f.hunks[0].rows.len(), 18); // lines 5..=22 (6 above + mod11 + ctx12..15 + mod16 + 6 below)";
    let spans = word_spans_with_profile(del, add, profile);
    let plain = Row::mod_with_spans(DelLine::new(1, del), AddLine::new(1, add), spans.clone());
    let kinds = |runs: &[OutputRun]| {
        runs.iter()
            .map(|r| (format!("{:?}", r.kind), r.text.to_string()))
            .collect::<Vec<_>>()
    };
    assert!(
        kinds(&colored_runs(&plain, None, &[]))
            .contains(&("Del".to_string(), "10..=17".to_string())),
        "uncached row keeps the legacy wholesale render"
    );

    let (_, runs) = word_spans_and_runs_with_profile(del, add, profile);
    let stored = Row::mod_with_alignment(DelLine::new(1, del), AddLine::new(1, add), spans, runs);
    let rendered = kinds(&colored_runs(&stored, None, &[]));
    for want in [("Del", "10"), ("Add", "5"), ("Ctx", "..="), ("Del", "17")] {
        assert!(
            rendered.contains(&(want.0.to_string(), want.1.to_string())),
            "cached row must render the profiled split form, missing {want:?}"
        );
    }
}

#[test]
fn test_markdown_and_doc_comment_table_pipeline() {
    let diff = "diff --git a/test.md b/test.md\n\
                --- a/test.md\n\
                +++ b/test.md\n\
                @@ -1,3 +1,3 @@\n\
                +| Col A | Col B |\n\
                +| :--- | :---: |\n\
                +| val1 | val2 |\n\
                diff --git a/test.rs b/test.rs\n\
                --- a/test.rs\n\
                +++ b/test.rs\n\
                @@ -1,3 +1,3 @@\n\
                +/// | Header A | Header B |\n\
                +/// | :--- | :---: |\n\
                +/// | Item 1 | Item 2 |\n";

    let mut new_content = HashMap::new();
    new_content.insert(
        "test.md".to_string(),
        "| Col A | Col B |\n| :--- | :---: |\n| val1 | val2 |\n".to_string(),
    );
    new_content.insert(
        "test.rs".to_string(),
        "/// | Header A | Header B |\n/// | :--- | :---: |\n/// | Item 1 | Item 2 |\n".to_string(),
    );

    let output = run_pipeline(diff, Some(new_content));
    assert_eq!(output.files.len(), 2);

    // 1. Verify test.md
    let md_file = &output.files[0];
    let md_rows = &md_file.hunks[0].rows;
    assert_eq!(md_rows.len(), 3);

    // Markdown Header row: pipes table borders, header text table_header with
    // SGR 14 (bold underline)
    let header_runs = &md_rows[0].runs;
    let pipe_runs: Vec<_> = header_runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(pipe_runs.len(), 3);
    for p in pipe_runs {
        assert_eq!(p.color, TokenColor::TableBorder);
    }
    let col_a = header_runs
        .iter()
        .find(|r| r.text.contains("Col A"))
        .unwrap();
    assert_eq!(col_a.color, TokenColor::TableHeader);
    assert_eq!(col_a.sgr, Some(14));

    // Markdown Alignment row: pipes and dashes table borders
    let align_runs = &md_rows[1].runs;
    for r in align_runs {
        assert_eq!(r.color, TokenColor::TableBorder);
    }

    // Markdown Body row: pipes table borders
    let body_runs = &md_rows[2].runs;
    let body_pipes: Vec<_> = body_runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(body_pipes.len(), 3);
    for p in body_pipes {
        assert_eq!(p.color, TokenColor::TableBorder);
    }

    // 2. Verify test.rs (Rust doc comment table)
    let rs_file = &output.files[1];
    let rs_rows = &rs_file.hunks[0].rows;
    assert_eq!(rs_rows.len(), 3);

    // Doc comment Header row: /// is com, pipes table borders,
    // headers table_header with SGR 14
    let rs_header_runs = &rs_rows[0].runs;
    assert_eq!(rs_header_runs[0].color, TokenColor::Comment);
    assert_eq!(rs_header_runs[0].text.as_ref(), "///");
    let rs_pipe_runs: Vec<_> = rs_header_runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(rs_pipe_runs.len(), 3);
    for p in rs_pipe_runs {
        assert_eq!(p.color, TokenColor::TableBorder);
    }
    let hdr_a = rs_header_runs
        .iter()
        .find(|r| r.text.contains("Header A"))
        .unwrap();
    assert_eq!(hdr_a.color, TokenColor::TableHeader);
    assert_eq!(hdr_a.sgr, Some(14));

    // Doc comment Alignment row: /// and space are com, pipes and
    // dashes table borders
    let rs_align_runs = &rs_rows[1].runs;
    assert_eq!(rs_align_runs[0].color, TokenColor::Comment);
    assert_eq!(rs_align_runs[0].text.as_ref(), "///");
    assert_eq!(rs_align_runs[1].color, TokenColor::Comment);
    for r in &rs_align_runs[2..] {
        assert_eq!(r.color, TokenColor::TableBorder);
    }

    // Doc comment Body row: /// is com, pipes table borders
    let rs_body_runs = &rs_rows[2].runs;
    assert_eq!(rs_body_runs[0].color, TokenColor::Comment);
    let rs_body_pipes: Vec<_> = rs_body_runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(rs_body_pipes.len(), 3);
    for p in rs_body_pipes {
        assert_eq!(p.color, TokenColor::TableBorder);
    }
}

#[test]
fn test_mod_row_in_table_structure_with_adds_and_dels() {
    let diff = "diff --git a/table.md b/table.md\n\
                --- a/table.md\n\
                +++ b/table.md\n\
                @@ -1,3 +1,3 @@\n\
                -| Feature | Old Description |\n\
                +| Feature | New Description |\n\
                -| | |\n\
                +| :--- | :---: |\n\
                -| `--theme <NAME>` | Color theme (`magic` default, `default`, etc.) |\n\
                +| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |\n";

    let mut new_content = HashMap::new();
    new_content.insert(
        "table.md".to_string(),
        "| Feature | New Description |\n| :--- | :---: |\n| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |\n".to_string(),
    );
    let mut old_content = HashMap::new();
    old_content.insert(
        "table.md".to_string(),
        "| Feature | Old Description |\n| | |\n| `--theme <NAME>` | Color theme (`magic` default, `default`, etc.) |\n".to_string(),
    );

    let opts = PipelineOptions::default().with_lazy_collapsed(false);
    let output = run_pipeline_full(diff, Some(new_content), Some(old_content), opts);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    assert_eq!(rows.len(), 4);

    // Row 0: Modified Header Row
    // | Feature | Old Description | -> | Feature | New Description |
    let row0 = &rows[0];
    assert_eq!(row0.kind, RowKind::Mod);
    // Unchanged pipes are Common and table borders
    let row0_pipes: Vec<_> = row0
        .runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(row0_pipes.len(), 3);
    for p in row0_pipes {
        assert_eq!(p.kind, RunKind::Ctx);
        assert_eq!(p.color, TokenColor::TableBorder);
    }
    // "Feature" is Ctx and table_header with SGR 14 (header)
    let feat = row0
        .runs
        .iter()
        .find(|r| r.text.contains("Feature"))
        .unwrap();
    assert_eq!(feat.kind, RunKind::Ctx);
    assert_eq!(feat.color, TokenColor::TableHeader);
    assert_eq!(feat.sgr, Some(14));

    // "Old" is Del
    let old_run = row0.runs.iter().find(|r| r.text.contains("Old")).unwrap();
    assert_eq!(old_run.kind, RunKind::Del);

    // "New" is Add, table_header with SGR 14 (header)
    let new_run = row0.runs.iter().find(|r| r.text.contains("New")).unwrap();
    assert_eq!(new_run.kind, RunKind::Add);
    assert_eq!(new_run.color, TokenColor::TableHeader);
    assert_eq!(new_run.sgr, Some(14));

    // "Description" is Ctx and table_header with SGR 14 (header)
    let desc = row0
        .runs
        .iter()
        .find(|r| r.text.contains("Description"))
        .unwrap();
    assert_eq!(desc.kind, RunKind::Ctx);
    assert_eq!(desc.color, TokenColor::TableHeader);
    assert_eq!(desc.sgr, Some(14));

    // Row 3: Modified Body Row with multiple adds and dels:
    // | `--theme <NAME>` | Color theme (`magic` default, `default`, etc.) |
    // -> | `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |
    let row3 = &rows[3];
    assert_eq!(row3.kind, RowKind::Mod);

    // Unchanged pipes are Ctx and table borders
    let row3_pipes: Vec<_> = row3
        .runs
        .iter()
        .filter(|r| r.text.as_ref() == "|")
        .collect();
    assert_eq!(row3_pipes.len(), 3);
    for p in row3_pipes {
        assert_eq!(p.kind, RunKind::Ctx);
        assert_eq!(p.color, TokenColor::TableBorder);
    }

    // Code span `--theme <NAME>` is Ctx and styled code
    let theme_code = row3
        .runs
        .iter()
        .find(|r| r.text.contains("--theme <NAME>"))
        .unwrap();
    assert_eq!(theme_code.kind, RunKind::Ctx);
    assert_eq!(theme_code.color, TokenColor::Code);

    // Del runs: `magic` default, and etc.
    let magic_del = row3.runs.iter().find(|r| r.text.contains("magic")).unwrap();
    assert_eq!(magic_del.kind, RunKind::Del);

    // Add runs: `dark`, `black`
    let dark_add = row3.runs.iter().find(|r| r.text.contains("dark")).unwrap();
    assert_eq!(dark_add.kind, RunKind::Add);
    assert_eq!(dark_add.color, TokenColor::Code);
}

#[test]
fn test_unrecognized_fence_language_normalizes_to_empty() {
    let diff = "\
diff --git a/test.md b/test.md
new file mode 100644
--- /dev/null
+++ b/test.md
@@ -0,0 +1,5 @@
+```mermaid
+flowchart TD
+  A --> B
+```
";
    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let file = &output.files[0];
    assert_eq!(file.fences.len(), 1);
    assert_eq!(file.fences[0].lang, "");
}

/// Build a gap-note fixture: context rows for the span `start..=end` with
/// per-line content from `line_text`, plus the matching note whose location,
/// stored rows, and content hash derive from those rows.
fn gap_note(
    id: &str,
    text: &str,
    start: u32,
    end: u32,
    new_side: bool,
    line_text: impl Fn(u32) -> String,
) -> crate::notes::ReviewNote {
    let rows: Vec<crate::wire::OutputRow> = (start..=end)
        .map(|n| crate::wire::OutputRow {
            kind: RowKind::Ctx,
            mov_side: None,
            // New-side gap rows carry both coordinates (old == new in
            // unchanged regions); old-side gap rows carry only the old one.
            old_line: Some(n),
            new_line: new_side.then_some(n),
            runs: vec![crate::wire::OutputRun {
                kind: crate::merge::RunKind::Ctx,
                color: TokenColor::Default,
                sgr: None,
                text: line_text(n).into(),
            }],
            wholesale: false,
            group: None,
        })
        .collect();
    let first = crate::notes::DiffCoord::from_row(&rows[0]).unwrap();
    let last = crate::notes::DiffCoord::from_row(rows.last().unwrap()).unwrap();
    crate::notes::ReviewNote {
        id: id.into(),
        file: "test.rs".into(),
        location: if start == end {
            crate::notes::NoteLocation::Line { coord: first }
        } else {
            crate::notes::NoteLocation::Range {
                start: first,
                end: last,
            }
        },
        text: text.into(),
        created_at: 1000,
        original_source_rows: rows.clone(),
        content_hash: crate::notes::LineHash::of_rows(&rows),
        ..Default::default()
    }
}

#[test]
fn test_isolated_note_hunk_synthesis() {
    use crate::notes::add_note;

    let mut lines = Vec::new();
    for i in 1..=100 {
        lines.push(format!("line {i}"));
    }
    let full_content = lines.join("\n");

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,3 +10,3 @@
 line 10
-line 11
+line 11 modified
 line 12
";
    let mut new_content = HashMap::new();
    new_content.insert("test.rs".to_string(), full_content);

    let mut db = HashMap::new();
    add_note(
        &mut db,
        "repo",
        "",
        gap_note("note-isolated", "isolated review note", 80, 80, true, |n| {
            format!("line {n}")
        }),
    );

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output = PipelineInput::new(diff)
        .with_new_content(new_content)
        .with_options(opts)
        .run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.counts.notes, 1);
    assert_eq!(
        f.hunks.len(),
        2,
        "Expected 2 hunks: 1 diff hunk and 1 synthetic note hunk"
    );
    let h2 = &f.hunks[1];
    assert_eq!(h2.new_start, 76);
    assert_eq!(h2.new_count, 9); // lines 76..=84 is 9 lines
    assert!(h2.rows.iter().all(|r| r.kind == RowKind::Ctx));
    assert!(f.rows.iter().any(|r| r.new_line == Some(80)));
}

#[test]
fn test_boundary_adjacent_note_merging() {
    use crate::notes::add_note;

    let mut lines = Vec::new();
    for i in 1..=100 {
        lines.push(format!("line {i}"));
    }
    let full_content = lines.join("\n");

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,3 +10,3 @@
 line 10
-line 11
+line 11 modified
 line 12
";
    let mut new_content = HashMap::new();
    new_content.insert("test.rs".to_string(), full_content);

    let mut db = HashMap::new();
    // Diff hunk with visual_context=4 ends around line 12 + 4 = 16.
    // Note at line 20 is distance 4 from hunk end (<= 2*N = 8), so it merges.
    add_note(
        &mut db,
        "repo",
        "",
        gap_note(
            "note-adjacent",
            "boundary adjacent note",
            20,
            20,
            true,
            |n| format!("line {n}"),
        ),
    );

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output = PipelineInput::new(diff)
        .with_new_content(new_content)
        .with_options(opts)
        .run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.counts.notes, 1);
    assert_eq!(
        f.hunks.len(),
        1,
        "Expected single merged hunk for adjacent note"
    );
    let h = &f.hunks[0];
    // Trailing context should extend to at least note.end + 4 = 24
    let h_end = h.new_start + h.new_count - 1;
    assert!(
        h_end >= 24,
        "Hunk end should be at least line 24, got {h_end}"
    );
    assert!(f.rows.iter().any(|r| r.new_line == Some(20)));
}

#[test]
fn test_multi_note_clustering_in_gap() {
    use crate::notes::add_note;

    let mut lines = Vec::new();
    for i in 1..=100 {
        lines.push(format!("line {i}"));
    }
    let full_content = lines.join("\n");

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,3 +10,3 @@
 line 10
-line 11
+line 11 modified
 line 12
";
    let mut new_content = HashMap::new();
    new_content.insert("test.rs".to_string(), full_content);

    let mut db = HashMap::new();
    // Note 1 at line 60, Note 2 at line 65. Distance is 5 <= 2*N = 8, so cluster
    // together.
    add_note(
        &mut db,
        "repo",
        "",
        gap_note("note-cluster-1", "cluster 1", 60, 60, true, |n| {
            format!("line {n}")
        }),
    );
    add_note(
        &mut db,
        "repo",
        "",
        gap_note("note-cluster-2", "cluster 2", 65, 65, true, |n| {
            format!("line {n}")
        }),
    );

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output = PipelineInput::new(diff)
        .with_new_content(new_content)
        .with_options(opts)
        .run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.counts.notes, 2);
    assert_eq!(
        f.hunks.len(),
        2,
        "Expected 2 hunks: 1 diff hunk and 1 clustered note hunk"
    );
    let h2 = &f.hunks[1];
    // Span is [60-4 .. 65+4] = [56 .. 69]
    assert_eq!(h2.new_start, 56);
    assert_eq!(h2.new_count, 14); // lines 56..=69 is 14 lines
    assert!(f.rows.iter().any(|r| r.new_line == Some(60)));
    assert!(f.rows.iter().any(|r| r.new_line == Some(65)));
}

#[test]
fn test_missing_snapshot_content_fallback() {
    use crate::notes::add_note;

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,3 +10,3 @@
 line 10
-line 11
+line 11 modified
 line 12
";
    let mut db = HashMap::new();
    let note = ReviewNote {
        id: "note-no-snap".into(),
        file: "test.rs".into(),
        location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(80)),
        text: "note without snapshot".into(),
        created_at: 1000,
        // No stored rows and no content hash: the note cannot anchor, so no
        // context hunk may be synthesized for it.
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut db, "repo", "", note);

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    // No snapshot provided
    let output = PipelineInput::new(diff).with_options(opts).run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.counts.notes, 1);
    // Without snapshot, cannot synthesize context hunk in gap
    assert_eq!(f.hunks.len(), 1);
}

#[test]
fn test_note_on_old_side_gap_line_mapping() {
    use crate::notes::add_note;

    // Old file: 100 lines
    let mut old_lines = Vec::new();
    for i in 1..=100 {
        old_lines.push(format!("old line {i}"));
    }
    let old_content = old_lines.join("\n");

    // New file: lines 1..9 unchanged, line 10 replaced with 4 lines (+3 net lines),
    // lines 11..100 shifted by +3
    let mut new_lines = Vec::new();
    for i in 1..=9 {
        new_lines.push(format!("old line {i}"));
    }
    new_lines.push("inserted a".to_string());
    new_lines.push("inserted b".to_string());
    new_lines.push("inserted c".to_string());
    new_lines.push("inserted d".to_string());
    for i in 11..=100 {
        new_lines.push(format!("old line {i}"));
    }
    let new_content = new_lines.join("\n");

    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,1 +10,4 @@
-old line 10
+inserted a
+inserted b
+inserted c
+inserted d
";
    let mut new_map = HashMap::new();
    new_map.insert("test.rs".to_string(), new_content);
    let mut old_map = HashMap::new();
    old_map.insert("test.rs".to_string(), old_content);

    let mut db = HashMap::new();
    // Note on Old side at line 50. Delta is +3, so new line is 53.
    add_note(
        &mut db,
        "repo",
        "",
        gap_note("note-old-side", "old side note", 50, 50, false, |n| {
            format!("old line {n}")
        }),
    );

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output = PipelineInput::new(diff)
        .with_new_content(new_map)
        .with_old_content(old_map)
        .with_options(opts)
        .run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(f.counts.notes, 1);
    assert_eq!(
        f.hunks.len(),
        2,
        "Expected 2 hunks: 1 diff hunk and 1 synthetic note hunk"
    );
    let h2 = &f.hunks[1];
    // New span is 53 - 4 = 49 to 53 + 4 = 57 (9 lines)
    assert_eq!(h2.new_start, 49);
    assert_eq!(h2.new_count, 9);
    // Old span is 50 - 4 = 46 to 50 + 4 = 54 (9 lines)
    assert_eq!(h2.old_start, 46);
    assert_eq!(h2.old_count, 9);

    // Context row for note must carry both new_line (53) and old_line (50)
    let matching_row = f.rows.iter().find(|r| r.old_line == Some(50));
    assert!(
        matching_row.is_some(),
        "Context row for old line 50 must exist"
    );
    assert_eq!(matching_row.unwrap().new_line, Some(53));
}

#[test]
fn test_synthetic_note_hunk_anchoring() {
    let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,3 +1,3 @@\n ctx1\n-old\n+new\n ctx2\n";
    let new_content: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    let old_content: String = (1..=100).map(|i| format!("line {i}\n")).collect();

    let mut new_map = HashMap::new();
    new_map.insert("test.rs".to_string(), new_content);
    let mut old_map = HashMap::new();
    old_map.insert("test.rs".to_string(), old_content);

    // Note 1: multi-line note (lines 50..=52) that matches cleanly and is unique.
    let mut db = HashMap::new();
    crate::notes::add_note(
        &mut db,
        "repo",
        "",
        gap_note("note-unique", "clean match", 50, 52, true, |n| {
            format!("line {n}")
        }),
    );

    let opts = PipelineOptions::default()
        .with_notes(Some(db), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output = PipelineInput::new(diff)
        .with_new_content(new_map.clone())
        .with_old_content(old_map.clone())
        .with_options(opts)
        .run();

    assert_eq!(output.files.len(), 1);
    let f = &output.files[0];
    assert_eq!(
        f.hunks.len(),
        2,
        "Hunk must be synthesized for cleanly anchored note"
    );

    // Note 2: stored rows do NOT match the file content -> gate rejects the
    // note and no hunk is synthesized!
    let mut db2 = HashMap::new();
    crate::notes::add_note(
        &mut db2,
        "repo",
        "",
        gap_note("note-mismatched", "mismatch", 50, 52, true, |n| {
            format!("changed {n}")
        }),
    );
    let opts2 = PipelineOptions::default()
        .with_notes(Some(db2), Some("repo".to_string()), None)
        .with_visual_context(4);

    let output2 = PipelineInput::new(diff)
        .with_new_content(new_map)
        .with_old_content(old_map)
        .with_options(opts2)
        .run();

    assert_eq!(
        output2.files[0].hunks.len(),
        1,
        "Mismatched note must not synthesize a hunk"
    );
}

#[test]
fn test_collapse_block_wholesale_rejection_preserves_absorbed_dels() {
    let diff = "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,3 +1,1 @@\n-let mut selected_lines = Vec::new();\n-for fr in &self.flat {\n-    if fr.file_idx != sel.file_idx {\n+let selected_rows: Vec<_> = self\n";
    let old_content = "let mut selected_lines = Vec::new();\nfor fr in &self.flat {\n    if fr.file_idx != sel.file_idx {\n";
    let new_content = "let selected_rows: Vec<_> = self\n";

    let mut old_map = HashMap::new();
    old_map.insert("test.rs".to_string(), old_content.to_string());
    let mut new_map = HashMap::new();
    new_map.insert("test.rs".to_string(), new_content.to_string());

    let out = PipelineInput::new(diff)
        .with_new_content(new_map)
        .with_old_content(old_map)
        .run();

    let f = &out.files[0];
    let old_lines: Vec<Option<u32>> = f.hunks[0].rows.iter().map(|r| r.old_line).collect();
    assert!(old_lines.contains(&Some(1)), "old line 1 must render");
    assert!(
        old_lines.contains(&Some(2)),
        "old line 2 must render even when wholesale collapse block is rejected"
    );
    assert!(old_lines.contains(&Some(3)), "old line 3 must render");
}

/// Expand anchors must mark reflow-moved source text as moved-out instead
/// of struck: the old tail survives verbatim in the aux rows, so striking
/// it claims a deletion that never happened (struck + context implies the
/// text occurs twice in old). Only genuinely deleted text stays struck.
#[test]
fn test_expand_anchor_marks_moved_source() {
    let diff = r#"diff --git a/crates/diffuse/src/main_tests.rs b/crates/diffuse/src/main_tests.rs
--- a/crates/diffuse/src/main_tests.rs
+++ b/crates/diffuse/src/main_tests.rs
@@ -131,7 +131,10 @@ fn test_resolve_theme_prefers_cli_flag() {
     let _guard = ENV_MUTEX.lock().unwrap();
     unsafe { env::remove_var("DIFFUSE_THEME") };
     assert_eq!(super::resolve_theme(Some("dark")), DiffuseTheme::dark());
-    assert_eq!(super::resolve_theme(Some("light")), DiffuseTheme::light());
+    assert_eq!(
+        super::resolve_theme(Some("light"), false),
+        DiffuseTheme::light()
+    );
     let auto_theme = super::resolve_theme(Some("auto"));
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;

    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.group == Some(crate::parse::RowGroup::Anchor)
                && r.old_line == Some(134)
        })
        .expect("expand anchor mod for old line 134");
    let kinds: Vec<crate::merge::RunKind> = anchor.runs.iter().map(|r| r.kind).collect();
    assert!(
        kinds.contains(&crate::merge::RunKind::MovedSrc),
        "anchor must carry moved-out runs, got: {:?}",
        anchor.runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    // Moved text is never struck ...
    for r in &anchor.runs {
        if r.kind == crate::merge::RunKind::Del {
            assert!(
                !r.text.contains("super") && !r.text.contains("DiffuseTheme"),
                "moved text must not be struck: {:?}",
                r.text
            );
        }
    }
    // ... and moved runs cover the reflowed tail.
    let moved_text: String = anchor
        .runs
        .iter()
        .filter(|r| r.kind == crate::merge::RunKind::MovedSrc)
        .map(|r| r.text.as_ref())
        .collect();
    assert!(
        moved_text.contains("super::resolve_theme"),
        "moved runs must cover the reflowed tail, got: {moved_text:?}"
    );
}

/// Expand aux rows mark moved-in target text: context that survived from
/// the anchor donor renders as moved-target, keeping the wire's account of
/// where text came from (the anchor's moved-out mark is the other half).
#[test]
fn test_expand_aux_marks_moved_target() {
    let diff = r#"diff --git a/crates/diffuse/src/main_tests.rs b/crates/diffuse/src/main_tests.rs
--- a/crates/diffuse/src/main_tests.rs
+++ b/crates/diffuse/src/main_tests.rs
@@ -131,7 +131,10 @@ fn test_resolve_theme_prefers_cli_flag() {
     let _guard = ENV_MUTEX.lock().unwrap();
     unsafe { env::remove_var("DIFFUSE_THEME") };
     assert_eq!(super::resolve_theme(Some("dark")), DiffuseTheme::dark());
-    assert_eq!(super::resolve_theme(Some("light")), DiffuseTheme::light());
+    assert_eq!(
+        super::resolve_theme(Some("light"), false),
+        DiffuseTheme::light()
+    );
     let auto_theme = super::resolve_theme(Some("auto"));
"#;

    let output = run_pipeline_diff(diff);
    let rows = &output.files[0].hunks[0].rows;
    let aux = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Add
                && r.group == Some(crate::parse::RowGroup::Aux { anchor: 134 })
                && r.new_line == Some(135)
        })
        .expect("expand aux add for new line 135");
    // Every run is either moved-in target, novel fill, or struck residue:
    // no plain context (everything shared came from the donor).
    for r in &aux.runs {
        assert!(
            matches!(
                r.kind,
                crate::merge::RunKind::MovedTgt
                    | crate::merge::RunKind::Add
                    | crate::merge::RunKind::Del
            ),
            "aux runs must be target, fill, or struck residue, got: {:?}:{}",
            r.kind,
            r.text
        );
    }
    let moved_text: String = aux
        .runs
        .iter()
        .filter(|r| r.kind == crate::merge::RunKind::MovedTgt)
        .map(|r| r.text.as_ref())
        .collect();
    assert!(
        moved_text.contains("super::resolve_theme"),
        "aux moved-in text must cover the reflowed tail, got: {moved_text:?}"
    );
    // Genuinely novel fills keep their kind.
    assert!(
        aux.runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::Add && r.text.contains("false")),
        "novel fills must stay Add, got: {:?}",
        aux.runs
            .iter()
            .map(|r| format!("{:?}:{}", r.kind, r.text))
            .collect::<Vec<_>>()
    );
}

/// Reflow anchors pair the statement head with the block's head line: old
/// `use std::...` anchors at new `use std::{` (same head, same indent,
/// same line number), with the continuation lines as aux adds — not at the
/// highest-content-overlap continuation line deeper in the block.
#[test]
fn test_reflow_anchor_prefers_head_line() {
    let diff = r#"diff --git a/crates/diffuse-render/src/ansi.rs b/crates/diffuse-render/src/ansi.rs
--- a/crates/diffuse-render/src/ansi.rs
+++ b/crates/diffuse-render/src/ansi.rs
@@ -4,7 +4,10 @@
 //! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.
 
-use std::fmt::{self, Write};
+use std::{
+    borrow::Cow,
+    fmt::{self, Write},
+};
 
 pub use yansi::{Color, Condition, Paint, Style};
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod && r.group == Some(crate::parse::RowGroup::Anchor)
        })
        .expect("reflow anchor mod");
    assert_eq!(
        (anchor.old_line, anchor.new_line),
        (Some(6), Some(6)),
        "anchor must pair the statement head lines, got old={:?} new={:?}",
        anchor.old_line,
        anchor.new_line
    );
    for new_line in [7, 8, 9] {
        assert!(
            rows.iter().any(|r| r.new_line == Some(new_line)
                && r.group == Some(crate::parse::RowGroup::Aux { anchor: 6 })),
            "new line {new_line} must be an aux add of the anchor"
        );
    }
}

/// Upward reflow absorption: when an old single-line statement reflows into
/// multiple new lines where the anchor pairs with the continuation line due to
/// indentation shift or content mass, the preceding statement head addition is
/// absorbed upwards into the group based on information-content evidence
/// explaining the struck prefix.
#[test]
fn test_reflow_upward_expansion_anchors_and_groups() {
    let diff = r#"diff --git a/crates/diffuse-tui/src/app/dispatch.rs b/crates/diffuse-tui/src/app/dispatch.rs
--- a/crates/diffuse-tui/src/app/dispatch.rs
+++ b/crates/diffuse-tui/src/app/dispatch.rs
@@ -350,5 +350,6 @@
         if in_viewport {
+            self.vis_line =
+                (target_vis - s).min(nav_counts[i].saturating_sub(1));
-            self.vis_line = (target_vis - s).min(nav_counts[i].saturating_sub(1));
         }
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;

    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.group == Some(crate::parse::RowGroup::Anchor)
                && r.old_line == Some(351)
        })
        .expect("anchor mod for old line 351");
    assert_eq!(anchor.new_line, Some(352));

    let aux = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Add
                && r.group == Some(crate::parse::RowGroup::Aux { anchor: 352 })
                && r.new_line == Some(351)
        })
        .expect("upward aux add for new line 351");

    assert!(
        aux.runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::MovedTgt && r.text.contains("vis_line")),
        "aux add must mark moved target text, got runs: {:?}",
        aux.runs
    );

    // Context indentation migrated to upward aux add row: leading 12 spaces of
    // indentation are marked as moved context rather than fill.
    assert_eq!(
        aux.runs.first().map(|r| (r.kind, r.text.as_ref())),
        Some((crate::merge::RunKind::MovedTgt, "            ")),
        "aux add leading indentation must migrate from anchor context: {:?}",
        aux.runs
    );

    assert!(
        anchor
            .runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::MovedSrc && r.text.contains("vis_line")),
        "anchor mod must mark moved source text instead of striking it, got runs: {:?}",
        anchor.runs
    );
}

/// Upward reflow collapse: when multiple old lines collapse into a single new
/// line where the anchor pairs with the continuation line due to indentation
/// shift or content mass, the preceding statement head deletion is absorbed
/// into the collapse group based on information-content evidence explaining the
/// addition deficit.
#[test]
fn test_reflow_collapse_anchors_and_groups() {
    let diff = r#"diff --git a/crates/diffuse-tui/src/app/dispatch.rs b/crates/diffuse-tui/src/app/dispatch.rs
--- a/crates/diffuse-tui/src/app/dispatch.rs
+++ b/crates/diffuse-tui/src/app/dispatch.rs
@@ -350,6 +350,5 @@
         if in_viewport {
-            self.vis_line =
-                (target_vis - s).min(nav_counts[i].saturating_sub(1));
+            self.vis_line = (target_vis - s).min(nav_counts[i].saturating_sub(1));
         }
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;

    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.group == Some(crate::parse::RowGroup::Anchor)
                && r.new_line == Some(351)
        })
        .expect("anchor mod for new line 351");
    assert_eq!(anchor.old_line, Some(351));

    let aux = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Del
                && r.group == Some(crate::parse::RowGroup::Aux { anchor: 351 })
                && r.old_line == Some(352)
        })
        .expect("collapse aux del for old line 352");

    assert!(
        aux.runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::MovedSrc && r.text.contains("min")),
        "aux del must mark moved source text, got runs: {:?}",
        aux.runs
    );

    assert!(
        anchor
            .runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::MovedTgt && r.text.contains("min")),
        "anchor mod must mark moved target text, got runs: {:?}",
        anchor.runs
    );
}

/// Upward reflow collapse: when multiple old lines collapse into a single new
/// line where the anchor pairs with a lower continuation line (due to identical
/// indentation and content mass), the preceding unattached statement-head
/// deletion is absorbed upward into the collapse group based on prefix
/// evidence.
#[test]
fn test_reflow_upward_collapse_anchors_and_groups() {
    let diff = r#"diff --git a/src/example.rs b/src/example.rs
--- a/src/example.rs
+++ b/src/example.rs
@@ -10,2 +10,1 @@
-        prefix_token_setup();
-    continuation_line_with_heavy_payload(arg1, arg2);
+    prefix_token_setup(); continuation_line_with_heavy_payload(arg1, arg2);
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;

    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod && r.group == Some(crate::parse::RowGroup::Anchor)
        })
        .expect("anchor mod row");
    assert_eq!(anchor.new_line, Some(10));
    assert_eq!(anchor.old_line, Some(10));

    let aux = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Del
                && r.group == Some(crate::parse::RowGroup::Aux { anchor: 10 })
        })
        .expect("collapse aux del for old line 11");
    assert_eq!(aux.old_line, Some(11));
}

/// Verbatim seeding forces the true move over a content-similar rival:
/// old `let prefix` reflows with its tail moving several lines down while
/// a novel `{repo_name}` line shares the head nearby. The seeded verbatim
/// pair must win; the rival stays a pure addition.
#[test]
fn test_verbatim_seed_forces_true_move_over_rival() {
    let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,6 @@\n ctx\n-        let prefix = format!(\"{r}@\");\n+            let prefix = format!(\"{repo_name}@\");\n+            let prefix = format!(\"{r}@\");\n ctx2\n";

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let verbatim = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.old_line == Some(2)
                && r.text().contains("{r}@")
        })
        .expect("verbatim reindent must pair as Mod");
    assert_eq!(
        verbatim.new_line,
        Some(3),
        "verbatim pair must target the identical line, got new={:?}",
        verbatim.new_line
    );
    let rival = rows
        .iter()
        .find(|r| r.new_line == Some(2))
        .expect("rival row");
    assert_eq!(
        rival.kind,
        crate::parse::RowKind::Add,
        "rival novel line must stay a pure addition, got {:?}",
        rival.kind
    );
}

/// Aux moved-in marking is multiplicity-aware: the old head line carries
/// two brackets (`{…}`) while the reflowed group carries four. Exactly two
/// of the four may render as context (survivors); the other two must render
/// as fill. All-context claims the same occurrence twice.
#[test]
fn test_reflow_aux_brackets_obey_multiplicity() {
    let diff = r#"diff --git a/crates/diffuse-render/src/ansi.rs b/crates/diffuse-render/src/ansi.rs
--- a/crates/diffuse-render/src/ansi.rs
+++ b/crates/diffuse-render/src/ansi.rs
@@ -4,7 +4,10 @@
 //! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.

-use std::fmt::{self, Write};
+use std::{
+    borrow::Cow,
+    fmt::{self, Write},
+};

 pub use yansi::{Color, Condition, Paint, Style};
"#;

    let output = run_pipeline_diff(diff);
    let rows = &output.files[0].hunks[0].rows;
    let group: Vec<_> = rows
        .iter()
        .filter(|r| {
            (r.kind == crate::parse::RowKind::Mod
                && r.group == Some(crate::parse::RowGroup::Anchor))
                || r.group == Some(crate::parse::RowGroup::Aux { anchor: 6 })
        })
        .collect();
    assert_eq!(group.len(), 4, "anchor + three aux rows, got {group:?}");
    let mut ctx = 0usize;
    let mut fill = 0usize;
    for row in &group {
        for run in &row.runs {
            for ch in run.text.chars() {
                if ch == '{' || ch == '}' {
                    match run.kind {
                        crate::merge::RunKind::Add => fill += 1,
                        // MovedTgt renders exactly as context.
                        _ if run.kind == crate::merge::RunKind::Ctx
                            || run.kind == crate::merge::RunKind::MovedTgt =>
                        {
                            ctx += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    assert_eq!(
        (ctx, fill),
        (2, 2),
        "two brackets survive as context, two render as fill (old carries 2, new carries 4)"
    );
}

/// Moved-in allocation is whole-token: aux-`"    let x = 1;"` must not
/// consume the donor's `let` letter-by-letter out of an unrelated
/// identifier, starving the verbatim aux row. The surviving row keeps its
/// tokens; the rival's shared tokens render as fill.
#[test]
fn test_reflow_aux_claims_are_whole_tokens() {
    let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,2 +1,5 @@\n ctx\n-    let last_line_hash = hash_line_content(\"    let y = 2;\");\n+    let compound_hash = LineHash::from_lines([\n+        \"    let x = 1;\",\n+        \"    let y = 2;\",\n+    ]);\n ctx2\n";

    let output = run_pipeline_diff(diff);
    let rows = &output.files[0].hunks[0].rows;
    let y_row = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Add && r.new_text().contains("\"    let y = 2;\"")
        })
        .expect("verbatim y string aux row");
    let moved: String = y_row
        .runs
        .iter()
        .filter(|r| r.kind == crate::merge::RunKind::MovedTgt)
        .map(|r| r.text.as_ref() as &str)
        .collect();
    assert!(
        moved.contains('y'),
        "verbatim row keeps its distinctive tokens moved-in, got moved {moved:?}"
    );
    assert!(
        !moved.contains("let"),
        "rival-row consumption must not scatter `let` letters; `let` renders as fill, got moved {moved:?}"
    );
}

/// Strict ordered replay: every Mod row's runs must apply left-to-right
/// over (del_text, new text) — Ctx checks both cursors with equality,
/// Del/MovedSrc consume old, Add/MovedTgt consume new — ending with both
/// sides fully consumed. This locks plumbing integrity (spans → runs →
/// wire): any corruption, cursor drift, or dropped span fails here.
#[test]
fn test_mod_rows_replay_cleanly() {
    let diff = r#"diff --git a/crates/diffuse-render/src/ansi.rs b/crates/diffuse-render/src/ansi.rs
--- a/crates/diffuse-render/src/ansi.rs
+++ b/crates/diffuse-render/src/ansi.rs
@@ -4,7 +4,10 @@
 //! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.

-use std::fmt::{self, Write};
+use std::{
+    borrow::Cow,
+    fmt::{self, Write},
+};

 pub use yansi::{Color, Condition, Paint, Style};
"#;

    let output = run_pipeline_diff(diff);
    let rows = &output.files[0].hunks[0].rows;
    let mut mods = 0;
    for r in rows.iter().filter(|r| r.kind == crate::parse::RowKind::Mod) {
        mods += 1;
        let old: Vec<char> = r.old_text().chars().collect();
        let new: Vec<char> = r.new_text().chars().collect();
        let (mut o, mut n) = (0usize, 0usize);
        for run in &r.runs {
            let t: Vec<char> = run.text.chars().collect();
            match run.kind {
                crate::merge::RunKind::Ctx => {
                    assert_eq!(
                        &old[o..o + t.len()],
                        &t[..],
                        "ctx run must match old at cursor (old={:?})",
                        r.old_text()
                    );
                    assert_eq!(
                        &new[n..n + t.len()],
                        &t[..],
                        "ctx run must match new at cursor (new={:?})",
                        r.new_text()
                    );
                    o += t.len();
                    n += t.len();
                }
                crate::merge::RunKind::Del | crate::merge::RunKind::MovedSrc => {
                    assert_eq!(&old[o..o + t.len()], &t[..], "del run must match old");
                    o += t.len();
                }
                crate::merge::RunKind::Add | crate::merge::RunKind::MovedTgt => {
                    assert_eq!(&new[n..n + t.len()], &t[..], "add run must match new");
                    n += t.len();
                }
            }
        }
        assert_eq!(o, old.len(), "old side fully consumed");
        assert_eq!(n, new.len(), "new side fully consumed");
    }
    assert!(mods > 0, "hunk must contain a Mod row");
}

/// Visible contiguity: replay a Mod row's runs tracking the old offset of
/// the last VISIBLE run (Ctx/Del advance it; MovedSrc is skipped by
/// renderers and must not). A Ctx run must start exactly there — anything
/// else claims context the reader cannot verify against old. Del runs
/// re-sync (their strike is visible). MovedSrc may not go backward.
#[test]
fn test_mod_rows_visible_contiguity() {
    let diff = r#"diff --git a/crates/diffuse-render/src/ansi.rs b/crates/diffuse-render/src/ansi.rs
--- a/crates/diffuse-render/src/ansi.rs
+++ b/crates/diffuse-render/src/ansi.rs
@@ -4,7 +4,10 @@
 //! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.

-use std::fmt::{self, Write};
+use std::{
+    borrow::Cow,
+    fmt::{self, Write},
+};

 pub use yansi::{Color, Condition, Paint, Style};
"#;

    let output = run_pipeline_diff(diff);
    let rows = &output.files[0].hunks[0].rows;
    let mut mods = 0;
    for r in rows.iter().filter(|r| r.kind == crate::parse::RowKind::Mod) {
        mods += 1;
        let old: Vec<char> = r.old_text().chars().collect();
        let (mut o, mut vo) = (0usize, 0usize);
        for run in &r.runs {
            let t: Vec<char> = run.text.chars().collect();
            match run.kind {
                crate::merge::RunKind::Ctx => {
                    assert_eq!(
                        o,
                        vo,
                        "ctx run {:?} follows skipped moved-out text; reader cannot verify it (old={:?})",
                        run.text,
                        r.old_text()
                    );
                    assert_eq!(&old[o..o + t.len()], &t[..]);
                    o += t.len();
                    vo = o;
                }
                crate::merge::RunKind::Del => {
                    assert!(o >= vo, "run must not go backward");
                    assert_eq!(&old[o..o + t.len()], &t[..]);
                    o += t.len();
                    vo = o;
                }
                crate::merge::RunKind::MovedSrc => {
                    assert!(o >= vo, "run must not go backward");
                    assert_eq!(&old[o..o + t.len()], &t[..]);
                    o += t.len();
                }
                crate::merge::RunKind::Add | crate::merge::RunKind::MovedTgt => {}
            }
        }
        assert_eq!(o, old.len(), "old side fully consumed");
    }
    assert!(mods > 0, "hunk must contain a Mod row");
}

/// A split statement keeps its head anchor: old `let heading = ...` pairs
/// with the unfinished `let heading =` head (new-432, context indent) and
/// the moved-down value lines attach as aux — never the reverse (anchor on
/// the content-heavy tail with the head as fragment).
#[test]
fn test_reflow_head_anchors_split_declaration() {
    let diff = r#"diff --git a/src/syntax/mod.rs b/src/syntax/mod.rs
--- a/src/syntax/mod.rs
+++ b/src/syntax/mod.rs
@@ -423,9 +423,15 @@ pub fn fence_regions(lines: &[String], profile: Option<&LangProfile>) -> Vec<Fen
 /// `markdownTokens` ported): `#`-headings (the marker muted, the title
 /// accent + bold), inline code (syntax-string), `**bold**`, `*italic*`,
 /// `~~strike~~`, `[text](url)` — the SGR rides the token.
 pub fn markdown_tokens(text: &str) -> Vec<ClassifiedToken> {
-    let heading = Regex::new(r"^(#{1,6})(\s+)(.*)$").expect("the md heading");
+    // The md regexes are FIXED — compile ONCE (the classify runs per row /
+    // per merged fragment; a recompile per call was the classify's cost for
+    // the markdown corpus).
+    static HEADING: OnceLock<Regex> = OnceLock::new();
+    static MD_INLINE: OnceLock<Regex> = OnceLock::new();
+    let heading =
+        HEADING.get_or_init(|| Regex::new(r"^(#{1,6})(\s+)(.*)$").expect("the md heading"));
     if let Some(h) = heading.captures(text) {
         return vec![];
     }
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let anchor = rows
        .iter()
        .find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.old_line == Some(427)
                && r.group == Some(crate::parse::RowGroup::Anchor)
        })
        .expect("reflow anchor mod on the declaration head");
    assert_eq!(
        (anchor.old_line, anchor.new_line),
        (Some(427), Some(432)),
        "anchor must pair the declaration head, got old={:?} new={:?}",
        anchor.old_line,
        anchor.new_line
    );
    assert!(
        rows.iter().any(|r| r.new_line == Some(433)
            && matches!(r.group, Some(crate::parse::RowGroup::Aux { .. }))),
        "moved-down value line must attach as aux"
    );
}

#[test]
fn test_pipeline_mov_src_and_tgt_carry_matching_runs() {
    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,2 @@
 fn alpha() {
-        calculate_total(items);
     remain_a();
 }
@@ -50,2 +49,3 @@
 fn beta() {
+    calculate_total(items);
     remain_b();
 }
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows: Vec<_> = output.files[0]
        .hunks
        .iter()
        .flat_map(|h| &h.rows)
        .filter(|r| r.kind == crate::parse::RowKind::Mov)
        .collect();
    assert_eq!(rows.len(), 2);
    let src = rows
        .iter()
        .find(|r| r.mov_side == Some(crate::wire::MovSide::Src))
        .unwrap();
    let tgt = rows
        .iter()
        .find(|r| r.mov_side == Some(crate::wire::MovSide::Tgt))
        .unwrap();

    assert_eq!(src.old_line, Some(11));
    assert_eq!(src.new_line, None);
    assert_eq!(tgt.old_line, None);
    assert_eq!(tgt.new_line, Some(50));

    assert_eq!(src.text(), "        calculate_total(items);");
    assert_eq!(tgt.text(), "    calculate_total(items);");

    assert_eq!(
        src.runs, tgt.runs,
        "MovSrc and MovTgt must carry identical runs on wire"
    );
    assert!(
        src.runs
            .iter()
            .any(|r| r.kind == crate::merge::RunKind::Del),
        "Runs must include deleted indentation"
    );
}

#[test]
fn test_contiguous_ngram_block_pipeline_integration() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,11 +1,7 @@
 ctx_before
-        step_one();
-        if condition {
-            common_helper();
-            break;
-        }
-        step_two();
-        if other_condition {
-            common_helper();
-            break;
-        }
+            step_one();
+            if condition {
+                common_helper();
+                break;
+            }
 ctx_after
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    for (old_l, new_l) in [(2, 2), (3, 3), (4, 4), (5, 5), (6, 6)] {
        let paired = rows.iter().find(|r| {
            r.kind == crate::parse::RowKind::Mod
                && r.old_line == Some(old_l)
                && r.new_line == Some(new_l)
        });
        assert!(
            paired.is_some(),
            "expected Mod row pairing old line {old_l} with new line {new_l}, rows:\n{:#?}",
            rows.iter()
                .map(|r| (r.kind, r.old_line, r.new_line, r.text()))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_keyword_only_match_across_offset_drift_rejected() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,6 +1,7 @@
 ctx_before
-    if handled {
-        break;
-    }
+    let filler = 1;
+    let another = 2;
+    let third = 3;
+    if !is_binary {
+        do_something();
+    }
 ctx_after
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let spurious_mod = rows.iter().find(|r| {
        r.kind == crate::parse::RowKind::Mod && r.old_line == Some(2) && r.text().contains("if")
    });
    assert!(
        spurious_mod.is_none(),
        "keyword-only lines sharing zero salient tokens must not pair across offsets: {spurious_mod:?}"
    );
}

#[test]
fn test_tier2_residual_declaration_pairing() {
    let diff = r#"diff --git a/src/types.rs b/src/types.rs
--- a/src/types.rs
+++ b/src/types.rs
@@ -20,6 +20,6 @@ ctx_before
-pub enum Side {
-    Old,
-    New,
-}
+pub struct DiffCoord {
+    pub line: u32,
+}
 ctx_after
"#;

    let output = run_pipeline_diff(diff);
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let type_mod = rows.iter().find(|r| {
        r.kind == crate::parse::RowKind::Mod && r.old_line == Some(20) && r.new_line == Some(20)
    });
    assert!(
        type_mod.is_some(),
        "residual declaration replacement pub enum Side <-> pub struct DiffCoord must pair in Tier 2: {rows:?}"
    );
}

#[test]
fn test_tier2_residual_rival_in_content_prevents_theft() {
    let diff = r#"diff --git a/src/types.rs b/src/types.rs
--- a/src/types.rs
+++ b/src/types.rs
@@ -20,6 +20,6 @@ ctx_before
-pub enum Side {
-    Old,
-    New,
-}
+pub struct DiffCoord {
+    pub line: u32,
+}
 ctx_after
"#;

    let mut new_content = std::collections::HashMap::new();
    // Simulate that `pub enum Side {` was moved elsewhere in the new file snapshot.
    new_content.insert(
        "src/types.rs".to_string(),
        "ctx_before\npub struct DiffCoord {\n    pub line: u32,\n}\nctx_after\npub enum Side {\n    Old,\n    New,\n}\n".to_string(),
    );

    let output = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    assert_eq!(output.files.len(), 1);
    let rows = &output.files[0].hunks[0].rows;
    let spurious_mod = rows.iter().find(|r| {
        r.kind == crate::parse::RowKind::Mod && r.old_line == Some(20) && r.new_line == Some(20)
    });
    assert!(
        spurious_mod.is_none(),
        "residual pairing must not steal a line that has a verbatim twin elsewhere in content: {rows:?}"
    );
}

// The `hit_indicator` block in dispatch.rs (scrollbar-grab-drag^!) moved
// with its indentation adjusted: the pre-pass must lock the trimmed-only
// pairs as a strong cross-cluster move group before collapse can steal
// either side into a spurious Mod (e.g. Old 330 with New 352, which share
// only the `saturating_sub(1)` token).
#[test]
fn test_cross_cluster_strong_move_group_guards_against_spurious_mod() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest.join("../..");
    let Ok(repo) = diffuse_git::Repository::open(&repo_root) else {
        return;
    };
    let target = diffuse_git::ReviewTarget::from_repo_args(
        diffuse_git::Repo::from_root(&repo_root),
        vec!["scrollbar-grab-drag^!".to_string()],
        false,
    );
    let Ok(repo_diff) =
        diffuse_git::diff_repository(&repo, &target, 4, diffuse_git::WhitespaceMode::Exact)
    else {
        return;
    };

    let f = repo_diff
        .files
        .into_iter()
        .find(|f| f.path.contains("dispatch.rs"))
        .expect("dispatch.rs must be in diff");
    let new_c = repo_diff.new_content.get(&f.path).map(|s| s.as_str());
    let old_c = repo_diff
        .old_content
        .get(f.old_path.as_deref().unwrap_or(&f.path))
        .map(|s| s.as_str());
    let out = crate::pipeline::file::process_single_diff_file(
        f,
        new_c,
        old_c,
        None,
        &crate::pipeline::PipelineOptions::default(),
    );

    let rows: Vec<_> = out.hunks.iter().flat_map(|h| &h.rows).collect();

    // Verify that Old line 330 is NOT stolen into a spurious Mod with New line 352
    let spurious_mod = rows.iter().find(|r| {
        r.kind == crate::parse::RowKind::Mod && r.old_line == Some(330) && r.new_line == Some(352)
    });
    assert!(
        spurious_mod.is_none(),
        "Old line 330 must not be paired into a spurious Mod with New line 352"
    );

    // Old line 330 must pair with new line 326: the touching halves
    // collapse to a direct Mod, never a move and never the spurious 352.
    let old_330 = rows
        .iter()
        .find(|r| r.old_line == Some(330))
        .expect("Old line 330 must exist");
    assert_eq!(
        old_330.kind,
        crate::parse::RowKind::Mod,
        "Old line 330 must be Mod, got {:?}",
        old_330.kind
    );
    assert_eq!(
        old_330.new_line,
        Some(326),
        "Old line 330 must pair with new line 326"
    );

    // New line 326 must be marked as Mod paired with old 330
    let new_326 = rows
        .iter()
        .find(|r| r.new_line == Some(326))
        .expect("New line 326 must exist");
    assert_eq!(
        new_326.kind,
        crate::parse::RowKind::Mod,
        "New line 326 must be Mod, got {:?}",
        new_326.kind
    );
    assert_eq!(
        new_326.old_line,
        Some(330),
        "New line 326 must pair with old line 330"
    );

    // Verify that new lines 305 and 306 (break; and }) are NOT marked Mov (they
    // must not steal break; and })
    let new_305 = rows
        .iter()
        .find(|r| r.new_line == Some(305))
        .expect("New line 305 must exist");
    assert_ne!(
        new_305.kind,
        crate::parse::RowKind::Mov,
        "New line 305 (break;) must not be Mov (stolen from hit_indicator)"
    );

    let new_306 = rows
        .iter()
        .find(|r| r.new_line == Some(306))
        .expect("New line 306 must exist");
    assert_ne!(
        new_306.kind,
        crate::parse::RowKind::Mov,
        "New line 306 (}}) must not be Mov (stolen from hit_indicator)"
    );
}

// The `hit_indicator` move in dispatch.rs (scrollbar-grab-drag^!) must have
// the Mov-Ctx tail shape: the locked core (old 315-335 <-> new 310-331)
// absorbs the uniform-Δ trailing continuation (old 336-337 <-> new 332-333),
// the broken Ctx far partners stay plain Add leftovers (new 363, 365), and
// absorption stops at the genuine `vis_line` split (old 355, new 351-352).
#[test]
fn test_dispatch_move_block_absorbs_uniform_tail() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest.join("../..");
    let Ok(repo) = diffuse_git::Repository::open(&repo_root) else {
        return;
    };
    let target = diffuse_git::ReviewTarget::from_repo_args(
        diffuse_git::Repo::from_root(&repo_root),
        vec!["scrollbar-grab-drag^!".to_string()],
        false,
    );
    let Ok(repo_diff) =
        diffuse_git::diff_repository(&repo, &target, 4, diffuse_git::WhitespaceMode::Exact)
    else {
        return;
    };

    let f = repo_diff
        .files
        .into_iter()
        .find(|f| f.path.contains("dispatch.rs"))
        .expect("dispatch.rs must be in diff");
    let new_c = repo_diff.new_content.get(&f.path).map(|s| s.as_str());
    let old_c = repo_diff
        .old_content
        .get(f.old_path.as_deref().unwrap_or(&f.path))
        .map(|s| s.as_str());
    let out = crate::pipeline::file::process_single_diff_file(
        f,
        new_c,
        old_c,
        None,
        &crate::pipeline::PipelineOptions::default(),
    );

    let rows: Vec<_> = out.hunks.iter().flat_map(|h| &h.rows).collect();
    let kind_of_old = |line: u32| {
        rows.iter()
            .find(|r| r.old_line == Some(line))
            .unwrap_or_else(|| panic!("Old line {line} must exist"))
            .kind
    };
    let kind_of_new = |line: u32| {
        rows.iter()
            .find(|r| r.new_line == Some(line))
            .unwrap_or_else(|| panic!("New line {line} must exist"))
            .kind
    };

    // Locked core plus absorbed uniform-Δ tail (verified: the whole
    // continuation through old 354 / new 350 is a rigid +4 shift): the
    // touching halves collapse into direct pairs — Mod where the indent
    // differs, Ctx for the identical blank pairs (old 338 / new 334 and
    // old 352 / new 348).
    for line in [315..=337, 339..=351, 353..=354].into_iter().flatten() {
        assert_eq!(
            kind_of_old(line),
            crate::parse::RowKind::Mod,
            "Old line {line} must be Mod (touching halves collapse)"
        );
    }
    for line in [310..=333, 335..=347, 349..=350].into_iter().flatten() {
        assert_eq!(
            kind_of_new(line),
            crate::parse::RowKind::Mod,
            "New line {line} must be Mod (touching halves collapse)"
        );
    }
    for line in [338, 352] {
        assert_eq!(
            kind_of_old(line),
            crate::parse::RowKind::Ctx,
            "Old line {line} must be Ctx (identical blank pair collapses)"
        );
    }
    for line in [334, 348] {
        assert_eq!(
            kind_of_new(line),
            crate::parse::RowKind::Ctx,
            "New line {line} must be Ctx (identical blank pair collapses)"
        );
    }

    // Orphaned Ctx far partners fall back to plain leftovers.
    for line in [363, 364, 365] {
        assert_ne!(
            kind_of_new(line),
            crate::parse::RowKind::Mov,
            "New line {line} must not be Mov (orphaned far partner)"
        );
    }

    // The coincidental `}`/`break;` context barriers (old 309-311 vs new
    // 345-347) split before pairing, so old 308-312 pair with their true
    // pre-move partners (new 304-308, uniform -4 shift) instead of stranding
    // as Del orphans; new 345-347 stay paired with old 349-351 (asserted
    // below via the absorbed-tail expectations).
    for (old_line, new_line) in [(308, 304), (309, 305), (310, 306), (311, 307), (312, 308)] {
        assert_eq!(
            kind_of_old(old_line),
            crate::parse::RowKind::Mod,
            "Old line {old_line} must be Mod (true pre-move partner, not orphan)"
        );
        assert_eq!(
            kind_of_new(new_line),
            crate::parse::RowKind::Mod,
            "New line {new_line} must be Mod (true pre-move partner, not orphan)"
        );
    }
    // The absorbed tail rows collapse with their partners: new 345-347
    // are Mod rows paired with old 349-351, not Mov leftovers.
    let hunk_rows: Vec<_> = out.hunks.iter().flat_map(|h| &h.rows).collect();
    for (new, old) in [(345, 349), (346, 350), (347, 351)] {
        let row = hunk_rows
            .iter()
            .find(|r| r.new_line == Some(new))
            .unwrap_or_else(|| panic!("New line {new} must exist"));
        assert_eq!(row.kind, crate::parse::RowKind::Mod);
        assert_eq!(row.old_line, Some(old));
    }

    // The genuine `vis_line` split edit stops the tail: old 355 stays a
    // Del, new 351-352 stay Adds.
    assert_ne!(
        kind_of_old(355),
        crate::parse::RowKind::Mov,
        "Old line 355 must not be Mov (genuine edit stops the tail)"
    );
    for line in [351, 352] {
        assert_ne!(
            kind_of_new(line),
            crate::parse::RowKind::Mov,
            "New line {line} must not be Mov (genuine edit stops the tail)"
        );
    }
}

// The dispatch.rs relocation (scrollbar-grab-drag^!) is a rigid -4 shift of
// old 314-354 onto new 310-350, but the mov expansion renders the target
// half (ending new 348) immediately followed by the source half (starting
// old 314): the halves touch. Touching corresponding halves are not a
// relocation worth showing as a move; they collapse into direct Mod rows.
#[test]
fn test_touching_move_halves_collapse_to_mod() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest.join("../..");
    let Ok(repo) = diffuse_git::Repository::open(&repo_root) else {
        return;
    };
    let target = diffuse_git::ReviewTarget::from_repo_args(
        diffuse_git::Repo::from_root(&repo_root),
        vec!["scrollbar-grab-drag^!".to_string()],
        false,
    );
    let Ok(repo_diff) =
        diffuse_git::diff_repository(&repo, &target, 4, diffuse_git::WhitespaceMode::Exact)
    else {
        return;
    };

    let f = repo_diff
        .files
        .into_iter()
        .find(|f| f.path.contains("dispatch.rs"))
        .expect("dispatch.rs must be in diff");
    let new_c = repo_diff.new_content.get(&f.path).map(|s| s.as_str());
    let old_c = repo_diff
        .old_content
        .get(f.old_path.as_deref().unwrap_or(&f.path))
        .map(|s| s.as_str());
    let out = crate::pipeline::file::process_single_diff_file(
        f,
        new_c,
        old_c,
        None,
        &crate::pipeline::PipelineOptions::default(),
    );

    let rows: Vec<_> = out.hunks.iter().flat_map(|h| &h.rows).collect();
    let kind_of_old = |line: u32| {
        rows.iter()
            .find(|r| r.old_line == Some(line))
            .unwrap_or_else(|| panic!("Old line {line} must exist"))
            .kind
    };
    let kind_of_new = |line: u32| {
        rows.iter()
            .find(|r| r.new_line == Some(line))
            .unwrap_or_else(|| panic!("New line {line} must exist"))
            .kind
    };

    // The rigid -4 shift pairs old 314-354 with new 310-350 directly:
    // indent-differing pairs collapse to Mod, the two identical blank
    // pairs (old 338/new 334, old 352/new 348) collapse to Ctx.
    for line in [314..=337, 339..=351, 353..=354].into_iter().flatten() {
        assert_eq!(
            kind_of_old(line),
            crate::parse::RowKind::Mod,
            "Old line {line} must be Mod (touching move halves collapse)"
        );
    }
    for line in [310..=333, 335..=347, 349..=350].into_iter().flatten() {
        assert_eq!(
            kind_of_new(line),
            crate::parse::RowKind::Mod,
            "New line {line} must be Mod (touching move halves collapse)"
        );
    }
    for line in [338, 352] {
        assert_eq!(
            kind_of_old(line),
            crate::parse::RowKind::Ctx,
            "Old line {line} must be Ctx (identical blank pair collapses)"
        );
    }
    for line in [334, 348] {
        assert_eq!(
            kind_of_new(line),
            crate::parse::RowKind::Ctx,
            "New line {line} must be Ctx (identical blank pair collapses)"
        );
    }

    // The touching boundary itself is gone: new 348 (old tgt end) must no
    // longer be immediately followed by old 314 (old src beg) in row order.
    // (Correspondence-aware touching is covered by the moves unit tests.)
    let pos_of_new = |line: u32| {
        rows.iter()
            .position(|r| r.new_line == Some(line))
            .unwrap_or_else(|| panic!("New line {line} must exist"))
    };
    let pos_of_old = |line: u32| {
        rows.iter()
            .position(|r| r.old_line == Some(line))
            .unwrap_or_else(|| panic!("Old line {line} must exist"))
    };
    assert_ne!(
        pos_of_old(314),
        pos_of_new(348) + 1,
        "old 314 must not immediately follow new 348 in row order"
    );
}
