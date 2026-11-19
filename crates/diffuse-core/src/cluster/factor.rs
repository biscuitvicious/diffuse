//! Macro cluster factorization into irreducible M-to-N peer reflow groups.

use diffuse_syntax::profile::LangProfile;

use crate::parse::{GroupId, PeerGroupMeta, Row};

/// A factored peer reflow group within a change cluster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoredGroup {
    /// Unique sequential group id within this cluster.
    pub id: GroupId,
    /// Range of old row indices in the cluster: `old_rows[old_range]`.
    pub old_range: std::ops::Range<usize>,
    /// Range of new row indices in the cluster: `new_rows[new_range]`.
    pub new_range: std::ops::Range<usize>,
}

impl FactoredGroup {
    #[inline]
    pub fn old_count(&self) -> usize {
        self.old_range.len()
    }

    #[inline]
    pub fn new_count(&self) -> usize {
        self.new_range.len()
    }

    #[inline]
    pub fn meta(&self) -> PeerGroupMeta {
        PeerGroupMeta::new(self.id, self.old_count(), self.new_count())
    }

    #[inline]
    pub fn is_pure_del(&self) -> bool {
        self.old_count() > 0 && self.new_count() == 0
    }

    #[inline]
    pub fn is_pure_add(&self) -> bool {
        self.old_count() == 0 && self.new_count() > 0
    }

    #[inline]
    pub fn is_single_mod(&self) -> bool {
        self.old_count() == 1 && self.new_count() == 1
    }

    #[inline]
    pub fn is_compound(&self) -> bool {
        self.old_count() > 1 || self.new_count() > 1
    }
}

