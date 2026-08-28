//! Finder integration — file finder dialog state.

use super::AppState;

impl AppState {
    pub fn open_file_finder(&mut self) {
        let mut finder =
            crate::finder::FileFinderState::new(&self.output.files, self.row_idx, self.scroll);
        // Default selected match to the current file
        let cur_fi = self.current_file_idx();
        if let Some(pos) = finder.matches.iter().position(|m| m.file_idx == cur_fi) {
            finder.selected_idx = pos;
        }
        self.file_finder = Some(finder);
    }

    pub fn cancel_file_finder(&mut self) {
        self.file_finder = None;
    }

    pub fn confirm_file_finder(&mut self) {
        if let Some(finder) = self.file_finder.take()
            && let Some(fi) = finder.selected_file_idx()
        {
            self.goto_file_idx(fi);
        }
    }
}
