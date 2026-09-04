use crate::core::wire::OutputFile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatch {
    pub file_idx: usize,
    pub path: String,
    pub score: i64,
    pub match_indices: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct FileFinderState {
    pub query: String,
    pub selected_idx: usize,
    pub initial_row_idx: usize,
    pub initial_scroll: usize,
    pub matches: Vec<FileMatch>,
}

impl FileFinderState {
    pub fn new(files: &[OutputFile], initial_row_idx: usize, initial_scroll: usize) -> Self {
        let mut finder = Self {
            query: String::new(),
            selected_idx: 0,
            initial_row_idx,
            initial_scroll,
            matches: Vec::new(),
        };
        finder.update_matches(files);
        finder
    }

    pub fn update_matches(&mut self, files: &[OutputFile]) {
        let q = self.query.trim().to_lowercase();
        let mut results = Vec::new();

        for (idx, f) in files.iter().enumerate() {
            if q.is_empty() {
                results.push(FileMatch {
                    file_idx: idx,
                    path: f.path.clone(),
                    score: 0,
                    match_indices: Vec::new(),
                });
                continue;
            }

            if let Some((score, indices)) = score_match(&f.path, &q) {
                results.push(FileMatch {
                    file_idx: idx,
                    path: f.path.clone(),
                    score,
                    match_indices: indices,
                });
            }
        }

        if !q.is_empty() {
            results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
        }

        self.matches = results;
        if self.selected_idx >= self.matches.len() {
            self.selected_idx = 0;
        }
    }

    pub fn selected_file_idx(&self) -> Option<usize> {
        self.matches.get(self.selected_idx).map(|m| m.file_idx)
    }

    pub fn move_down(&mut self) {
        if !self.matches.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.matches.len();
        }
    }

    pub fn move_up(&mut self) {
        if !self.matches.is_empty() {
            if self.selected_idx == 0 {
                self.selected_idx = self.matches.len() - 1;
            } else {
                self.selected_idx -= 1;
            }
        }
    }
}

/// Score a candidate path against query fragments.
/// Higher score means a better match.
pub fn score_match(path: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    let lower_path = path.to_lowercase();
    let fragments: Vec<&str> = query.split_whitespace().collect();
    if fragments.is_empty() {
        return Some((0, Vec::new()));
    }

    let mut total_score = 0;
    let mut all_indices = Vec::new();

    let basename = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let basename_offset = path.len().saturating_sub(basename.len());

    for frag in fragments {
        if let Some(pos) = basename.find(frag) {
            total_score += 100;
            if pos == 0 {
                total_score += 50; // Prefix bonus
            }
            let start = basename_offset + pos;
            all_indices.extend(start..start + frag.len());
        } else if let Some(pos) = lower_path.find(frag) {
            total_score += 40;
            if pos == 0 {
                total_score += 20;
            }
            all_indices.extend(pos..pos + frag.len());
        } else if let Some((fuzzy_score, indices)) = fuzzy_subsequence(&lower_path, frag) {
            total_score += fuzzy_score;
            all_indices.extend(indices);
        } else {
            return None; // All fragments must match
        }
    }

    all_indices.sort_unstable();
    all_indices.dedup();

    Some((total_score, all_indices))
}

fn fuzzy_subsequence(text: &str, pattern: &str) -> Option<(i64, Vec<usize>)> {
    let mut text_chars = text.char_indices();
    let mut indices = Vec::new();
    let mut score = 10;

    for pch in pattern.chars() {
        let mut found = false;
        for (idx, tch) in text_chars.by_ref() {
            if tch == pch {
                indices.push(idx);
                score += 5;
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }

    Some((score, indices))
}

#[cfg(test)]
mod tests;
