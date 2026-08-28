use super::*;
use crate::core::PipelineOptions;

#[test]
fn test_output_ends_with_terminal_reset() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_pipeline_output(&out, &DiffuseTheme::MAGIC, true);
    assert!(
        rendered.ends_with("\x1b[0m"),
        "terminal must be left in a clean SGR state"
    );
}
