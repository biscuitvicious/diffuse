//! Flat synchronization — streaming append and reload with cursor preservation.

use crate::{
    core::wire::{OutputFile, PipelineOutput},
    types::{FlatRow, HunkStart},
};

use super::AppState;

impl AppState {
    pub fn append_file(&mut self, file: OutputFile) {
        let fi = self.output.files.len();
        if file.status == "untracked" {
            self.collapsed_files.insert(fi);
        }

        let is_collapsed = self.collapsed_files.contains(&fi);
        let meta = Self::build_file_hunk_meta(&file);
        self.hunk_meta.push(meta);

        let file_header_idx = self.flat.len();
        self.flat.push(Self::file_header_row(fi));

        if !is_collapsed {
            if self.raw_diff {
                let hunks = if !file.raw_hunks.is_empty() {
                    &file.raw_hunks
                } else {
                    &file.hunks
                };
                for (hi, h) in hunks.iter().enumerate() {
                    self.flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hi,
                        row_idx: 0,
                        is_file_header: false,
                        is_hunk_divider: true,
                    });
                    self.hunk_starts.push(HunkStart {
                        flat: self.flat.len() - 1,
                        file: fi,
                        hunk: hi,
                    });
                    for ri in 0..h.rows.len() {
                        self.flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            row_idx: ri,
                            is_file_header: false,
                            is_hunk_divider: false,
                        });
                    }
                }
            } else if !file.rows.is_empty() {
                for ri in 0..file.rows.len() {
                    self.flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: 0,
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                }
                self.hunk_starts
                    .extend(Self::build_file_hunk_starts_for_file(
                        &self.flat,
                        fi,
                        &file,
                        file_header_idx,
                    ));
            } else {
                for (hi, h) in file.hunks.iter().enumerate() {
                    let hunk_start_flat = self.flat.len();
                    self.hunk_starts.push(HunkStart {
                        flat: hunk_start_flat,
                        file: fi,
                        hunk: hi,
                    });
                    for ri in 0..h.rows.len() {
                        self.flat.push(FlatRow {
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

        self.output.files.push(file);

        if self.row_idx == 0
            && self.flat.len() > 1
            && self.flat[0].is_file_header
            && let Some(pos) = self
                .flat
                .iter()
                .position(|r| self.flat_row_coord(r).is_some())
        {
            self.row_idx = pos;
        }
        self.vis_line = 0;
    }

    pub(crate) fn rebuild_flat_preserving_cursor(&mut self, target_fi: usize) {
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // If target file is now collapsed, jump to its header row
        if self.collapsed_files.contains(&target_fi) {
            self.row_idx = self.first_row_of_file(target_fi);
        } else if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }
        self.adjust_scroll();
    }

    pub(crate) fn adjust_scroll(&mut self) {
        let max_rows = self.viewport_height.saturating_sub(2);
        if self.row_idx < self.scroll {
            self.scroll = self.row_idx;
        }
        if self.row_idx >= self.scroll + max_rows {
            self.scroll = self.row_idx.saturating_sub(max_rows.saturating_sub(1));
        }
    }

    pub fn reload(&mut self, output: PipelineOutput) {
        let (file_path, coord, prefer_new) = {
            let cur_path = self
                .current_file()
                .map(|f| f.path.clone())
                .unwrap_or_default();
            let row = self.current_flat_row();
            let c = row.and_then(|r| self.flat_row_coord(r)).unwrap_or(0);
            let prefer = row
                .and_then(|r| self.row_of(r))
                .map(|r| r.new_line.is_some())
                .unwrap_or(true);
            (cur_path, c, prefer)
        };

        for (fi, f) in output.files.iter().enumerate() {
            if f.status == "untracked" && !self.output.files.iter().any(|old| old.path == f.path) {
                self.collapsed_files.insert(fi);
            }
        }

        self.output = output;
        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            self.should_quit = true;
            return;
        }

        self.hunk_meta = Self::build_hunk_meta(&self.output);
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.anchor = None;
        self.confirmed_sel = None;

        let target_fi = self
            .output
            .files
            .iter()
            .position(|f| f.path == file_path)
            .unwrap_or(0);

        self.goto_coord(target_fi, coord, prefer_new);
    }

    /// Jump to the first flat row in `file` carrying `coord` (preferring the
    /// given side); fall back to the file's first coordinate row — the
    /// reference's coordinate-resume behavior.
    pub(crate) fn goto_coord(&mut self, file: usize, coord: u32, prefer_new: bool) {
        let mut idx = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            let side_ok = if prefer_new {
                r.new_line.is_some()
            } else {
                r.old_line.is_some()
            };
            if Self::row_coord(r) == Some(coord) && side_ok {
                idx = Some(i);
                break;
            }
        }
        let idx = idx.unwrap_or_else(|| self.first_row_of_file(file));
        self.row_idx = idx;
        self.scroll = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app::AppState,
        core::{
            PipelineOptions,
            streaming::stream_diff_pipeline,
            wire::{OutputFile, PipelineOutput},
        },
    };
    #[test]
    fn test_incremental_file_append_and_streaming_navigation() {
        let diff = "diff --git a/file1.rs b/file1.rs\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_mod() {}\ndiff --git a/file2.rs b/file2.rs\n--- a/file2.rs\n+++ b/file2.rs\n@@ -1,2 +1,2 @@\n-fn two() {}\n+fn two_mod() {}\n";
        let new_content = [
            ("file1.rs".to_string(), "fn one_mod() {}\n".to_string()),
            ("file2.rs".to_string(), "fn two_mod() {}\n".to_string()),
        ]
        .into_iter()
        .collect();

        let consumer = stream_diff_pipeline(
            diff,
            Some(new_content),
            None,
            PipelineOptions::default(),
            Some(2),
        );

        let initial_output = PipelineOutput {
            files: Vec::new(),
            branch: "main".into(),
            untracked_skipped: Vec::new(),
        };

        let mut state = AppState::new(
            initial_output,
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-streaming-notes.json"),
        );
        state.is_streaming = true;
        assert_eq!(state.output.files.len(), 0);

        let files: Vec<OutputFile> = consumer.collect();
        assert_eq!(files.len(), 2);

        // Ingest file 0
        state.append_file(files[0].clone());
        assert_eq!(state.output.files.len(), 1);
        assert_eq!(state.current_file_idx(), 0);
        assert_eq!(state.output.files[0].path, "file1.rs");

        // Ingest file 1
        state.append_file(files[1].clone());
        assert_eq!(state.output.files.len(), 2);
        assert_eq!(state.output.files[1].path, "file2.rs");

        // Navigate to next file
        state.next_file();
        assert_eq!(state.current_file_idx(), 1);
        assert_eq!(state.current_file().unwrap().path, "file2.rs");

        // Navigate to prev file
        state.prev_file();
        assert_eq!(state.current_file_idx(), 0);
        assert_eq!(state.current_file().unwrap().path, "file1.rs");
    }

    #[test]
    fn test_streaming_append_file_multi_hunk_with_gap() {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let file = std::fs::File::open("../../tests/golden/mirage-2025-08.wire.json.gz")
            .or_else(|_| std::fs::File::open("tests/golden/mirage-2025-08.wire.json.gz"))
            .expect("must open mirage-2025-08.wire.json.gz");
        let mut gz = GzDecoder::new(file);
        let mut json_str = String::new();
        gz.read_to_string(&mut json_str).unwrap();
        let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();

        // Simulate streaming: start with empty AppState and append files one by one
        let mut empty_output = output.clone();
        empty_output.files.clear();
        let mut state = AppState::new(
            empty_output,
            "mirage".into(),
            std::env::temp_dir().join("diffuse-mirage-notes.json"),
        );

        for file in output.files {
            state.append_file(file);
        }

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

        let cache_hunks: Vec<_> = state
            .hunk_starts
            .iter()
            .filter(|hs| hs.file == fi)
            .collect();
        assert_eq!(
            cache_hunks.len(),
            state.output.files[fi].hunks.len(),
            "streaming append_file must register hunk starts for all hunks in cache.rs"
        );

        let frame = state.render_frame(140, 40);
        assert!(
            frame.contains("───── H1 ·"),
            "streaming render must contain H1 divider"
        );
        assert!(
            frame.contains("───── H2 ·"),
            "streaming render must contain H2 divider"
        );
    }
}
