use std::collections::HashMap;

use super::*;
use crate::{
    merge::RunKind,
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
    let mut contents = HashMap::new();
    contents.insert(
        "test.rs".to_string(),
        "// The deepest layer the below-walk may descend to: a directory found with\n".to_string(),
    );
    let output = run_pipeline(diff, &contents);
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
        .map(|r| (r.kind, r.text.as_str()))
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
        .map(|r| (r.kind, r.text.as_str()))
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
    let mut contents = HashMap::new();
    contents.insert(
        "src/a.rs".to_string(),
        "new line one\nnew line two".to_string(),
    );

    let wire = run_pipeline(diff, &contents);
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
    let mut contents = HashMap::new();
    contents.insert(
        "src/lib.rs".to_string(),
        "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        layers: unresolved,\n    })\n}".to_string(),
    );
    let mut old_contents = HashMap::new();
    old_contents.insert(
        "src/lib.rs".to_string(),
        "pub fn into_filesystem() {\n    Box::new(OverlayFilesystem {\n        resolved_layers: unresolved,\n    })\n}".to_string(),
    );
    let out = run_pipeline_full(diff, &contents, Some(&old_contents), None, None);
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
    assert_eq!(del_run.map(|r| r.text.as_str()), Some("resolved_"));
}