/// Factors a change cluster of old rows and new rows into irreducible (N -> M)
/// peer groups.
pub fn factor_cluster(
    old_rows: &[Row],
    new_rows: &[Row],
    _profile: Option<&LangProfile>,
) -> Vec<FactoredGroup> {
    let n = old_rows.len();
    let m = new_rows.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }

    if n == 1 && m == 1 {
        let u_text = old_rows[0].text();
        let v_text = new_rows[0].text();
        let spans = crate::lcs::word_spans(u_text, v_text);
        if !crate::merge::is_wholesale_replacement_spans(u_text, v_text, &spans) {
            return vec![FactoredGroup {
                id: GroupId(0),
                old_range: 0..1,
                new_range: 0..1,
            }];
        }
    }

    // 1. Precompute delimiter deltas
    let old_deltas: Vec<i32> = old_rows
        .iter()
        .map(|r| robust_delim_delta(r.text()))
        .collect();
    let new_deltas: Vec<i32> = new_rows
        .iter()
        .map(|r| robust_delim_delta(r.text()))
        .collect();

    // 2. Identify information edges between old and new rows
    let mut edges = Vec::new();
    for (u, old_row) in old_rows.iter().enumerate() {
        if old_row.is_moved() {
            continue;
        }
        let u_text = old_row.text();
        for (v, new_row) in new_rows.iter().enumerate() {
            if new_row.is_moved() {
                continue;
            }
            let v_text = new_row.text();
            if has_information_edge(u_text, v_text) {
                edges.push((u, v));
            }
        }
    }

    // 3. Monotonic cut factor walk
    let mut groups = Vec::new();
    let mut u_start = 0;
    let mut v_start = 0;
    let mut group_id = 0u32;
    let mut has_mod_anchor = false;

    while u_start < n || v_start < m {
        // Pre-pass moved rows are locked relocations and must form isolated pure groups
        if u_start < n && old_rows[u_start].is_moved() {
            let mut u_end = u_start + 1;
            while u_end < n && old_rows[u_end].is_moved() {
                u_end += 1;
            }
            groups.push(FactoredGroup {
                id: GroupId(group_id),
                old_range: u_start..u_end,
                new_range: v_start..v_start,
            });
            group_id += 1;
            u_start = u_end;
            continue;
        }

        if v_start < m && new_rows[v_start].is_moved() {
            let mut v_end = v_start + 1;
            while v_end < m && new_rows[v_end].is_moved() {
                v_end += 1;
            }
            groups.push(FactoredGroup {
                id: GroupId(group_id),
                old_range: u_start..u_start,
                new_range: v_start..v_end,
            });
            group_id += 1;
            v_start = v_end;
            continue;
        }

        if u_start < n
            && v_start < m
            && old_rows[u_start].text().trim().is_empty()
            && new_rows[v_start].text().trim().is_empty()
        {
            groups.push(FactoredGroup {
                id: GroupId(group_id),
                old_range: u_start..u_start + 1,
                new_range: v_start..v_start + 1,
            });
            group_id += 1;
            u_start += 1;
            v_start += 1;
            continue;
        }

        // Initialize minimal candidate window
        let mut u_end;
        let mut v_end;

        if u_start >= n {
            // Only new rows remaining
            u_end = n;
            v_end = v_start + 1;
        } else if v_start >= m {
            // Only old rows remaining
            u_end = u_start + 1;
            v_end = m;
        } else {
            // Check targets of current lines
            let u_target = edges
                .iter()
                .filter(|&&(u, _)| u == u_start)
                .map(|&(_, v)| v)
                .min();
            let v_target = edges
                .iter()
                .filter(|&&(_, v)| v == v_start)
                .map(|&(u, _)| u)
                .min();

            if has_mod_anchor && v_target.is_none() && u_target.is_some_and(|v| v > v_start) {
                // Current addition has no edge and precedes the match for current deletion,
                // and a preceding Mod row already anchors it.
                u_end = u_start;
                v_end = v_start + 1;
            } else if u_target.is_none() && v_target.is_some_and(|u| u > u_start) {
                // Current deletion has no edge and precedes the match for current addition
                u_end = u_start + 1;
                v_end = v_start;
            } else if !has_mod_anchor && v_target.is_none() {
                // Before any Mod anchor in the cluster, an addition with no edge
                // cannot precede deletions. Advance pure deletion first (git diff
                // topological order: dels before adds).
                u_end = u_start + 1;
                v_end = v_start;
            } else if u_target.is_some() || v_target.is_some() {
                u_end = u_start + 1;
                v_end = v_start + 1;
                has_mod_anchor = true;
            } else {
                // Neither current deletion nor current addition has an edge to the other side.
                // Advance pure deletion first (git diff topological order: dels before adds)
                u_end = u_start + 1;
                v_end = v_start;
            }
        }

        // 4. Closure loop: expand window until all crossing edges and open containers
        //    are resolved
        loop {
            let mut changed = false;

            // Expand across all intersecting edges
            for &(u, v) in &edges {
                if u >= u_start && u < u_end && v >= v_end {
                    v_end = (v + 1).min(m);
                    changed = true;
                }
                if v >= v_start && v < v_end && u >= u_end {
                    u_end = (u + 1).min(n);
                    changed = true;
                }
            }

            // Expand across unclosed delimiter depth in new_rows
            if v_end > v_start && v_end < m && !new_rows[v_end].is_moved() {
                let depth: i32 = (v_start..v_end).map(|v| new_deltas[v]).sum();
                if depth > 0 {
                    v_end += 1;
                    changed = true;
                }
            }

            // Expand across unclosed delimiter depth in old_rows
            if u_end > u_start && u_end < n && !old_rows[u_end].is_moved() {
                let depth: i32 = (u_start..u_end).map(|u| old_deltas[u]).sum();
                if depth > 0 {
                    u_end += 1;
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }

        groups.push(FactoredGroup {
            id: GroupId(group_id),
            old_range: u_start..u_end,
            new_range: v_start..v_end,
        });
        group_id += 1;
        u_start = u_end;
        v_start = v_end;
    }

    groups
}

const COMMON_KEYWORDS: &[&str] = &[
    "let", "mut", "const", "static", "pub", "fn", "for", "in", "if", "else", "match", "return",
    "true", "false", "impl", "trait", "struct", "enum", "type", "where", "use", "mod", "crate",
    "self", "super", "async", "await", "while", "loop",
];

fn significant_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|w| {
            w.len() >= 3 && w.chars().any(char::is_alphabetic) && !COMMON_KEYWORDS.contains(w)
        })
        .collect()
}

