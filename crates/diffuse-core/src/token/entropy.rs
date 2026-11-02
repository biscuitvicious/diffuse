//! Shannon self-information entropy and multi-token phrase landmark anchoring.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use diffuse_syntax::profile::LangProfile;

use super::{
    class::{classify_token, is_anchor_content_token, is_delim, is_function_word},
    container::{ContainerKind, compute_token_containers, get_enclosing_caller},
};
use crate::lcs::{char_len, is_subword_token, token_is_alnum, token_is_ws, tokens_can_match};

/// Minimum self-information threshold for a single token to be considered an
/// anchor.
pub const MIN_SINGLE_TOKEN_ENTROPY: f64 = 1.0;

/// Minimum cumulative self-information threshold for a multi-token phrase
/// landmark.
pub const MIN_PHRASE_ENTROPY: f64 = 2.5;

/// Computes the Shannon self-information score of a single token:
/// $$I(t) = \text{class\_weight}(t.\text{class}) \times \ln(1 +
/// \text{char\_len}(t))$$
pub fn token_self_information(token: &str, profile: Option<&LangProfile>) -> f64 {
    let class = classify_token(token, profile);
    let len = char_len(token);
    class.class_weight() * ((1 + len) as f64).ln()
}

#[inline]
fn trim_to_balanced_boundaries(tokens: &[&str]) -> (usize, usize) {
    diffuse_algo::trim_balanced_boundaries(tokens, &diffuse_algo::UniversalDelimiterClassifier)
}

#[inline]
fn is_dangling_prefix_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) {
        return true;
    }
    if matches!(t, ")" | "]" | "}") {
        return true;
    }
    !is_anchor_content_token(t, profile) && !matches!(t, "(" | "[" | "{" | "\"" | "'" | "`")
}

#[inline]
fn is_dangling_suffix_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) {
        return true;
    }
    if matches!(t, "(" | "[" | "{") {
        return true;
    }
    !is_anchor_content_token(t, profile) && !matches!(t, ")" | "]" | "}" | "\"" | "'" | "`")
}

