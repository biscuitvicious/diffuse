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

    let lcs = algo::MyersOnD;
    let indices = algo::LcsEngine::lcs_indices(&lcs, &[1, 2], &[1, 2]);
    assert_eq!(indices, vec![(0, 0), (1, 1)]);
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

    let raw = render_raw_pipeline_output(&out, &theme);
    assert!(!raw.is_empty());

    const { assert!(DEFAULT_CLUSTER_BUDGET > 0) };
    const { assert!(DEFAULT_CLUSTER_BANDWIDTH_MIN > 0) };

    let stripped = strip_ansi("\x1b[31mhello\x1b[0m");
    assert_eq!(stripped, "hello");
    assert!(!SGR_RESET.is_empty());

    let _status = NoteStatus::Open;

    let prof: Option<&LangProfile> = lang_profile("rs");
    assert!(prof.is_some());

    let lcs = MyersOnD;
    assert_eq!(lcs.lcs_indices(&[1], &[1]), vec![(0, 0)]);
}

#[test]
fn test_render_with_expand_mods_options() {
    use crate::prelude::*;

    let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old_value\n+new_value\n";
    let out = run_pipeline(diff, None);
    let theme = default_theme();

    // Default / None / Some(false): collapsed mod line with ~
    let collapsed_none = render_pipeline_output_with_options(&out, &theme, false, 80, None, None);
    let collapsed_false =
        render_pipeline_output_with_options(&out, &theme, false, 80, Some(false), None);
    assert_eq!(collapsed_none, collapsed_false);
    let stripped_collapsed = strip_ansi(&collapsed_none);
    assert!(
        stripped_collapsed.contains("~"),
        "collapsed mode must render modification row glyph '~'"
    );
    assert!(!stripped_collapsed.contains("┌-"));
    assert!(!stripped_collapsed.contains("└+"));

    // Some(true): expanded mod lines with ┌- and └+
    let expanded = render_pipeline_output_with_options(&out, &theme, false, 80, Some(true), None);
    let stripped_expanded = strip_ansi(&expanded);
    assert!(
        stripped_expanded.contains("┌-"),
        "expanded mode must contain deletion line '┌-'"
    );
    assert!(
        stripped_expanded.contains("└+"),
        "expanded mode must contain addition line '└+'"
    );
    assert!(stripped_expanded.contains("old_value"));
    assert!(stripped_expanded.contains("new_value"));
}
