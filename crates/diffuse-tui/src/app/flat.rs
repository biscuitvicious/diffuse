//! Flat model — construction and indexing of the visual flat list.

use diffuse_core::wire::{OutputFile, PipelineOutput};

use crate::types::{FlatRow, HunkMeta, HunkStart};

use super::AppState;

impl AppState {
    pub(crate) fn build_visual_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let flat = Self::build_flat(output, collapsed_files);
        let hunk_starts = Self::build_hunk_starts(output, &flat);
        (flat, hunk_starts)
    }

    /// The raw unified-diff flat sequence: per file a header banner, then per
    /// hunk an `@@` header row and the hunk rows exactly as loaded in the diff.
    /// Hunk navigation jumps to the `@@` rows.
    pub(crate) fn build_raw_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let mut flat = Vec::new();
        let mut hunk_starts = Vec::new();

        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            let hunks = if !f.raw_hunks.is_empty() {
                &f.raw_hunks
            } else {
                &f.hunks
            };
            for (hi, h) in hunks.iter().enumerate() {
                flat.push(FlatRow {
                    file_idx: fi,
                    hunk_idx: hi,
                    row_idx: 0,
                    is_file_header: false,
                    is_hunk_divider: true,
                });
                hunk_starts.push(HunkStart {
                    flat: flat.len() - 1,
                    file: fi,
                    hunk: hi,
                });
                for ri in 0..h.rows.len() {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hi,
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                }
            }
        }
        (flat, hunk_starts)
    }

    /// Build the flat row list (shared by the visual and raw representations).
    pub(crate) fn build_flat(
        output: &PipelineOutput,
        collapsed_files: &std::collections::HashSet<usize>,
    ) -> Vec<FlatRow> {
        let mut flat = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            if !f.rows.is_empty() {
                for ri in 0..f.rows.len() {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: 0,
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                }
            } else {
                for (hi, h) in f.hunks.iter().enumerate() {
                    for ri in 0..h.rows.len() {
                        flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            row_idx: ri,
                            is_file_header: false,
                            is_hunk_divider: false,
                        });
                    }
                }
            }
        }
        flat
    }

    pub(crate) fn file_header_row(fi: usize) -> FlatRow {
        FlatRow {
            file_idx: fi,
            hunk_idx: 0,
            row_idx: 0,
            is_file_header: true,
            is_hunk_divider: false,
        }
    }

    /// Locate each hunk's first content row in the flat sequence by file index
    /// and coordinate envelope.
    pub(crate) fn build_file_hunk_starts_for_file(
        flat: &[FlatRow],
        fi: usize,
        f: &OutputFile,
        file_header_idx: usize,
    ) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        if f.hunks.is_empty() {
            return starts;
        }
        let mut search_from = file_header_idx + 1;
        for (hi, h) in f.hunks.iter().enumerate() {
            if hi == 0 {
                if search_from < flat.len() && flat[search_from].file_idx == fi {
                    starts.push(HunkStart {
                        flat: search_from,
                        file: fi,
                        hunk: hi,
                    });
                }
            } else {
                let target_start = h
                    .rows
                    .iter()
                    .find_map(|r| r.new_line.or(r.old_line))
                    .unwrap_or({
                        if h.new_count > 0 {
                            h.new_start
                        } else {
                            h.old_start
                        }
                    });
                let mut found = None;
                for (idx, fr) in flat.iter().enumerate().skip(search_from) {
                    if fr.file_idx != fi {
                        break;
                    }
                    if fr.is_file_header || fr.is_hunk_divider {
                        continue;
                    }
                    let row = if !f.rows.is_empty() {
                        f.rows.get(fr.row_idx)
                    } else {
                        f.hunks
                            .get(fr.hunk_idx)
                            .and_then(|h| h.rows.get(fr.row_idx))
                    };
                    if let Some(r) = row {
                        let coord = Self::row_coord(r);
                        if let Some(c) = coord
                            && c >= target_start
                        {
                            found = Some(idx);
                            break;
                        }
                    }
                }
                if let Some(idx) = found {
                    starts.push(HunkStart {
                        flat: idx,
                        file: fi,
                        hunk: hi,
                    });
                    search_from = idx + 1;
                }
            }
        }
        starts
    }

    pub(crate) fn build_hunk_starts(output: &PipelineOutput, flat: &[FlatRow]) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            let file_header_idx = flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_file_header)
                .unwrap_or(0);
            starts.extend(Self::build_file_hunk_starts_for_file(
                flat,
                fi,
                f,
                file_header_idx,
            ));
        }
        starts
    }

    /// Per-file per-hunk context counts — the leading/trailing context rows
    /// carried through the wire on each hunk.
    pub(crate) fn build_file_hunk_meta(f: &OutputFile) -> Vec<HunkMeta> {
        f.hunks
            .iter()
            .map(|h| {
                let mut above = 0usize;
                for r in &h.rows {
                    if r.kind == diffuse_core::parse::RowKind::Ctx {
                        above += 1;
                    } else {
                        break;
                    }
                }
                let mut below = 0usize;
                for r in h.rows.iter().rev() {
                    if r.kind == diffuse_core::parse::RowKind::Ctx {
                        below += 1;
                    } else {
                        break;
                    }
                }
                HunkMeta { above, below }
            })
            .collect()
    }

    pub(crate) fn build_hunk_meta(output: &PipelineOutput) -> Vec<Vec<HunkMeta>> {
        output
            .files
            .iter()
            .map(Self::build_file_hunk_meta)
            .collect()
    }

    pub fn first_row_of_file(&self, fi: usize) -> usize {
        self.flat
            .iter()
            .position(|r| r.file_idx == fi && r.is_file_header)
            .unwrap_or(0)
    }

    pub fn last_row_of_file(&self, fi: usize) -> usize {
        match self.flat.iter().position(|r| r.file_idx > fi) {
            Some(next) => next - 1,
            None => self.flat.len().saturating_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{app::AppState, view::hunk_divider_text};
    use diffuse_core::{PipelineOptions, wire::PipelineOutput};

    fn sample_output_two_hunks() -> PipelineOutput {
        let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,4 @@\n fn a() {\n+    let x = 1;\n     return 1;\n }\n@@ -30,3 +31,4 @@ fn b() {\n fn b() {\n+    let y = 2;\n     return 2;\n }\n";
        let mut old_lines = vec![
            "fn a() {".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 4..=29 {
            old_lines.push(format!("    // filler {i}"));
        }
        old_lines.push("fn b() {".to_string());
        old_lines.push("    return 2;".to_string());
        old_lines.push("}".to_string());

        let mut new_lines = vec![
            "fn a() {".to_string(),
            "    let x = 1;".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 5..=30 {
            new_lines.push(format!("    // filler {i}"));
        }
        new_lines.push("fn b() {".to_string());
        new_lines.push("    let y = 2;".to_string());
        new_lines.push("    return 2;".to_string());
        new_lines.push("}".to_string());

        let new_content = [("foo.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_content = [("foo.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        )
    }
    #[test]
    fn test_hunk_starts_and_navigation() {
        let mut state = AppState::new(
            sample_output_two_hunks(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        assert_eq!(state.hunk_starts.len(), 2, "both hunks must be located");
        // Start from file header (row 0), next jumps to hunk 1's start
        state.row_idx = 0;
        state.next_hunk();
        assert_eq!(state.current_hunk(), Some(0));

        state.next_hunk();
        let at = state.row_idx;
        assert_eq!(
            state.current_hunk(),
            Some(1),
            "next_hunk must land in hunk 2"
        );
        assert!(
            at > state.hunk_starts[0].flat,
            "next_hunk must jump past hunk 1"
        );

        state.prev_hunk();
        assert_eq!(state.current_hunk(), Some(0), "prev_hunk returns to hunk 1");
    }

    #[test]
    fn test_untracked_files_collapsed_by_default() {
        let diff = r#"diff --git a/tracked.rs b/tracked.rs
index 1111111..2222222 100644
--- a/tracked.rs
+++ b/tracked.rs
@@ -1 +1,2 @@
 fn main() {}
+fn extra() {}
"#;
        let new_content = [
            (
                "tracked.rs".to_string(),
                "fn main() {}\nfn extra() {}\n".to_string(),
            ),
            (
                "untracked.rs".to_string(),
                "fn new_untracked() {}\n".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let mut out = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            None,
            PipelineOptions::default(),
        );
        // Add an untracked file to output
        out.files.push(diffuse_core::wire::OutputFile {
            path: "untracked.rs".to_string(),
            status: "untracked".to_string(),
            new_line_count: 1,
            old_line_count: 0,
            new_content: vec!["fn new_untracked() {}".to_string()],
            old_content: Vec::new(),
            hunks: vec![diffuse_core::wire::OutputHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                rows: vec![diffuse_core::wire::OutputRow {
                    kind: diffuse_core::parse::RowKind::Add,
                    new_line: Some(1),
                    old_line: None,
                    text: "fn new_untracked() {}".to_string(),
                    del_text: None,
                    spans: Default::default(),
                    runs: Vec::new(),
                    wholesale: false,
                    moved: false,
                }],
            }],
            raw_hunks: Vec::new(),
            counts: diffuse_core::counts::FileCounts {
                adds: 1,
                dels: 0,
                mods: 0,
                movs: 0,
            },
            fences: Vec::new(),
            notes: Vec::new(),
            content_runs: Vec::new(),
            rows: Vec::new(),
        });

        let mut state = AppState::new(
            out,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes-untracked.json"),
        );

        assert!(
            state.collapsed_files.contains(&1),
            "untracked file (index 1) must be collapsed by default"
        );
        assert!(
            !state.collapsed_files.contains(&0),
            "tracked file (index 0) must be expanded by default"
        );

        // The flat sequence must contain rows for file 0, but only the header row for
        // file 1
        let file_1_rows: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
        assert_eq!(
            file_1_rows.len(),
            1,
            "collapsed file only has 1 flat row (header)"
        );
        assert!(file_1_rows[0].is_file_header);

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("▶ untracked.rs (untracked)"),
            "frame shows collapsed indicator"
        );

        // Toggle collapse on file 1 to expand it
        state.toggle_file_collapse(1);
        assert!(
            !state.collapsed_files.contains(&1),
            "untracked file is now expanded"
        );
        let file_1_rows_expanded: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
        assert!(
            file_1_rows_expanded.len() > 1,
            "expanded file has header and content rows"
        );

        let frame_expanded = state.render_frame(100, 30);
        assert!(
            frame_expanded.contains("▼ untracked.rs (untracked)"),
            "frame shows expanded indicator"
        );
    }

    #[test]
    fn test_hunks_with_hidden_gap_has_hunk_header_after_separator() {
        let diff = r#"diff --git a/cache.rs b/cache.rs
index 1111111..2222222 100644
--- a/cache.rs
+++ b/cache.rs
@@ -1,3 +1,4 @@
 fn a() {
 +    let x = 1;
     return 1;
 }
@@ -85,3 +86,4 @@
 fn b() {
 +    let y = 2;
     return 2;
 }
"#;
        let mut old_lines = vec![
            "fn a() {".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 4..=84 {
            old_lines.push(format!("    // filler {i}"));
        }
        old_lines.push("fn b() {".to_string());
        old_lines.push("    return 2;".to_string());
        old_lines.push("}".to_string());

        let mut new_lines = vec![
            "fn a() {".to_string(),
            "    let x = 1;".to_string(),
            "    return 1;".to_string(),
            "}".to_string(),
        ];
        for i in 5..=85 {
            new_lines.push(format!("    // filler {i}"));
        }
        new_lines.push("fn b() {".to_string());
        new_lines.push("    let y = 2;".to_string());
        new_lines.push("    return 2;".to_string());
        new_lines.push("}".to_string());

        let new_content = [("cache.rs".to_string(), new_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let old_content = [("cache.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
        let output = diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        );
        let mut state = AppState::new(
            output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );

        assert_eq!(state.hunk_starts.len(), 2, "must find 2 hunk starts");

        // Jump to hunk 2
        let h2_start = state.hunk_starts[1].flat;
        state.next_hunk();
        assert_eq!(
            state.row_idx, h2_start,
            "next_hunk must jump to the first row of Hunk 2"
        );
        assert_eq!(
            state.current_hunk(),
            Some(1),
            "current_hunk must be 1 when on the first row of Hunk 2"
        );

        // Render frame with viewport covering hunk 2 start: must show H2 divider with
        // hidden lines gap and buttons
        let gap = state.hunk_leading_gap(0, 1);
        let frame = state.render_frame(120, 40);
        let h2_txt = hunk_divider_text(1, &state.output.files[0].hunks[1], gap);
        assert!(
            frame.contains(&h2_txt),
            "rendered frame must show H2 divider with merged gap info"
        );
        assert!(
            !frame.contains("⋯ 72 lines hidden"),
            "standalone separator row must NOT be rendered"
        );
    }

    #[test]
    fn test_mirage_golden_cache_rs_hunk_divider() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let file = std::fs::File::open("../../tests/golden/mirage-2025-08.wire.json.gz")
            .or_else(|_| std::fs::File::open("tests/golden/mirage-2025-08.wire.json.gz"))
            .expect("must open mirage-2025-08.wire.json.gz");
        let mut gz = GzDecoder::new(file);
        let mut json_str = String::new();
        gz.read_to_string(&mut json_str).unwrap();
        let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();
        let mut state = AppState::new(
            output,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-mirage-notes.json"),
        );

        // Find cache.rs file index
        let fi = state
            .output
            .files
            .iter()
            .position(|f| f.path.contains("cache.rs"))
            .expect("cache.rs must exist");

        // Jump to cache.rs
        let cache_file_row = state
            .flat
            .iter()
            .position(|fr| fr.file_idx == fi && fr.is_file_header)
            .expect("file header row for cache.rs");
        state.jump_to_row(cache_file_row, false);

        let frame = state.render_frame(140, 40);
        assert!(frame.contains("H1"), "must contain H1 divider");
        assert!(frame.contains("H2"), "must contain H2 divider");
    }
}
