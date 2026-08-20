//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use crate::{
    bin_pipeline::{OutputFile, OutputRow, PipelineOutput},
    render::{DiffuseTheme, S_ADD, S_DEL, S_MOD, render_row_segment, styled_prefix},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file_idx: usize,
    pub start: u32,
    pub end: u32,
    pub side_is_new: bool,
}

pub struct FlatRow {
    pub file_idx: usize,
    pub hunk_idx: usize,
    pub is_file_header: bool,
    pub row: OutputRow,
}

pub struct AppState {
    pub output: PipelineOutput,
    pub flat: Vec<FlatRow>,
    pub row_idx: usize,
    pub vis_line: usize,
    pub scroll: usize,
    pub col_offset: usize,
    pub num_buffer: String,
    pub syntax_on: bool,
    pub word_diff: bool,
    pub show_help: bool,
    pub note_draft: Option<String>,
    pub anchor: Option<usize>,
    pub confirmed_sel: Option<Selection>,
    pub theme: DiffuseTheme,
    pub repo: String,
    pub should_quit: bool,
    pub status_message: Option<(String, bool)>, // (message, is_error)
    pub viewport_height: usize,
}

impl AppState {
    pub fn new(output: PipelineOutput, repo: String) -> Self {
        let mut flat = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            // File header banner row
            flat.push(FlatRow {
                file_idx: fi,
                hunk_idx: 0,
                is_file_header: true,
                row: OutputRow {
                    kind: crate::parse::RowKind::File,
                    new_line: None,
                    old_line: None,
                    text: f.path.clone(),
                    del_text: None,
                    spans: Default::default(),
                    runs: Vec::new(),
                    wholesale: false,
                    moved: false,
                },
            });

            // Flatten rows from file's precomputed visible rows or hunks
            if !f.rows.is_empty() {
                for r in &f.rows {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: 0,
                        is_file_header: false,
                        row: r.clone(),
                    });
                }
            } else {
                for (hi, h) in f.hunks.iter().enumerate() {
                    for r in &h.rows {
                        flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            is_file_header: false,
                            row: r.clone(),
                        });
                    }
                }
            }
        }

        // Start on first coordinate row if available
        let start_row = flat
            .iter()
            .position(|r| !r.is_file_header && Self::row_coord(&r.row).is_some())
            .unwrap_or(0);

        Self {
            output,
            flat,
            row_idx: start_row,
            vis_line: 0,
            scroll: 0,
            col_offset: 0,
            num_buffer: String::new(),
            syntax_on: true,
            word_diff: true,
            show_help: false,
            note_draft: None,
            anchor: None,
            confirmed_sel: None,
            theme: DiffuseTheme::MAGIC,
            repo,
            should_quit: false,
            status_message: None,
            viewport_height: 20,
        }
    }

    pub fn current_file_idx(&self) -> usize {
        self.flat.get(self.row_idx).map(|r| r.file_idx).unwrap_or(0)
    }

    pub fn current_file(&self) -> Option<&OutputFile> {
        self.output.files.get(self.current_file_idx())
    }

    pub fn current_flat_row(&self) -> Option<&FlatRow> {
        self.flat.get(self.row_idx)
    }

    pub fn row_coord(row: &OutputRow) -> Option<u32> {
        row.new_line.or(row.old_line)
    }

    /// Render full visual screen frame as a String of ANSI terminal lines.
    pub fn render_frame(&mut self, width: usize, height: usize) -> String {
        let max_rows = height.saturating_sub(4).max(4);
        self.viewport_height = max_rows;
        let mut lines = Vec::new();

        let cur_fi = self.current_file_idx();
        let f = match self.output.files.get(cur_fi) {
            Some(file) => file,
            None => {
                return "\x1b[2J\x1b[H No diff to display.".to_string();
            }
        };

        // Header bar
        let counts = &f.counts;
        let note_count = f.notes.len();
        let note_s = format!(
            "{note_count} note{}",
            if note_count == 1 { "" } else { "s" }
        );
        let counts_str = format!(
            "+{}{}{}{} · {}",
            counts.adds,
            if counts.dels > 0 {
                format!(" −{}", counts.dels)
            } else {
                String::new()
            },
            if counts.mods > 0 {
                format!(" ~{}", counts.mods)
            } else {
                String::new()
            },
            if counts.movs > 0 {
                format!(" ≈{} moved", counts.movs)
            } else {
                String::new()
            },
            note_s
        );

        let head_raw = format!(
            " Review: {} · {} ({}) · {}",
            self.repo, f.path, f.status, counts_str
        );
        lines.push(format!(
            "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{:<width$}\x1b[0m",
            head_raw,
            width = width
        ));

        // Secondary status bar
        let total_files = self.output.files.len();
        let cur_hunk = self.current_flat_row().map(|r| r.hunk_idx).unwrap_or(0);
        let total_hunks = f.hunks.len().max(1);
        let go_txt = if !self.num_buffer.is_empty() {
            format!(" · go {}", self.num_buffer)
        } else {
            String::new()
        };
        let col_txt = if self.col_offset > 0 {
            format!(" · col {}+", self.col_offset)
        } else {
            String::new()
        };
        let branch_txt = if !self.output.branch.is_empty() {
            format!(" · branch: {}", self.output.branch)
        } else {
            String::new()
        };
        let status_raw = format!(
            " {}/{} files · hunk {}/{}{}{}{}",
            cur_fi + 1,
            total_files,
            cur_hunk + 1,
            total_hunks,
            col_txt,
            go_txt,
            branch_txt
        );
        lines.push(format!("\x1b[38;5;244m{status_raw}\x1b[39m"));

        // Inline note draft if active
        if let Some(ref draft) = self.note_draft {
            let loc = format!(
                "{}:{}",
                f.path,
                self.current_flat_row()
                    .and_then(|fr| Self::row_coord(&fr.row))
                    .unwrap_or(0)
            );
            lines.push(format!(
                "\x1b[38;5;220m Note on {loc}: {draft}_  [ctrl+enter save · esc cancel]\x1b[39m"
            ));
        }

        // Global flat rows display
        if self.row_idx < self.scroll {
            self.scroll = self.row_idx;
        }
        if self.row_idx >= self.scroll + max_rows {
            self.scroll = self.row_idx.saturating_sub(max_rows - 1);
        }

        let total_rows = self.flat.len();
        let from = self.scroll;
        let to = (self.scroll + max_rows).min(total_rows);

        for idx in from..to {
            let item = &self.flat[idx];
            let is_selected = idx == self.row_idx;
            let r = &item.row;

            if item.is_file_header {
                let file_obj = &self.output.files[item.file_idx];
                let f_counts = &file_obj.counts;
                let bar = format!(
                    "─── {} ({}) +{} -{} ~{} ───",
                    file_obj.path, file_obj.status, f_counts.adds, f_counts.dels, f_counts.mods
                );
                let styled_bar = if is_selected {
                    format!(
                        "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{:<width$}\x1b[0m",
                        bar,
                        width = width
                    )
                } else {
                    format!("\x1b[38;5;244m{bar}\x1b[39m")
                };
                lines.push(styled_bar);
                continue;
            }

            let pfx = styled_prefix(r.kind, r.new_line, r.old_line, r.moved, &r.runs);
            let mut rendered_row = render_row_segment(
                &r.text,
                r.del_text.as_deref(),
                &r.spans,
                r.kind,
                r.moved,
                &r.runs,
                0,
                width.saturating_sub(6),
                &pfx,
                is_selected,
                self.col_offset,
                self.word_diff,
                self.syntax_on,
                &self.theme,
            );
            if is_selected {
                // Ensure the background color remains active into the trailing
                // Erase-to-EOL (resolved through the row-style table).
                let sel_bg = match r.kind {
                    crate::parse::RowKind::Del => S_DEL.cursor_bg(),
                    crate::parse::RowKind::Add if !r.moved => S_ADD.cursor_bg(),
                    _ => S_MOD.cursor_bg(),
                };
                rendered_row.push_str(&sel_bg);
            }
            lines.push(rendered_row);
        }

        // Fill remaining height with empty rows if total rows < max_rows
        while lines.len() < height.saturating_sub(2) {
            lines.push(String::new());
        }

        lines.push(format!(
            "\x1b[38;5;244m {}\x1b[39m",
            "─".repeat(width.saturating_sub(2).min(60))
        ));
        lines.push("\x1b[38;5;244m [j/k]move [n/p]file [J/K]hunk [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help\x1b[39m".to_string());

        // Help modal overlays the frame when toggled on.
        if self.show_help {
            let modal = Self::render_help_modal(width);
            let start = lines.len().saturating_sub(modal.len()) / 2;
            for (i, mline) in modal.iter().enumerate() {
                let idx = start + i;
                if idx < lines.len() {
                    lines[idx] = mline.clone();
                }
            }
        }

        lines.truncate(height);

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(line);
            out.push_str("\x1b[K\x1b[0m");
        }
        out
    }

    /// Render the `?` key-bindings modal as full-width overlay lines.
    fn render_help_modal(width: usize) -> Vec<String> {
        let entries: &[(&str, &str)] = &[
            ("j / k / ↑ / ↓", "move"),
            ("J / K / } / {", "prev / next hunk"),
            ("Space · PgDn/PgUp · Ctrl+d f u b", "page up/down"),
            ("g / G / Home / End", "top / bottom"),
            ("n / p / ] / [", "next / prev file"),
            ("h / l / ← / →", "horizontal scroll"),
            ("w", "toggle word diff"),
            ("y", "toggle syntax highlighting"),
            ("m", "new note (ctrl+enter save, esc cancel)"),
            ("?", "toggle help"),
            ("q / Ctrl+c", "quit"),
        ];
        let mut boxed = vec![String::from(" KEY BINDINGS ")];
        for (key, desc) in entries {
            boxed.push(format!("   {key:<32} {desc}"));
        }
        boxed.push(String::from("   ? or Esc to close"));
        let w = width.saturating_sub(2).min(64);
        boxed
            .into_iter()
            .map(|l| format!("\x1b[48;5;236m\x1b[38;5;255m{:<w$}\x1b[0m", l))
            .collect()
    }

    pub fn move_down(&mut self) {
        if self.row_idx + 1 < self.flat.len() {
            self.row_idx += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.row_idx > 0 {
            self.row_idx -= 1;
        }
    }

    pub fn next_hunk(&mut self) {
        let cur_hunk = self.current_flat_row().map(|r| r.hunk_idx).unwrap_or(0);
        let cur_fi = self.current_file_idx();
        for i in self.row_idx + 1..self.flat.len() {
            if self.flat[i].file_idx != cur_fi || self.flat[i].hunk_idx != cur_hunk {
                self.row_idx = i;
                return;
            }
        }
    }

    pub fn prev_hunk(&mut self) {
        let cur_hunk = self.current_flat_row().map(|r| r.hunk_idx).unwrap_or(0);
        let cur_fi = self.current_file_idx();
        for i in (0..self.row_idx).rev() {
            if self.flat[i].file_idx != cur_fi || self.flat[i].hunk_idx != cur_hunk {
                self.row_idx = i;
                return;
            }
        }
    }

    pub fn next_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in self.row_idx + 1..self.flat.len() {
            if self.flat[i].file_idx > cur_fi {
                self.row_idx = i;
                return;
            }
        }
    }

    pub fn prev_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in (0..self.row_idx).rev() {
            if self.flat[i].file_idx < cur_fi {
                // Find the first row of that previous file
                let prev_fi = self.flat[i].file_idx;
                let first_row = self
                    .flat
                    .iter()
                    .position(|r| r.file_idx == prev_fi)
                    .unwrap_or(i);
                self.row_idx = first_row;
                return;
            }
        }
    }

    pub fn page_down(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let max_idx = self.flat.len() - 1;
        let bottom_of_viewport =
            (self.scroll + self.viewport_height.saturating_sub(1)).min(max_idx);

        // If cursor is not at the bottom of the current visible page, jump to bottom
        if self.row_idx < bottom_of_viewport {
            self.row_idx = bottom_of_viewport;
        } else {
            // Otherwise, scroll down 1/3 of the viewport height with half-up rounding
            let step = ((self.viewport_height * 2 + 3) / 6).max(1);
            self.row_idx = (self.row_idx + step).min(max_idx);
        }
    }

    pub fn page_up(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let top_of_viewport = self.scroll;

        // If cursor is not at the top of the current visible page, jump to top
        if self.row_idx > top_of_viewport {
            self.row_idx = top_of_viewport;
        } else {
            // Otherwise, scroll up 1/3 of the viewport height with half-up rounding
            let step = ((self.viewport_height * 2 + 3) / 6).max(1);
            self.row_idx = self.row_idx.saturating_sub(step);
        }
    }

    pub fn move_top(&mut self) {
        self.row_idx = 0;
        self.scroll = 0;
    }

    pub fn move_bottom(&mut self) {
        if !self.flat.is_empty() {
            self.row_idx = self.flat.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_scroll_through_all_rows() {
        let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
        let contents = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();

        let out = crate::bin_pipeline::run_pipeline_full(
            diff,
            &contents,
            Some(&old_contents),
            None,
            None,
        );
        let mut state = AppState::new(out, "test_repo".into());

        // Step down through every single row and verify render_frame never panics
        for _ in 0..state.flat.len() + 5 {
            let frame = state.render_frame(100, 30);
            assert!(!frame.is_empty());
            state.move_down();
        }

        // Step up through every single row
        for _ in 0..state.flat.len() + 5 {
            let frame = state.render_frame(80, 24);
            assert!(!frame.is_empty());
            state.move_up();
        }
    }

    fn sample_output() -> PipelineOutput {
        let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
      let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
        let contents = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        crate::bin_pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None)
    }

    #[test]
    fn test_help_modal_toggles_and_renders() {
        let mut state = AppState::new(sample_output(), "test_repo".into());

        let plain = state.render_frame(100, 30);
        assert!(
            !plain.contains("KEY BINDINGS"),
            "help modal must be hidden by default"
        );

        state.show_help = true;
        let with_help = state.render_frame(100, 30);
        assert!(
            with_help.contains("KEY BINDINGS"),
            "help modal must render when enabled"
        );
        assert!(with_help.contains("toggle syntax"));
    }
}
