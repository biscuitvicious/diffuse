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
    profile: Option<&LangProfile>,
) -> Vec<FactoredGroup> {
    factor_cluster_with_content(old_rows, new_rows, None, None, profile)
}

/// Factors a change cluster with optional old/new content context.
pub fn factor_cluster_with_content(
    old_rows: &[Row],
    new_rows: &[Row],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<FactoredGroup> {
    let n = old_rows.len();
    let m = new_rows.len();

    if n == 0 && m == 0 {
        return Vec::new();
    }

    if n == 1 && m == 1 && !old_rows[0].is_moved() && !new_rows[0].is_moved() {
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
    let mut exact_matches_u: rustc_hash::FxHashSet<usize> = rustc_hash::FxHashSet::default();
    let mut exact_matches_v: rustc_hash::FxHashSet<usize> = rustc_hash::FxHashSet::default();

    let old_words: Vec<Vec<&str>> = old_rows
        .iter()
        .map(|r| {
            let mut w = significant_words(r.text().trim());
            w.sort_unstable();
            w.dedup();
            w
        })
        .collect();

    let new_words: Vec<Vec<&str>> = new_rows
        .iter()
        .map(|r| {
            let mut w = significant_words(r.text().trim());
            w.sort_unstable();
            w.dedup();
            w
        })
        .collect();

    let mut exact_new_rows: rustc_hash::FxHashMap<&str, smallvec::SmallVec<[usize; 2]>> =
        rustc_hash::FxHashMap::default();

    for (v, new_row) in new_rows.iter().enumerate() {
        if new_row.is_moved() {
            continue;
        }
        let v_trim = new_row.text().trim();
        if !v_trim.is_empty() {
            exact_new_rows.entry(v_trim).or_default().push(v);
        }
    }

    for (u, old_row) in old_rows.iter().enumerate() {
        if old_row.is_moved() {
            continue;
        }
        let u_trim = old_row.text().trim();
        if u_trim.is_empty() {
            continue;
        }
        if let Some(vs) = exact_new_rows.get(u_trim)
            && crate::lcs::line_has_salient_token(u_trim, profile)
        {
            exact_matches_u.insert(u);
            for &v in vs {
                exact_matches_v.insert(v);
            }
        }
    }

    let mut edges = Vec::new();

    if n * m <= 4096 {
        for (u, old_row) in old_rows.iter().enumerate() {
            if old_row.is_moved() || old_row.text().trim().is_empty() {
                continue;
            }
            let u_text = old_row.text();
            let u_trim = u_text.trim();
            for (v, new_row) in new_rows.iter().enumerate() {
                if new_row.is_moved() || new_row.text().trim().is_empty() {
                    continue;
                }
                let v_text = new_row.text();
                let is_exact = u_trim == v_text.trim();
                if is_exact {
                    if has_information_edge_with_words(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    ) {
                        edges.push((u, v));
                    }
                } else if !exact_matches_u.contains(&u)
                    && !exact_matches_v.contains(&v)
                    && has_information_edge_with_words(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    )
                {
                    edges.push((u, v));
                }
            }
        }
    } else {
        for (u, old_row) in old_rows.iter().enumerate() {
            if old_row.is_moved() {
                continue;
            }
            let u_text = old_row.text();
            let u_trim = u_text.trim();
            if u_trim.is_empty() {
                continue;
            }

            let v_min = u.saturating_sub(32);
            let v_max = (u + 33).min(m);

            for v in v_min..v_max {
                let new_row = &new_rows[v];
                if new_row.is_moved() {
                    continue;
                }
                let v_text = new_row.text();
                let v_trim = v_text.trim();
                if v_trim.is_empty() {
                    continue;
                }

                let is_exact = u_trim == v_trim;
                if is_exact {
                    if has_information_edge_with_words(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    ) {
                        edges.push((u, v));
                    }
                } else if !exact_matches_u.contains(&u)
                    && !exact_matches_v.contains(&v)
                    && has_information_edge_with_words(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    )
                {
                    edges.push((u, v));
                }
            }
        }
    }

    // 4. Monotonic cut factor walk
    let mut groups = Vec::new();
    let mut u_start = 0;
    let mut v_start = 0;
    let mut group_id = 0u32;

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

        let first_moved_u = (u_start..n).find(|&u| old_rows[u].is_moved()).unwrap_or(n);
        let first_moved_v = (v_start..m).find(|&v| new_rows[v].is_moved()).unwrap_or(m);
        let max_u = first_moved_u;
        let max_v = first_moved_v;

        // Initialize minimal candidate window
        let mut u_end;
        let mut v_end;

        if u_start >= max_u {
            // Only new rows remaining before anchor/end
            u_end = max_u;
            v_end = (v_start + 1).min(max_v);
        } else if v_start >= max_v {
            // Only old rows remaining before anchor/end
            u_end = (u_start + 1).min(max_u);
            v_end = max_v;
        } else {
            // Check targets of current lines
            let u_target = edges
                .iter()
                .filter(|&&(u, v)| u == u_start && v >= v_start)
                .map(|&(_, v)| v)
                .min();
            let v_target = edges
                .iter()
                .filter(|&&(u, v)| v == v_start && u >= u_start)
                .map(|&(u, _)| u)
                .min();

            let has_direct_edge = u_target == Some(v_start)
                || (!old_rows[u_start].is_moved()
                    && !new_rows[v_start].is_moved()
                    && old_rows[u_start].text().trim() == new_rows[v_start].text().trim());

            if has_direct_edge {
                u_end = (u_start + 1).min(max_u);
                v_end = (v_start + 1).min(max_v);
            } else if u_target.is_some() && v_target.is_none() {
                // Current addition has no edge and precedes the match for current deletion:
                // advance addition so it precedes the deletion's match.
                u_end = u_start;
                v_end = (v_start + 1).min(max_v);
            } else if u_target.is_none() && v_target.is_some() {
                // Current deletion has no edge and precedes the match for current addition:
                // advance deletion so it precedes the addition's match.
                u_end = (u_start + 1).min(max_u);
                v_end = v_start;
            } else {
                // Advance pure deletion first (git diff topological order: dels before adds)
                u_end = (u_start + 1).min(max_u);
                v_end = v_start;
            }
        }

        // 5. Closure loop: expand window until all crossing edges and open containers
        //    are resolved
        loop {
            let mut changed = false;

            // Expand across all intersecting edges (never crossing an is_moved boundary)
            for &(u, v) in &edges {
                if u >= u_start && u < u_end && v >= v_end {
                    let cand_v = (v + 1).min(max_v);
                    if cand_v > v_end
                        && cand_v - v_start <= 16
                        && !(v_end..cand_v).any(|idx| new_rows[idx].is_moved())
                    {
                        v_end = cand_v;
                        changed = true;
                    }
                }
                if v >= v_start && v < v_end && u >= u_end {
                    let cand_u = (u + 1).min(max_u);
                    if cand_u > u_end
                        && cand_u - u_start <= 16
                        && !(u_end..cand_u).any(|idx| old_rows[idx].is_moved())
                    {
                        u_end = cand_u;
                        changed = true;
                    }
                }
            }

            // Expand across unclosed delimiter depth
            let old_depth: i32 = (u_start..u_end).map(|u| old_deltas[u]).sum();
            let new_depth: i32 = (v_start..v_end).map(|v| new_deltas[v]).sum();
            let diff = old_depth - new_depth;

            const MAX_DELIM_EXPAND: usize = 16;
            let can_expand_u = u_end - u_start < MAX_DELIM_EXPAND;
            let can_expand_v = v_end - v_start < MAX_DELIM_EXPAND;

            if can_expand_v
                && u_end > u_start
                && v_end > v_start
                && diff != 0
                && v_end < max_v
                && !new_rows[v_end].is_moved()
                && (old_depth - (new_depth + new_deltas[v_end])).abs() < diff.abs()
            {
                v_end += 1;
                changed = true;
            } else if can_expand_u
                && u_end < max_u
                && !old_rows[u_end].is_moved()
                && ((u_end > u_start
                    && v_end > v_start
                    && diff != 0
                    && ((old_depth + old_deltas[u_end]) - new_depth).abs() < diff.abs())
                    || old_depth > 0)
            {
                u_end += 1;
                changed = true;
            } else if can_expand_v && new_depth > 0 && v_end < max_v && !new_rows[v_end].is_moved()
            {
                v_end += 1;
                changed = true;
            }

            if !changed {
                break;
            }
        }

        assert!(
            u_end > u_start || v_end > v_start,
            "factor_cluster made no forward progress: u_start={u_start}, n={n}, v_start={v_start}, m={m}, max_u={max_u}, max_v={max_v}"
        );
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
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut delta = 0i32;
    let mut in_str = false;
    let mut str_char = b'"';
    let mut escaped = false;
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
            i += 2;
            while i < len && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            continue;
        }
        if in_str {
            if b == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'`' {
            in_str = true;
            str_char = b;
            i += 1;
            continue;
        }
        if b == b'\'' {
            let is_char_lit = (i + 2 < len && bytes[i + 2] == b'\'')
                || (i + 3 < len && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'');
            if is_char_lit {
                in_str = true;
                str_char = b'\'';
            }
            i += 1;
            continue;
        }
        match b {
            b'{' | b'(' | b'[' => delta += 1,
            b'}' | b')' | b']' => delta -= 1,
            _ => {}
        }
        i += 1;
    }
    delta
}

fn has_information_edge_with_words(
    del_text: &str,
    add_text: &str,
    d_words: &[&str],
    a_words: &[&str],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    let d_trim = del_text.trim();
    let a_trim = add_text.trim();
    if d_trim.is_empty() || a_trim.is_empty() {
        return false;
    }
    // 1. Exact trimmed match: form edge for identical code
    if d_trim == a_trim {
        if d_trim == "}" || d_trim == "{" || d_trim == "}," {
            let del_indent = del_text.len() - d_trim.len();
            let add_indent = add_text.len() - a_trim.len();
            return (del_indent as i32 - add_indent as i32).abs() <= 8;
        }
        return true;
    }
    // Fast path: multi-character phrase match where one is a
    // substring/prefix/suffix of the other (>= 8 chars)
    if crate::lcs::is_affix_pair(d_trim, a_trim) {
        let len_diff = (d_trim.len() as i32 - a_trim.len() as i32).abs();
        if len_diff <= 8 {
            return true;
        }
    }
    if (d_trim.len() >= 8 && a_trim.contains(d_trim))
        || (a_trim.len() >= 8 && d_trim.contains(a_trim))
    {
        return true;
    }
    // Compatible declaration roles (e.g. enum <-> struct replacement)
    if let (Some(d_role), Some(a_role)) = (
        crate::collapse::residual::declaration_role(d_trim),
        crate::collapse::residual::declaration_role(a_trim),
    ) && crate::collapse::residual::are_roles_compatible(d_role, a_role)
        && d_trim.ends_with('{') == a_trim.ends_with('{')
        && !crate::collapse::residual::has_verbatim_rival(d_trim, new_content)
        && !crate::collapse::residual::has_verbatim_rival(a_trim, old_content)
    {
        return true;
    }
    // Common assignment target/key or value before/after '=' or ':'
    if let (Some(d_eq), Some(a_eq)) = (d_trim.find('='), a_trim.find('=')) {
        let d_lhs = d_trim[..d_eq].trim();
        let a_lhs = a_trim[..a_eq].trim();
        let d_rhs = d_trim[d_eq + 1..].trim();
        let a_rhs = a_trim[a_eq + 1..].trim();
        if (!d_lhs.is_empty() && d_lhs == a_lhs)
            || (!d_rhs.is_empty()
                && d_rhs == a_rhs
                && crate::lcs::line_has_salient_token(d_rhs, profile))
        {
            return true;
        }
    }
    if let (Some(d_col), Some(a_col)) = (d_trim.find(':'), a_trim.find(':')) {
        let d_key = d_trim[..d_col].trim();
        let a_key = a_trim[..a_col].trim();
        if !d_key.is_empty() && d_key == a_key && !d_key.contains(' ') {
            return true;
        }
    }
    // 2. Significant non-keyword word sharing with non-wholesale context and
    //    salient ratio
    let matched_salient: usize = d_words
        .iter()
        .filter(|dw| {
            a_words.iter().any(|aw| {
                **dw == *aw
                    || (aw.len() >= 4 && dw.split('_').any(|s| s == *aw))
                    || (dw.len() >= 4 && aw.split('_').any(|s| s == **dw))
            })
        })
        .map(|w| w.len())
        .sum();

    if matched_salient > 0 {
        let del_salient: usize = d_words.iter().map(|w| w.len()).sum();
        let add_salient: usize = a_words.iter().map(|w| w.len()).sum();
        let qualifies = if del_salient > 0 && add_salient > 0 {
            let min_salient = del_salient.min(add_salient);
            (matched_salient >= 6 || (matched_salient >= 3 && matched_salient == min_salient))
                && (matched_salient as f64 / min_salient as f64) >= 0.50
        } else {
            true
        };
        if qualifies {
            let spans = crate::lcs::word_spans(del_text, add_text);
            if !crate::merge::is_wholesale_replacement_spans(del_text, add_text, &spans) {
                return true;
            }
        }
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
