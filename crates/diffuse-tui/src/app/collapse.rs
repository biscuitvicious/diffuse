//! Collapse — file collapse toggling.

use super::AppState;

impl AppState {
    pub fn toggle_file_collapse(&mut self, fi: usize) {
        if let Some(f) = self.output.files.get(fi)
            && f.is_binary
        {
            return;
        }
        if self.collapsed_files.contains(&fi) {
            self.collapsed_files.remove(&fi);
            if !self.is_file_loaded(fi) {
                self.request_file_load(fi);
            }
        } else {
            self.collapsed_files.insert(fi);
            if let Some(f) = self.output.files.get_mut(fi) {
                f.release_rendered_memory();
            }
        }
        self.rebuild_flat_preserving_cursor(fi);
    }

    pub fn toggle_current_file_collapse(&mut self) {
        let fi = self.current_file_idx();
        self.toggle_file_collapse(fi);
    }
}

#[cfg(test)]
mod tests;
