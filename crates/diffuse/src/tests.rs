use super::*;

#[test]
fn test_unified_facade_submodules() {
    let diff =
        "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let out = core::pipeline::run_pipeline(diff, None);
    assert_eq!(out.files.len(), 1);

    let theme = render::default_theme();
    let rendered = render::render_pipeline_output(&out, &theme, false);
    assert!(!rendered.is_empty());

    let prof = syntax::registry::lang_profile("rs");
    assert!(prof.is_some());
}

#[test]
fn test_prelude_reexports() {
    use crate::prelude::*;

    let diff =
        "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let out = run_pipeline(diff, None);
    assert_eq!(out.files.len(), 1);
    let theme = default_theme();
    let rendered = render_pipeline_output(&out, &theme, false);
    assert!(!rendered.is_empty());

    let md = render_markdown_output(&out, &MarkdownOptions::default());
    assert!(md.contains("### Diff Summary"));
    assert!(md.contains("```diff"));
}
