//! Macro cluster factorization into irreducible M-to-N peer reflow groups.

use diffuse_algo::{DelimiterCategory, UniversalDelimiterClassifier};
use diffuse_syntax::profile::LangProfile;

use crate::parse::{GroupId, Row};

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
            let mut w = significant_words(r.text().trim(), profile);
            w.sort_unstable();
            w.dedup();
            w
        })
        .collect();

    let new_words: Vec<Vec<&str>> = new_rows
        .iter()
        .map(|r| {
            let mut w = significant_words(r.text().trim(), profile);
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

    let mut raw_edges: Vec<(usize, usize, f64)> = Vec::new();

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
                    if let Some(w) = calculate_edge_explanation_ratio(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    ) {
                        raw_edges.push((u, v, w));
                    }
                } else if !exact_matches_u.contains(&u)
                    && !exact_matches_v.contains(&v)
                    && let Some(w) = calculate_edge_explanation_ratio(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    )
                {
                    raw_edges.push((u, v, w));
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
                    if let Some(w) = calculate_edge_explanation_ratio(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    ) {
                        raw_edges.push((u, v, w));
                    }
                } else if !exact_matches_u.contains(&u)
                    && !exact_matches_v.contains(&v)
                    && let Some(w) = calculate_edge_explanation_ratio(
                        u_text,
                        v_text,
                        &old_words[u],
                        &new_words[v],
                        old_content,
                        new_content,
                        profile,
                    )
                {
                    raw_edges.push((u, v, w));
                }
            }
        }
    }

    // Identify exact multi-word landmarks that retain transport power across
    // distance
    let is_exact_landmark = |u: usize, v: usize| -> bool {
        let u_trim = old_rows[u].text().trim();
        let v_trim = new_rows[v].text().trim();
        if u_trim == v_trim {
            return true;
        }
        if u_trim.len() >= 8
            && (v_trim.starts_with(u_trim)
                || v_trim.ends_with(u_trim)
                || u_trim.starts_with(v_trim)
                || u_trim.ends_with(v_trim))
        {
            return true;
        }
        if let (Some(d_eq), Some(a_eq)) = (u_trim.find('='), v_trim.find('=')) {
            let d_lhs = u_trim[..d_eq].trim();
            let a_lhs = v_trim[..a_eq].trim();
            if !d_lhs.is_empty() && d_lhs == a_lhs {
                return true;
            }
        }
        let matched: usize = old_words[u]
            .iter()
            .filter(|dw| new_words[v].iter().any(|aw| is_salient_word_match(dw, aw)))
            .map(|w| w.len())
            .sum();
        let match_count = old_words[u]
            .iter()
            .filter(|dw| new_words[v].iter().any(|aw| is_salient_word_match(dw, aw)))
            .count();
        (match_count >= 2 && matched >= 12) || matched >= 16
    };

    // Step 2: Distance-penalized edge thresholding
    let mut distance_pruned = Vec::with_capacity(raw_edges.len());
    for &(u, v, w) in &raw_edges {
        let delta = (u as isize - v as isize).unsigned_abs();
        let tau_min = if delta <= 1 || is_exact_landmark(u, v) {
            0.25
        } else {
            (0.25 + 0.14 * ((1 + delta) as f64).ln()).min(0.65)
        };
        if w >= tau_min {
            distance_pruned.push((u, v, w));
        }
    }

    // Step 3: Competing edge suppression
    // Where a line has competing edges separated by a gap (not part of a contiguous
    // expansion or collapse block), suppress weak secondary edges whose weight
    // is less than 0.5 * the dominant edge.
    let mut max_u_weight = vec![0.0f64; n];
    let mut max_v_weight = vec![0.0f64; m];
    for &(u, v, w) in &distance_pruned {
        if w > max_u_weight[u] {
            max_u_weight[u] = w;
        }
        if w > max_v_weight[v] {
            max_v_weight[v] = w;
        }
    }

    let edges: Vec<(usize, usize)> = distance_pruned
        .iter()
        .filter(|&&(u, v, w)| {
            // Check if (u, v) is a secondary edge for u (< 0.5 * dominant)
            let is_weak_u = w < 0.50 * max_u_weight[u];
            if is_weak_u {
                // If v has an immediate neighbor candidate for u (|v - v'| <= 1),
                // it is part of a contiguous expansion block and not competing.
                let has_neighbor = distance_pruned.iter().any(|&(u_other, v_other, _)| {
                    u_other == u && (v as isize - v_other as isize).abs() == 1
                });
                if !has_neighbor {
                    return false;
                }
            }

            // Check if (u, v) is a secondary edge for v (< 0.5 * dominant)
            let is_weak_v = w < 0.50 * max_v_weight[v];
            if is_weak_v {
                // If u has an immediate neighbor candidate for v (|u - u'| <= 1),
                // it is part of a contiguous collapse block and not competing.
                let has_neighbor = distance_pruned.iter().any(|&(u_other, v_other, _)| {
                    v_other == v && (u as isize - u_other as isize).abs() == 1
                });
                if !has_neighbor {
                    return false;
                }
            }

            true
        })
        .map(|&(u, v, _)| (u, v))
        .collect();

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

            if u_end < max_u
                && v_end < max_v
                && old_rows[u_end].text().trim().is_empty()
                && new_rows[v_end].text().trim().is_empty()
            {
                break;
            }

            let v_reduces_delta = can_expand_v
                && u_end > u_start
                && v_end > v_start
                && diff != 0
                && v_end < max_v
                && !new_rows[v_end].is_moved()
                && (old_depth - (new_depth + new_deltas[v_end])).abs() < diff.abs();

            let u_reduces_delta = can_expand_u
                && u_end > u_start
                && v_end > v_start
                && diff != 0
                && u_end < max_u
                && !old_rows[u_end].is_moved()
                && ((old_depth + old_deltas[u_end]) - new_depth).abs() < diff.abs();

            let u_catches_up = diff > 0
                && old_depth > 0
                && can_expand_u
                && u_end < max_u
                && !old_rows[u_end].is_moved()
                && !old_rows[u_end].text().trim().is_empty();

            let v_catches_up = diff < 0
                && new_depth > 0
                && can_expand_v
                && v_end < max_v
                && !new_rows[v_end].is_moved()
                && !new_rows[v_end].text().trim().is_empty();

            if v_reduces_delta {
                v_end += 1;
                changed = true;
            } else if u_reduces_delta || u_catches_up {
                u_end += 1;
                changed = true;
            } else if v_catches_up {
                v_end += 1;
                changed = true;
            } else if diff == 0
                && old_depth > 0
                && new_depth > 0
                && can_expand_u
                && can_expand_v
                && u_end < max_u
                && v_end < max_v
                && !old_rows[u_end].is_moved()
                && !new_rows[v_end].is_moved()
                && !is_comment_line(old_rows[u_end].text(), profile)
                && !is_comment_line(new_rows[v_end].text(), profile)
            {
                u_end += 1;
                v_end += 1;
                changed = true;
            }

            // Expand across open continuation statements (e.g. ending in '=')
            // only when factoring a compound group (both old and new present)
            let u_is_open = can_expand_u
                && u_end > u_start
                && v_end > v_start
                && u_end < max_u
                && !old_rows[u_end].is_moved()
                && is_open_continuation_line(old_rows[u_end - 1].text());

            let v_is_open = can_expand_v
                && u_end > u_start
                && v_end > v_start
                && v_end < max_v
                && !new_rows[v_end].is_moved()
                && is_open_continuation_line(new_rows[v_end - 1].text());

            if u_is_open {
                u_end += 1;
                changed = true;
            }
            if v_is_open {
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
    "as", "let", "mut", "const", "static", "pub", "fn", "for", "in", "if", "else", "match",
    "return", "true", "false", "impl", "trait", "struct", "enum", "type", "where", "use", "mod",
    "crate", "self", "super", "async", "await", "while", "loop",
];