/// Identifies maximal multi-token phrase landmarks ($k \ge 2$) shared between
/// token slices `a` and `b`. Returns pairs of matching token indices `(a_idx,
/// b_idx)`.
pub fn multi_token_phrase_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    if a.len() < 2 || b.len() < 2 {
        return Vec::new();
    }

    let a_containers = compute_token_containers(a);
    let b_containers = compute_token_containers(b);

    let classifier = diffuse_algo::UniversalDelimiterClassifier;
    let mut a_depths_buf = [0u16; 64];
    let mut a_depths_vec;
    let a_depths: &mut [u16] = if a.len() <= 64 {
        &mut a_depths_buf[..a.len()]
    } else {
        a_depths_vec = vec![0u16; a.len()];
        &mut a_depths_vec
    };
    diffuse_algo::track_nesting_depths_into(&[], a, &classifier, a_depths);

    let mut b_depths_buf = [0u16; 64];
    let mut b_depths_vec;
    let b_depths: &mut [u16] = if b.len() <= 64 {
        &mut b_depths_buf[..b.len()]
    } else {
        b_depths_vec = vec![0u16; b.len()];
        &mut b_depths_vec
    };
    diffuse_algo::track_nesting_depths_into(&[], b, &classifier, b_depths);

    #[derive(Clone, Copy, Debug)]
    struct PhraseCandidate {
        a_start: usize,
        b_start: usize,
        len: usize,
        entropy: f64,
    }

    let mut candidates = Vec::new();

    // Inverted index of token occurrences in B to prune non-matching (i, j)
    // pairs
    let mut b_pos: FxHashMap<&str, SmallVec<[usize; 4]>> = FxHashMap::default();
    for (j, &tok) in b.iter().enumerate() {
        b_pos.entry(tok).or_default().push(j);
    }

    // Inverted index of token occurrences in A to fast-check phrase uniqueness
    let mut a_pos: FxHashMap<&str, SmallVec<[usize; 4]>> = FxHashMap::default();
    for (i, &tok) in a.iter().enumerate() {
        a_pos.entry(tok).or_default().push(i);
    }

    for i in 0..a.len() {
        let tok = a[i];
        let Some(matching_js) = b_pos.get(tok) else {
            continue;
        };

        for &j in matching_js {
            // Only inspect left-maximal matches
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                continue;
            }

            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }

            let (rel_start, rel_end) = trim_to_balanced_boundaries(&a[i..i + len]);
            let start = rel_start;
            let end = rel_end;

            let trimmed_len = end - start;
            if trimmed_len < 2 {
                continue;
            }

            let has_content = (start..end).any(|idx| is_anchor_content_token(a[i + idx], profile));
            if !has_content {
                continue;
            }

            let mut a_start = i + start;
            let mut b_start = j + start;
            let mut a_end = a_start + trimmed_len;
            let mut b_end = b_start + trimmed_len;

            // Landmark boundary invariant: A phrase landmark must begin and end on
            // valid syntactic phrase boundaries (alphanumerics, balanced container
            // delimiters, or quotes), trimming dangling operators, separators, or
            // whitespace.
            while a_start < a_end && is_dangling_prefix_token(a[a_start], profile) {
                a_start += 1;
                b_start += 1;
            }
            while a_end > a_start && is_dangling_suffix_token(a[a_end - 1], profile) {
                a_end -= 1;
                b_end -= 1;
            }

            let trimmed_len = a_end - a_start;
            if trimmed_len < 2 {
                continue;
            }

            // Invariant 2: A phrase that starts inside a syntactic container
            // (e.g. string literal or backtick code span) must not cross out
            // of that container into the outer scope or adjacent containers.
            if let Some(c_a) = a_containers[a_start]
                && a_containers[a_start..a_end].iter().any(|&c| c != Some(c_a))
            {
                continue;
            }
            if let Some(c_b) = b_containers[b_start]
                && b_containers[b_start..b_end].iter().any(|&c| c != Some(c_b))
            {
                continue;
            }

            let starts_inside_word =
                (a_start > 0 && is_subword_token(a[a_start - 1]) && is_subword_token(a[a_start]))
                    || (b_start > 0
                        && is_subword_token(b[b_start - 1])
                        && is_subword_token(b[b_start]));
            let ends_inside_word =
                (a_end < a.len() && is_subword_token(a[a_end - 1]) && is_subword_token(a[a_end]))
                    || (b_end < b.len()
                        && is_subword_token(b[b_end - 1])
                        && is_subword_token(b[b_end]));

            if starts_inside_word || ends_inside_word {
                continue;
            }

            if !tokens_can_match(a, b, &[], &[], None, None, a_start, b_start) {
                continue;
            }
            let phrase_slice = &a[a_start..a_start + trimmed_len];

            // Verify unique occurrence in both token slices.
            // A phrase cannot occur at any window where its least frequent
            // token is not present.
            let min_a_idx = (0..trimmed_len)
                .min_by_key(|&idx| a_pos.get(phrase_slice[idx]).map_or(0, |p| p.len()))
                .unwrap_or(0);
            let count_a = {
                let tok = phrase_slice[min_a_idx];
                let positions = &a_pos[tok];
                if positions.len() == 1 {
                    1
                } else {
                    let mut c = 0;
                    for &pos in positions {
                        if pos >= min_a_idx && pos + trimmed_len - min_a_idx <= a.len() {
                            let start = pos - min_a_idx;
                            if &a[start..start + trimmed_len] == phrase_slice {
                                c += 1;
                                if c > 1 {
                                    break;
                                }
                            }
                        }
                    }
                    c
                }
            };
            if count_a != 1 {
                continue;
            }

            let min_b_idx = (0..trimmed_len)
                .min_by_key(|&idx| b_pos.get(phrase_slice[idx]).map_or(0, |p| p.len()))
                .unwrap_or(0);
            let (total_b, depth_b) = {
                let tok = phrase_slice[min_b_idx];
                let positions = &b_pos[tok];
                let mut total = 0;
                let mut depth_count = 0;
                for &pos in positions {
                    if pos >= min_b_idx && pos + trimmed_len - min_b_idx <= b.len() {
                        let start = pos - min_b_idx;
                        if &b[start..start + trimmed_len] == phrase_slice {
                            total += 1;
                            if b_depths[start] == a_depths[a_start] {
                                depth_count += 1;
                            }
                        }
                    }
                }
                (total, depth_count)
            };

            let is_unique =
                total_b == 1 || (a_depths[a_start] == b_depths[b_start] && depth_b == 1);
            if !is_unique {
                continue;
            }

            // Information symmetry / bilateral ungroundedness check for phrases:
            // An isolated phrase flanked by edits on both sides must share
            // context with at least one side and match Dyck nesting depth.
            let a_end = a_start + trimmed_len;
            let b_end = b_start + trimmed_len;
            let left_content_a = a[..a_start].iter().any(|&x| !token_is_ws(x));
            let left_content_b = b[..b_start].iter().any(|&x| !token_is_ws(x));
            let right_content_a = a[a_end..].iter().any(|&x| !token_is_ws(x));
            let right_content_b = b[b_end..].iter().any(|&x| !token_is_ws(x));

            if left_content_a && left_content_b && right_content_a && right_content_b {
                let depth_discordant = a_depths[a_start] != b_depths[b_start]
                    && a_depths[a_start] >= 1
                    && b_depths[b_start] >= 1
                    && get_enclosing_caller(a, a_start) != get_enclosing_caller(b, b_start);
                let left_supported = a[..a_start].iter().any(|&x| {
                    !token_is_ws(x)
                        && token_is_alnum(x)
                        && !is_function_word(x)
                        && b[..b_start].contains(&x)
                });
                let right_supported = a[a_end..].iter().any(|&x| {
                    !token_is_ws(x)
                        && token_is_alnum(x)
                        && !is_function_word(x)
                        && b[b_end..].contains(&x)
                });
                if depth_discordant && !left_supported && !right_supported {
                    continue;
                }
            }

            // Verify cumulative information entropy meets landmark threshold
            let mut entropy = 0.0;
            for &tok in phrase_slice {
                entropy += token_self_information(tok, profile);
            }
            if entropy < MIN_PHRASE_ENTROPY {
                continue;
            }

            candidates.push(PhraseCandidate {
                a_start,
                b_start,
                len: trimmed_len,
                entropy,
            });
        }
    }

    let selected = diffuse_algo::find_monotonic_chain(
        &candidates,
        |c| diffuse_algo::MatchInterval::new(c.a_start, c.b_start, c.len),
        |c| c.entropy,
    );

    // Emit all non-whitespace tokens within selected phrases as anchors.
    let mut result = Vec::new();
    for phrase in selected {
        for offset in 0..phrase.len {
            let t = a[phrase.a_start + offset];
            if !token_is_ws(t) {
                result.push((phrase.a_start + offset, phrase.b_start + offset));
            }
        }
    }

    result
}

