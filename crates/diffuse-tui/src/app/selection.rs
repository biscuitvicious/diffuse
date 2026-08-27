//! Selection — anchor, confirmed selection and band computation.

use crate::types::Selection;

use super::AppState;

impl AppState {
    /// Line-coordinate selection over the current file, mirroring the
    /// reference `computeSelection`: anchor..cursor snapped to coordinate
    /// rows and clamped to the file. With no active anchor, the confirmed
    /// selection is returned when it lives in the current file.
    pub fn compute_selection(&self) -> Option<Selection> {
        let Some(anchor) = self.anchor else {
            return self
                .confirmed_sel
                .filter(|s| s.file_idx == self.current_file_idx());
        };
        let c_file = self.current_file_idx();
        let mut lo = anchor.min(self.row_idx);
        let mut hi = anchor.max(self.row_idx);
        if self.flat.get(anchor).map(|r| r.file_idx) != Some(c_file) {
            lo = self.first_row_of_file(c_file);
        }
        let fs = self.first_row_of_file(c_file);
        let fe = self.last_row_of_file(c_file);
        lo = lo.min(fe).max(fs);
        hi = hi.min(fe).max(fs);
        if hi < lo {
            hi = lo;
        }
        lo = self.next_coord_row(lo);
        hi = self.prev_coord_row(hi);
        let first = self.flat.get(lo)?;
        let last = self.flat.get(hi)?;
        let r_first = self.row_of(first)?;
        let r_last = self.row_of(last)?;
        let side_is_new = r_first.new_line.is_some() || r_last.new_line.is_some();
        let f = Self::row_coord(r_first)?;
        let l = Self::row_coord(r_last)?;
        Some(Selection {
            file_idx: c_file,
            start: f.min(l),
            end: f.max(l),
            side_is_new,
        })
    }

    /// `v`/`V`: start the anchor at the cursor, or confirm the range into a
    /// selection. Mirror of the reference `v` handler.
    pub fn start_or_confirm_selection(&mut self) {
        match self.anchor {
            None => {
                self.anchor = Some(self.row_idx);
                self.sel_pivot = Some(self.row_idx);
                self.confirmed_sel = None;
            }
            Some(_) => {
                self.confirmed_sel = self.compute_selection();
                self.anchor = None;
            }
        }
    }

    /// Set of flat rows inside the visual selection band (anchor active or a
    /// confirmed selection), mirroring `component_render`'s band computation.
    pub(crate) fn band_rows(&self) -> Vec<bool> {
        let mut set = vec![false; self.flat.len()];
        if let Some(anchor) = self.anchor {
            let c_file = self.current_file_idx();
            let lo = anchor.min(self.row_idx);
            let hi = anchor.max(self.row_idx);
            let lo = if self.flat.get(anchor).map(|r| r.file_idx) != Some(c_file) {
                self.first_row_of_file(c_file)
            } else {
                lo
            };
            let fs = self.first_row_of_file(c_file);
            let fe = self.last_row_of_file(c_file);
            for i in fs.max(lo)..=fe.min(hi) {
                if i < set.len() {
                    set[i] = true;
                }
            }
        } else if let Some(conf) = &self.confirmed_sel
            && conf.file_idx == self.current_file_idx()
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != conf.file_idx {
                    continue;
                }
                if let Some(c) = self.flat_row_coord(item)
                    && c >= conf.start
                    && c <= conf.end
                {
                    set[i] = true;
                }
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {

    use crate::app::AppState;

    fn sample_output() -> diffuse_core::wire::PipelineOutput {
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
        let new_content = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_content = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        diffuse_core::pipeline::run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            diffuse_core::PipelineOptions::default(),
        )
    }

    #[test]
    fn test_selection_anchor_span_snaps_to_coord_rows() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        // flat[0] is the file header (no coordinate); AppState starts on the
        // first coordinate row (1020). The engine pads the hunk with empty
        // above-context rows (1022-1024), then the real rows: ctx
        // " let x = 1;" (1025), the mod pair (1026), the add (1027). Walk to
        // the first real row, anchor, extend over the mod.
        for _ in 0..3 {
            state.move_down();
        }
        state.start_or_confirm_selection();
        state.move_down();
        state.move_down();

        let sel = state
            .compute_selection()
            .expect("anchor over coordinate rows must yield a selection");
        assert_eq!(sel.file_idx, 0);
        assert!(sel.start < sel.end, "range must span two rows");
        assert_eq!(sel.start, 1025);
        assert_eq!(sel.end, 1027);
        assert!(
            sel.side_is_new,
            "a range led by a new-side row is a new-side selection"
        );
    }

    #[test]
    fn test_selection_v_confirm_clears_anchor_and_band_renders() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        for _ in 0..3 {
            state.move_down();
        }
        state.start_or_confirm_selection();
        state.move_down();
        state.move_down();

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("\x1b[48;5;240m"),
            "an active anchor must band the rows between anchor and cursor"
        );

        state.start_or_confirm_selection();
        assert!(state.anchor.is_none(), "confirming clears the anchor");
        let sel = state
            .confirmed_sel
            .expect("confirm must record a selection");
        assert_eq!(sel.start, 1025);
        assert_eq!(sel.end, 1027);

        let frame2 = state.render_frame(100, 30);
        assert!(
            frame2.contains("\x1b[48;5;240m"),
            "a confirmed selection must keep banding its coordinate range"
        );
    }

    #[test]
    fn test_selection_requires_coord_rows() {
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        // Anchor on the file header (no coordinate) and confirm in place: the
        // range collapses onto the header which carries no line number, so
        // the reference computes no selection.
        state.move_up();
        state.start_or_confirm_selection();
        state.start_or_confirm_selection();
        assert!(state.anchor.is_none());
        assert!(state.confirmed_sel.is_none(), "header has no coordinate");

        // Anchoring on the header then walking onto a coordinate row snaps
        // the low end down to the first coordinate row of the file.
        let mut state = AppState::new(
            sample_output(),
            "test_repo".into(),
            std::env::temp_dir().join("diffuse-test-notes.json"),
        );
        state.move_up();
        state.start_or_confirm_selection();
        state.move_down();
        let sel = state
            .compute_selection()
            .expect("anchor over the header must snap to a coordinate row");
        assert_eq!(sel.start, 1022, "snap to the first padded ctx row");
        assert_eq!(sel.end, 1022);
    }
}