fn significant_words<'a>(text: &'a str, profile: Option<&LangProfile>) -> Vec<&'a str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|w| {
            w.len() >= 3
                && w.chars().any(char::is_alphabetic)
                && !COMMON_KEYWORDS.contains(w)
                && !profile.is_some_and(|p| p.is_non_salient(w))
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
        match UniversalDelimiterClassifier.classify_char(b as char) {
            Some(DelimiterCategory::Open(_)) => delta += 1,
            Some(DelimiterCategory::Close(_)) => delta -= 1,
            _ => {}
        }
        i += 1;
    }
    delta
}

#[inline]
fn is_salient_word_match(w1: &str, w2: &str) -> bool {
    w1 == w2
        || (w2.len() >= 4 && w1.split('_').any(|s| s == w2))
        || (w1.len() >= 4 && w2.split('_').any(|s| s == w1))
}

#[inline]
fn is_open_continuation_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    (t.ends_with('=')
        && !t.ends_with("==")
        && !t.ends_with("!=")
        && !t.ends_with("<=")
        && !t.ends_with(">="))
        || t.ends_with("=>")
        || t.ends_with("->")
}

fn is_comment_line(line: &str, profile: Option<&LangProfile>) -> bool {
    profile.is_some_and(|p| p.is_comment_line(line))
}