/// Computes entropy-weighted anchors combining multi-token phrase landmarks
/// and high-information single tokens.
pub fn entropy_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    let phrase_anchors = multi_token_phrase_anchors(a, b, profile);
    let a_containers = compute_token_containers(a);
    let b_containers = compute_token_containers(b);

    let classifier = diffuse_algo::UniversalDelimiterClassifier;
    let mut a_depths_buf = [0u16; 64];
    let mut a_depths_vec;
    let a_depths: &mut [u16] = if a.len() <= 64 {
        &mut a_depths_buf[..a.len()]
    } else {
        a_depths_vec = vec![0u16; a.len()];
        &mut a_depths_vec
    };
    diffuse_algo::track_nesting_depths_into(&[], a, &classifier, a_depths);

    let mut b_depths_buf = [0u16; 64];
    let mut b_depths_vec;
    let b_depths: &mut [u16] = if b.len() <= 64 {
        &mut b_depths_buf[..b.len()]
    } else {
        b_depths_vec = vec![0u16; b.len()];
        &mut b_depths_vec
    };
    diffuse_algo::track_nesting_depths_into(&[], b, &classifier, b_depths);

    let mut a_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (i, &t) in a.iter().enumerate() {
        if !token_is_ws(t) && !is_delim(t) {
            let entry = a_counts.entry(t).or_insert((0, i));
            entry.0 += 1;
        }
    }

    let mut b_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (j, &t) in b.iter().enumerate() {
        if !token_is_ws(t) && !is_delim(t) {
            let entry = b_counts.entry(t).or_insert((0, j));
            entry.0 += 1;
        }
    }

    let is_compatible_with_phrases = |cand_a: usize, cand_b: usize| -> bool {
        for &(pa, pb) in &phrase_anchors {
            if cand_a == pa || cand_b == pb {
                return false;
            }
            if (cand_a < pa && cand_b > pb) || (cand_a > pa && cand_b < pb) {
                return false;
            }
        }
        true
    };

    let mut candidates = Vec::with_capacity(phrase_anchors.len() + 16);
    candidates.extend_from_slice(&phrase_anchors);

    for (&t, &(count_a, idx_a)) in &a_counts {
        if count_a == 1
            && let Some(&(count_b, idx_b)) = b_counts.get(t)
            && count_b == 1
        {
            let is_op = profile.is_some_and(|p| p.is_operator_anchor(t));
            let is_alnum = token_is_alnum(t);
            if (is_alnum || is_op)
                && !is_function_word(t)
                && is_compatible_with_phrases(idx_a, idx_b)
                && ContainerKind::is_compatible_with(a_containers[idx_a], b_containers[idx_b])
            {
                // Information symmetry / bilateral ungroundedness & depth concordance check:
                // An isolated single token flanked by edits on both sides
                // must share context with at least one side.
                let mut left_bound_a = idx_a;
                while left_bound_a > 0
                    && (is_subword_token(a[left_bound_a - 1]) || a[left_bound_a - 1] == "::")
                {
                    left_bound_a -= 1;
                }
                let mut left_bound_b = idx_b;
                while left_bound_b > 0
                    && (is_subword_token(b[left_bound_b - 1]) || b[left_bound_b - 1] == "::")
                {
                    left_bound_b -= 1;
                }

                let mut right_bound_a = idx_a + 1;
                while right_bound_a < a.len()
                    && (is_subword_token(a[right_bound_a]) || a[right_bound_a] == "::")
                {
                    right_bound_a += 1;
                }
                let mut right_bound_b = idx_b + 1;
                while right_bound_b < b.len()
                    && (is_subword_token(b[right_bound_b]) || b[right_bound_b] == "::")
                {
                    right_bound_b += 1;
                }

                let left_content_a = a[..left_bound_a].iter().any(|&x| !token_is_ws(x));
                let left_content_b = b[..left_bound_b].iter().any(|&x| !token_is_ws(x));
                let right_content_a = a[right_bound_a..].iter().any(|&x| !token_is_ws(x));
                let right_content_b = b[right_bound_b..].iter().any(|&x| !token_is_ws(x));

                if left_content_a && left_content_b && right_content_a && right_content_b {
                    let depth_discordant = a_depths[idx_a] != b_depths[idx_b]
                        && a_depths[idx_a] >= 1
                        && b_depths[idx_b] >= 1
                        && get_enclosing_caller(a, idx_a) != get_enclosing_caller(b, idx_b);

                    let is_support_token = |tok: &str| {
                        !token_is_ws(tok)
                            && !is_delim(tok)
                            && (token_is_alnum(tok)
                                || profile.is_some_and(|p| p.is_operator_anchor(tok)))
                            && !is_function_word(tok)
                    };
                    let left_supported = a[..left_bound_a]
                        .iter()
                        .any(|&x| is_support_token(x) && b[..left_bound_b].contains(&x));
                    let right_supported = a[right_bound_a..]
                        .iter()
                        .any(|&x| is_support_token(x) && b[right_bound_b..].contains(&x));

                    if depth_discordant && !left_supported && !right_supported {
                        continue;
                    }
                    if profile.is_none() && !left_supported && !right_supported {
                        continue;
                    }
                }

                if is_alnum {
                    let left_sub_a = idx_a > 0 && is_subword_token(a[idx_a - 1]);
                    let left_sub_b = idx_b > 0 && is_subword_token(b[idx_b - 1]);
                    let right_sub_a = idx_a + 1 < a.len() && is_subword_token(a[idx_a + 1]);
                    let right_sub_b = idx_b + 1 < b.len() && is_subword_token(b[idx_b + 1]);
                    if left_sub_a || left_sub_b || right_sub_a || right_sub_b {
                        continue;
                    }
                }
                if tokens_can_match(a, b, &[], &[], None, None, idx_a, idx_b) {
                    let info = token_self_information(t, profile);
                    if info >= MIN_SINGLE_TOKEN_ENTROPY || is_op {
                        candidates.push((idx_a, idx_b));
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    candidates.sort_by_key(|&(ai, _)| ai);

    // Compute Longest Increasing Subsequence of b indices
    let mut piles: Vec<usize> = Vec::new();
    let mut pile_indices: Vec<usize> = Vec::new();
    let mut parent = vec![None; candidates.len()];

    for (idx, &(_, b_idx)) in candidates.iter().enumerate() {
        let p = piles.partition_point(|&x| x < b_idx);
        if p == piles.len() {
            piles.push(b_idx);
            pile_indices.push(idx);
        } else {
            piles[p] = b_idx;
            pile_indices[p] = idx;
        }
        if p > 0 {
            parent[idx] = Some(pile_indices[p - 1]);
        }
    }

    let mut lis = Vec::new();
    let mut curr = *pile_indices.last().unwrap();
    loop {
        lis.push(candidates[curr]);
        match parent[curr] {
            Some(prev) => curr = prev,
            None => break,
        }
    }
    lis.reverse();
    lis
}