pub(crate) fn robust_delim_delta(text: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_str = false;
    let mut str_char = '"';
    let mut escaped = false;
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        let ch = chars[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if in_str {
            if ch == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' || ch == '`' {
            in_str = true;
            str_char = ch;
            i += 1;
            continue;
        }
        if ch == '\'' {
            // Check if this is a char literal like 'x' or '\n' vs a lifetime like 'a or
            // 'static
            let is_char_lit = (i + 2 < len && chars[i + 2] == '\'')
                || (i + 3 < len && chars[i + 1] == '\\' && chars[i + 3] == '\'');
            if is_char_lit {
                in_str = true;
                str_char = '\'';
            }
            i += 1;
            continue;
        }
        match ch {
            '{' | '(' | '[' => delta += 1,
            '}' | ')' | ']' => delta -= 1,
            _ => {}
        }
        i += 1;
    }
    delta
}

fn has_information_edge(del_text: &str, add_text: &str) -> bool {
    let d_trim = del_text.trim();
    let a_trim = add_text.trim();
    if d_trim.is_empty() || a_trim.is_empty() {
        return false;
    }
    // 1. Exact trimmed match
    if d_trim == a_trim {
        return true;
    }
    // 2. Significant non-keyword word sharing with non-wholesale context and
    //    salient ratio
    let mut d_words = significant_words(d_trim);
    let mut a_words = significant_words(a_trim);
    d_words.sort_unstable();
    d_words.dedup();
    a_words.sort_unstable();
    a_words.dedup();

    let common_words: Vec<_> = d_words
        .iter()
        .filter(|dw| a_words.iter().any(|aw| aw == *dw))
        .copied()
        .collect();
    if !common_words.is_empty() {
        let spans = crate::lcs::word_spans(del_text, add_text);
        if !crate::merge::is_wholesale_replacement_spans(del_text, add_text, &spans) {
            let del_salient: usize = d_words.iter().map(|w| w.len()).sum();
            let add_salient: usize = a_words.iter().map(|w| w.len()).sum();
            if del_salient > 0 && add_salient > 0 {
                let matched_salient: usize = common_words.iter().map(|w| w.len()).sum();
                let min_salient = del_salient.min(add_salient);
                if matched_salient >= 4 && (matched_salient as f64 / min_salient as f64) >= 0.30 {
                    return true;
                }
            } else {
                return true;
            }
        }
    }
    // 3. Multi-character phrase match: if a contiguous phrase of >= 8
    //    non-whitespace chars matches
    if d_trim.len() >= 8 && a_trim.contains(d_trim) {
        return true;
    }
    if a_trim.len() >= 8 && d_trim.contains(a_trim) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    fn make_del(num: u32, text: &str) -> Row {
        Row::Del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::Add(AddLine::new(num, text))
    }

    #[test]
    fn test_factor_pure_deletion() {
        let dels = vec![make_del(10, "let x = 1;")];
        let adds = vec![];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 1);
        assert_eq!(groups[0].new_count(), 0);
        assert!(groups[0].is_pure_del());
    }

    #[test]
    fn test_factor_pure_addition() {
        let dels = vec![];
        let adds = vec![make_add(10, "let y = 2;")];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 0);
        assert_eq!(groups[0].new_count(), 1);
        assert!(groups[0].is_pure_add());
    }

    #[test]
    fn test_factor_independent_mods() {
        let dels = vec![
            make_del(10, "let alpha = 100;"),
            make_del(11, "let beta = 200;"),
        ];
        let adds = vec![
            make_add(10, "let alpha = 101;"),
            make_add(11, "let beta = 201;"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].old_range, 0..1);
        assert_eq!(groups[0].new_range, 0..1);
        assert!(groups[0].is_single_mod());

        assert_eq!(groups[1].old_range, 1..2);
        assert_eq!(groups[1].new_range, 1..2);
        assert!(groups[1].is_single_mod());
    }

    #[test]
    fn test_factor_expansion_block() {
        let dels = vec![make_del(20, "fn run() -> bool { false }")];
        let adds = vec![
            make_add(20, "fn run() -> bool {"),
            make_add(21, "    let active = true;"),
            make_add(22, "    active"),
            make_add(23, "}"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 1);
        assert_eq!(groups[0].new_count(), 4);
        assert!(groups[0].is_compound());
    }

    #[test]
    fn test_factor_collapse_block() {
        let dels = vec![
            make_del(30, "fn stop() {"),
            make_del(31, "    cleanup();"),
            make_del(32, "}"),
        ];
        let adds = vec![make_add(30, "fn stop() { cleanup(); }")];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 3);
        assert_eq!(groups[0].new_count(), 1);
        assert!(groups[0].is_compound());
    }

    #[test]
    fn test_factor_compound_scrollbar_reflow() {
        let dels = vec![
            make_del(293, "find_entry(pat: &str) -> Option<usize>"),
            make_del(
                294,
                "|pat: &str| self.entries.iter().position(|e| e.name == pat);",
            ),
        ];
        let adds = vec![
            make_add(
                288,
                "find_entry<'a>(&'a self, pat: &str) -> Option<usize> {",
            ),
            make_add(289, "    self.entries.iter().position(|e| e.name == pat)"),
            make_add(290, "};"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 2);
        assert_eq!(groups[0].new_count(), 3);
        assert_eq!(groups[0].old_range, 0..2);
        assert_eq!(groups[0].new_range, 0..3);
    }

    #[test]
    fn test_factor_mixed_mod_add_mod() {
        let dels = vec![
            make_del(10, "let item_a = compute_a();"),
            make_del(12, "let item_c = compute_c();"),
        ];
        let adds = vec![
            make_add(10, "let item_a = compute_a_fast();"),
            make_add(11, "let item_b = compute_b();"),
            make_add(12, "let item_c = compute_c_fast();"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].old_range, 0..1);
        assert_eq!(groups[0].new_range, 0..1);
        assert!(groups[0].is_single_mod());

        assert_eq!(groups[1].old_range, 1..1);
        assert_eq!(groups[1].new_range, 1..2);
        assert!(groups[1].is_pure_add());

        assert_eq!(groups[2].old_range, 1..2);
        assert_eq!(groups[2].new_range, 2..3);
        assert!(groups[2].is_single_mod());
    }

    #[test]
    fn test_factor_unbalanced_delimiters() {
        let dels = vec![make_del(1, "macro_rules! my_macro {")];
        let adds = vec![
            make_add(1, "macro_rules! my_macro {"),
            make_add(2, "    ($x:expr) => {"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        // Does not panic or hang; terminates cleanly
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].old_count(), 1);
        assert_eq!(groups[0].new_count(), 2);
    }

    #[test]
    fn test_token_cluster_debug() {
        let dels = vec![
            make_del(
                681,
                "    let mut result = Vec::with_capacity(tokens.len());",
            ),
            make_del(
                682,
                "    let mut stack: diffuse_algo::SmallStack<ContainerKind, 8> = diffuse_algo::SmallStack::new();",
            ),
            make_del(683, ""),
            make_del(684, "    for &t in tokens {"),
            make_del(685, "        match t {"),
            make_del(686, "            \"`\" => {"),
            make_del(
                687,
                "                if stack.last() == Some(&ContainerKind::Backtick) {",
            ),
            make_del(688, "                    let top = stack.pop();"),
            make_del(689, "                    result.push(top);"),
            make_del(690, "                } else {"),
            make_del(
                691,
                "                    stack.push(ContainerKind::Backtick);",
            ),
            make_del(
                692,
                "                    result.push(stack.last().copied());",
            ),
            make_del(693, "                }"),
            make_del(694, "            }"),
            make_del(695, "            \"\\\"\" => {"),
            make_del(
                696,
                "                if stack.last() == Some(&ContainerKind::DoubleQuote) {",
            ),
            make_del(697, "                    let top = stack.pop();"),
            make_del(698, "                    result.push(top);"),
            make_del(699, "                } else {"),
            make_del(
                700,
                "                    stack.push(ContainerKind::DoubleQuote);",
            ),
            make_del(
                701,
                "                    result.push(stack.last().copied());",
            ),
            make_del(702, "                }"),
            make_del(703, "            }"),
            make_del(704, "            _ => {"),
            make_del(705, "                result.push(stack.last().copied());"),
            make_del(706, "            }"),
            make_del(707, "        }"),
            make_del(708, "    }"),
            make_del(709, "    result"),
        ];
        let adds = vec![
            make_add(
                720,
                "    diffuse_algo::track_enclosing_containers(tokens, &diffuse_algo::AsciiDelimiterClassifier)",
            ),
            make_add(721, "        .into_iter()"),
            make_add(
                722,
                "        .map(|opt| opt.and_then(ContainerKind::from_id))",
            ),
            make_add(723, "        .collect()"),
        ];
        let groups = factor_cluster(&dels, &adds, None);
        assert!(!groups.is_empty());
        let total_old: usize = groups.iter().map(|g| g.old_count()).sum();
        let total_new: usize = groups.iter().map(|g| g.new_count()).sum();
        assert_eq!(total_old, 29);
        assert_eq!(total_new, 4);
    }
}