fn calculate_edge_explanation_ratio(
    del_text: &str,
    add_text: &str,
    d_words: &[&str],
    a_words: &[&str],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Option<f64> {
    let d_trim = del_text.trim();
    let a_trim = add_text.trim();
    if d_trim.is_empty() || a_trim.is_empty() {
        return None;
    }
    let d_is_comment = is_comment_line(del_text, profile);
    let a_is_comment = is_comment_line(add_text, profile);
    if d_is_comment != a_is_comment {
        return None;
    }
    // 1. Exact trimmed match: form edge for identical code
    if d_trim == a_trim {
        if d_trim == "}" || d_trim == "{" || d_trim == "}," {
            let del_indent = del_text.len() - d_trim.len();
            let add_indent = add_text.len() - a_trim.len();
            if (del_indent as i32 - add_indent as i32).abs() <= 8 {
                return Some(1.0);
            } else {
                return None;
            }
        }
        return Some(1.0);
    }
    // Fast path: multi-character phrase match where one is a
    // prefix/suffix (>= 8 chars) or significant substring of the other
    let is_affix = a_trim.starts_with(d_trim)
        || a_trim.ends_with(d_trim)
        || d_trim.starts_with(a_trim)
        || d_trim.ends_with(a_trim);
    let min_len = d_trim.len().min(a_trim.len());
    let max_len = d_trim.len().max(a_trim.len());
    if min_len >= 8 && is_affix {
        let ratio = min_len as f64 / max_len as f64;
        return Some(ratio.max(0.70));
    }
    if (d_trim.len() >= 8 && a_trim.contains(d_trim))
        || (a_trim.len() >= 8 && d_trim.contains(a_trim))
    {
        let substr_ratio = min_len as f64 / max_len as f64;
        if substr_ratio >= 0.20 || min_len >= 16 {
            return Some(substr_ratio.max(0.40));
        }
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
        return Some(1.0);
    }
    // Common assignment target/key or value before/after '=' or ':'
    if let (Some(d_eq), Some(a_eq)) = (d_trim.find('='), a_trim.find('=')) {
        let d_lhs = d_trim[..d_eq].trim();
        let a_lhs = a_trim[..a_eq].trim();
        let d_rhs = d_trim[d_eq + 1..].trim();
        let a_rhs = a_trim[a_eq + 1..].trim();
        if !d_lhs.is_empty() && d_lhs == a_lhs {
            let lhs_ratio = d_lhs.len() as f64 / d_trim.len().max(a_trim.len()) as f64;
            return Some(lhs_ratio.max(0.70));
        }
        if !d_rhs.is_empty() && d_rhs == a_rhs && crate::lcs::line_has_salient_token(d_rhs, profile)
        {
            let rhs_ratio = d_rhs.len() as f64 / d_trim.len().max(a_trim.len()) as f64;
            return Some(rhs_ratio.max(0.70));
        }
    }
    if let (Some(d_col), Some(a_col)) = (d_trim.find(':'), a_trim.find(':')) {
        let d_key = d_trim[..d_col].trim();
        let a_key = a_trim[..a_col].trim();
        if !d_key.is_empty() && d_key == a_key && !d_key.contains(' ') {
            return Some(0.80);
        }
    }
    // 2. Significant non-keyword word sharing with non-wholesale context and
    //    salient ratio
    let matched_salient: usize = d_words
        .iter()
        .filter(|dw| a_words.iter().any(|aw| is_salient_word_match(dw, aw)))
        .map(|w| w.len())
        .sum();

    if matched_salient > 0 {
        let del_salient: usize = d_words.iter().map(|w| w.len()).sum();
        let add_salient: usize = a_words.iter().map(|w| w.len()).sum();
        if del_salient > 0 && add_salient > 0 {
            let del_ratio = matched_salient as f64 / del_salient as f64;
            let add_ratio = matched_salient as f64 / add_salient as f64;

            // Bilateral coverage noise floor:
            // If the matched salient tokens explain less than 20% of either side,
            // reject the edge unless it is a high-entropy compound identifier (>= 16
            // chars).
            if (del_ratio < 0.20 || add_ratio < 0.20) && matched_salient < 16 {
                return None;
            }

            let min_salient = del_salient.min(add_salient);
            let has_min_salient =
                matched_salient >= 6 || (matched_salient >= 3 && matched_salient == min_salient);
            let is_full_containment = matched_salient == min_salient;
            let is_bilateral_balance = del_ratio >= 0.40 && add_ratio >= 0.40;
            let is_dominant_coverage =
                (del_ratio >= 0.65 || add_ratio >= 0.65) && del_ratio >= 0.25 && add_ratio >= 0.25;

            let qualifies = has_min_salient
                && (is_full_containment || is_bilateral_balance || is_dominant_coverage);

            if qualifies {
                let spans = crate::lcs::word_spans(del_text, add_text);
                if !crate::merge::is_wholesale_replacement_spans(del_text, add_text, &spans) {
                    let jaccard = matched_salient as f64
                        / (del_salient + add_salient - matched_salient) as f64;
                    return Some(jaccard);
                }
            }
        }
    }
    None
}

#[cfg(test)]
#[inline]
pub(crate) fn has_information_edge_with_words(
    del_text: &str,
    add_text: &str,
    d_words: &[&str],
    a_words: &[&str],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    calculate_edge_explanation_ratio(
        del_text,
        add_text,
        d_words,
        a_words,
        old_content,
        new_content,
        profile,
    )
    .is_some_and(|w| w >= 0.20)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    fn make_del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    #[test]
    fn test_scrollbar_reflow_does_not_group_unrelated_body() {
        let dels = vec![
            make_del(227, "                let scrollbar_seek = is_on_scrollbar;"),
            make_del(228, "                if scrollbar_seek {"),
            make_del(
                229,
                "                    // Click and drag share one exact mapping from pointer",
            ),
            make_del(230, "                    // row to scroll position."),
            make_del(231, "                    //"),
            make_del(
                232,
                "                    // The scrollbar widget spans the full terminal height",
            ),
            make_del(
                233,
                "                    // (screen_height), but the viewport it represents is",
            ),
            make_del(
                234,
                "                    // only `vh` rows tall. The mapping must therefore use",
            ),
            make_del(
                235,
                "                    // the viewport-relative row and the viewport height so",
            ),
            make_del(
                236,
                "                    // the pointer position on the track corresponds to the",
            ),
            make_del(
                237,
                "                    // visible content area, not the full screen.",
            ),
            make_del(
                238,
                "                    self.is_dragging_scrollbar = true;",
            ),
            make_del(
                239,
                "                    let (_starts, _counts, _seg, total_vis) =",
            ),
            make_del(
                240,
                "                        self.cached_geometry_tuple(self.last_content_width);",
            ),
            make_del(
                241,
                "                    let max_scroll = total_vis.saturating_sub(vh);",
            ),
            make_del(
                242,
                "                    let row_relative = row.saturating_sub(header_offset);",
            ),
            make_del(
                243,
                "                    let target_vis = Self::scrollbar_scroll_for_row(vh, max_scroll, row_relative);",
            ),
            make_del(
                244,
                "                    tracing::debug!(row, col, target_vis, max_scroll, \"scrollbar mousedown\");",
            ),
            make_del(
                245,
                "                    self.scroll_to_visual(target_vis);",
            ),
            make_del(246, "                } else if in_viewport {"),
        ];
        let adds = vec![
            make_add(237, "                if is_on_scrollbar {"),
            make_add(
                238,
                "                    self.on_scrollbar_click(row, col);",
            ),
            make_add(239, "                } else {"),
        ];
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let old_words: Vec<Vec<&str>> = dels
            .iter()
            .map(|r| significant_words(r.text(), profile))
            .collect();
        let new_words: Vec<Vec<&str>> = adds
            .iter()
            .map(|r| significant_words(r.text(), profile))
            .collect();
        let mut edge_count = 0;
        for (u, d) in dels.iter().enumerate() {
            for (v, a) in adds.iter().enumerate() {
                if has_information_edge_with_words(
                    d.text(),
                    a.text(),
                    &old_words[u],
                    &new_words[v],
                    None,
                    None,
                    profile,
                ) {
                    edge_count += 1;
                    assert_eq!((u, v), (0, 0), "only header lines may have an edge");
                }
            }
        }
        assert_eq!(
            edge_count, 1,
            "exactly one edge exists for the header refactor"
        );
        let groups = factor_cluster(&dels, &adds, profile);
        // Header refactor forms group 0
        assert_eq!(groups[0].old_range, 0..2);
        assert_eq!(groups[0].new_range, 0..1);

        // Body deletions (lines 229-246, indices 2..20) must be pure deletions,
        // never grouped with unrelated additions (lines 238-239, indices 1..3).
        for g in &groups[1..19] {
            assert_eq!(g.old_range.len(), 1);
            assert!(g.new_range.is_empty());
        }
        assert_eq!(groups[19].old_range, 20..20);
        assert_eq!(groups[19].new_range, 1..2);
        assert_eq!(groups[20].old_range, 20..20);
        assert_eq!(groups[20].new_range, 2..3);
    }

    #[test]
    fn test_factor_synchronized_blank_lines_separate_groups() {
        let dels = vec![
            make_del(248, "let row_in_viewport = row - header_offset;"),
            make_del(
                249,
                "let target_vis = self.viewport_row_to_visual(row_in_viewport);",
            ),
            make_del(
                250,
                "let geo = self.cached_geometry(self.last_content_width);",
            ),
            make_del(251, "let starts = &geo.starts;"),
            make_del(252, "let counts = &geo.counts;"),
            make_del(253, "let nav_counts = &geo.nav_counts;"),
            make_del(
                254,
                "let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {",
            ),
            make_del(255, "    HashMap::new()"),
            make_del(256, "} else {"),
            make_del(257, "    self.hunk_starts"),
            make_del(258, "        .iter()"),
            make_del(259, "        .map(|h| (h.flat, (h.file, h.hunk)))"),
            make_del(260, "        .collect()"),
            make_del(261, "};"),
            make_del(262, ""),
            make_del(263, "for (i, &s) in starts.iter().enumerate() {"),
        ];
        let adds = vec![
            make_add(242, "if in_viewport {"),
            make_add(243, "    let row_in_viewport = row - header_offset;"),
            make_add(
                244,
                "    let target_vis = self.viewport_row_to_visual(row_in_viewport);",
            ),
            make_add(
                245,
                "    let geo = self.cached_geometry(self.last_content_width);",
            ),
            make_add(246, "    let starts = &geo.starts;"),
            make_add(247, "    let counts = &geo.counts;"),
            make_add(248, "    let nav_counts = &geo.nav_counts;"),
            make_add(
                249,
                "    let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {",
            ),
            make_add(250, "        HashMap::new()"),
            make_add(251, "    } else {"),
            make_add(252, "        self.hunk_starts"),
            make_add(253, "            .iter()"),
            make_add(254, "            .map(|h| (h.flat, (h.file, h.hunk)))"),
            make_add(255, "            .collect()"),
            make_add(256, "    };"),
            make_add(257, ""),
            make_add(258, "    for (i, &s) in starts.iter().enumerate() {"),
        ];
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let groups = factor_cluster(&dels, &adds, profile);

        assert_eq!(groups[0].old_range, 0..14, "pre-blank block old");
        assert_eq!(groups[0].new_range, 0..15, "pre-blank block new");
        assert_eq!(groups[1].old_range, 14..15, "blank line old");
        assert_eq!(groups[1].new_range, 15..16, "blank line new");
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
        // Does not panic or hang; terminates cleanly into balanced mod and pure add
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].old_count(), 1);
        assert_eq!(groups[0].new_count(), 1);
        assert_eq!(groups[1].old_count(), 0);
        assert_eq!(groups[1].new_count(), 1);
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

    #[test]
    fn test_short_fragment_does_not_form_information_edge_with_unrelated_long_line() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let d_text = "                .round()";
        let a_text = "        let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)";
        let d_words = significant_words(d_text, profile);
        let a_words = significant_words(a_text, profile);
        assert!(
            !has_information_edge_with_words(
                d_text, a_text, &d_words, &a_words, None, None, profile
            ),
            "short method call .round() must NOT form an information edge with unrelated assignment let min_thumb = ..."
        );

        let d_expr = "            ((vh as f64 / total_vis_lines as f64) * track_h as f64)";
        let a_expr = "        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;";
        let d_expr_words = significant_words(d_expr, profile);
        let a_expr_words = significant_words(a_expr, profile);
        assert!(
            has_information_edge_with_words(
                d_expr,
                a_expr,
                &d_expr_words,
                &a_expr_words,
                None,
                None,
                profile
            ),
            "genuine reflowed expression must maintain an information edge"
        );
    }

    #[test]
    fn test_distance_penalized_edge_pruning_and_competing_suppression() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        // Line 0: strong match with line 0 (w ~ 0.9), weak partial match with line 1 (w
        // ~ 0.35) Line 1: strong match with line 1 (w ~ 0.9)
        let dels = vec![
            make_del(10, "let alpha_config = compute_alpha_settings();"),
            make_del(11, "let beta_config = compute_beta_settings();"),
        ];
        let adds = vec![
            make_add(10, "let alpha_config = compute_alpha_settings_fast();"),
            make_add(11, "let beta_config = compute_beta_settings_fast();"),
        ];
        let groups = factor_cluster(&dels, &adds, profile);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].old_range, 0..1);
        assert_eq!(groups[0].new_range, 0..1);
        assert_eq!(groups[1].old_range, 1..2);
        assert_eq!(groups[1].new_range, 1..2);
    }

    #[test]
    fn test_scrollbar_thumb_factoring_does_not_rope_min_thumb() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let dels = vec![
            make_del(
                238,
                "        let max_thumb = track_h.saturating_sub(1).max(2);",
            ),
            make_del(239, "        Some("),
            make_del(
                240,
                "            ((vh as f64 / total_vis_lines as f64) * track_h as f64)",
            ),
            make_del(241, "                .round()"),
            make_del(
                242,
                "                .clamp(2.0, max_thumb as f64) as usize,",
            ),
            make_del(243, "        )"),
        ];
        let adds = vec![
            make_add(238, "        let max_thumb = track_h.saturating_sub(1);"),
            make_add(
                239,
                "        if max_thumb < Self::SCROLLBAR_MIN_THUMB_LINES {",
            ),
            make_add(240, "            return None;"),
            make_add(241, "        }"),
            make_add(
                242,
                "        let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)",
            ),
            make_add(
                243,
                "            .clamp(Self::SCROLLBAR_MIN_THUMB_LINES, max_thumb);",
            ),
            make_add(
                244,
                "        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;",
            ),
            make_add(245, "        Some(prop_thumb.clamp(min_thumb, max_thumb))"),
        ];

        let groups = factor_cluster(&dels, &adds, profile);
        // max_thumb is factored as group 0 (old 0..1, new 0..1)
        assert_eq!(groups[0].old_range, 0..1);
        assert_eq!(groups[0].new_range, 0..1);

        // Group 1 captures the enclosed Some(...) expression replacement
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].old_range, 1..6);
        assert_eq!(groups[1].new_range, 1..8);

        // Crucial invariant: min_thumb (adds index 4) must NOT have an information edge
        // with .round() (dels index 3).
        let d_round_words = significant_words(dels[3].text(), profile);
        let a_min_thumb_words = significant_words(adds[4].text(), profile);
        assert!(
            !has_information_edge_with_words(
                dels[3].text(),
                adds[4].text(),
                &d_round_words,
                &a_min_thumb_words,
                None,
                None,
                profile
            ),
            "min_thumb must NOT form an information edge with .round()"
        );

        // In contrast, genuine reflowed expression ((vh as f64... (dels index 2)
        // maintains an edge with prop_thumb (adds index 6).
        let d_prop_words = significant_words(dels[2].text(), profile);
        let a_prop_words = significant_words(adds[6].text(), profile);
        assert!(
            has_information_edge_with_words(
                dels[2].text(),
                adds[6].text(),
                &d_prop_words,
                &a_prop_words,
                None,
                None,
                profile
            ),
            "prop_thumb must maintain an information edge with reflowed expression"
        );
    }
}
