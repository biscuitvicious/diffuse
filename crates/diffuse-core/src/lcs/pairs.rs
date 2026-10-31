//! Recursive Patience LCS, token anchoring, and identifier-sequence matching.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashMap;

use super::{
    bit_parallel,
    raw::raw_lcs_pairs_with_context,
    tokenizer::{are_contiguous_subwords, is_subword_token, unique_token_anchors},
};

/// LCS backtrace: map of old-token index → matched new-token index.
/// Combines recursive Patience unique token anchoring with run-continuation
/// LCS.
pub fn lcs_pairs(a: &[&str], b: &[&str]) -> FxHashMap<usize, usize> {
    lcs_pairs_recursive(a, b, &[], &[], None, None, None)
}

/// LCS backtrace with optional syntax profile for operator anchor awareness.
pub fn lcs_pairs_with_profile(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> FxHashMap<usize, usize> {
    lcs_pairs_recursive(a, b, &[], &[], None, None, profile)
}

/// Recursive Patience LCS backtrace with anchor lookahead context.
pub fn lcs_pairs_recursive(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    profile: Option<&LangProfile>,
) -> FxHashMap<usize, usize> {
    if a.is_empty() || b.is_empty() {
        return FxHashMap::default();
    }
    let anchors = unique_token_anchors(a, b, profile);
    if anchors.is_empty() {
        return raw_lcs_pairs_with_context(a, b, prev_a, prev_b, next_a, next_b);
    }
    let mut pairs = FxHashMap::default();
    let mut a_prev = 0;
    let mut b_prev = 0;
    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let mut back_a = ai;
        let mut back_b = bi;
        while back_a > a_prev
            && back_b > b_prev
            && is_subword_token(a[back_a - 1])
            && is_subword_token(b[back_b - 1])
            && a[back_a - 1] == b[back_b - 1]
        {
            back_a -= 1;
            back_b -= 1;
        }

        let (next_limit_a, next_limit_b) = if let Some(&(nai, nbi)) = anchors.get(anchor_idx + 1) {
            (nai, nbi)
        } else {
            (a.len(), b.len())
        };

        let mut fwd_a = ai;
        let mut fwd_b = bi;
        while fwd_a + 1 < next_limit_a
            && fwd_b + 1 < next_limit_b
            && are_contiguous_subwords(a[fwd_a], a[fwd_a + 1])
            && are_contiguous_subwords(b[fwd_b], b[fwd_b + 1])
            && a[fwd_a + 1] == b[fwd_b + 1]
        {
            fwd_a += 1;
            fwd_b += 1;
        }

        if back_a > a_prev && back_b > b_prev {
            let sub_prev_a = if a_prev == 0 { prev_a } else { &a[..a_prev] };
            let sub_prev_b = if b_prev == 0 { prev_b } else { &b[..b_prev] };
            let sub_pairs = lcs_pairs_recursive(
                &a[a_prev..back_a],
                &b[b_prev..back_b],
                sub_prev_a,
                sub_prev_b,
                Some(a[back_a]),
                Some(b[back_b]),
                profile,
            );
            for (si, sj) in sub_pairs {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (k_a, k_b) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(k_a, k_b);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }
    if a_prev < a.len() && b_prev < b.len() {
        let sub_prev_a = if a_prev == 0 { prev_a } else { &a[..a_prev] };
        let sub_prev_b = if b_prev == 0 { prev_b } else { &b[..b_prev] };
        let sub_pairs = lcs_pairs_recursive(
            &a[a_prev..],
            &b[b_prev..],
            sub_prev_a,
            sub_prev_b,
            next_a,
            next_b,
            profile,
        );
        for (si, sj) in sub_pairs {
            pairs.insert(a_prev + si, b_prev + sj);
        }
    }
    pairs
}

pub fn lcs_pairs_anchored(
    a: &[&str],
    b: &[&str],
    anchors: &[(usize, usize)],
) -> FxHashMap<usize, usize> {
    let mut pairs = FxHashMap::default();
    let mut a_prev = 0;
    let mut b_prev = 0;
    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let mut back_a = ai;
        let mut back_b = bi;
        while back_a > a_prev
            && back_b > b_prev
            && is_subword_token(a[back_a - 1])
            && is_subword_token(b[back_b - 1])
            && a[back_a - 1] == b[back_b - 1]
        {
            back_a -= 1;
            back_b -= 1;
        }

        let (next_limit_a, next_limit_b) = if let Some(&(nai, nbi)) = anchors.get(anchor_idx + 1) {
            (nai, nbi)
        } else {
            (a.len(), b.len())
        };

        let mut fwd_a = ai;
        let mut fwd_b = bi;
        while fwd_a + 1 < next_limit_a
            && fwd_b + 1 < next_limit_b
            && are_contiguous_subwords(a[fwd_a], a[fwd_a + 1])
            && are_contiguous_subwords(b[fwd_b], b[fwd_b + 1])
            && a[fwd_a + 1] == b[fwd_b + 1]
        {
            fwd_a += 1;
            fwd_b += 1;
        }

        if back_a > a_prev && back_b > b_prev {
            let sub_pairs = raw_lcs_pairs_with_context(
                &a[a_prev..back_a],
                &b[b_prev..back_b],
                &a[..a_prev],
                &b[..b_prev],
                Some(a[back_a]),
                Some(b[back_b]),
            );
            for (si, sj) in sub_pairs {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (k_a, k_b) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(k_a, k_b);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }
    if a_prev < a.len() && b_prev < b.len() {
        let sub_pairs = raw_lcs_pairs_with_context(
            &a[a_prev..],
            &b[b_prev..],
            &a[..a_prev],
            &b[..b_prev],
            None,
            None,
        );
        for (si, sj) in sub_pairs {
            pairs.insert(a_prev + si, b_prev + sj);
        }
    }
    pairs
}

/// The LCS over INTERNED token ids — the pairing's hot inner loop (D×A
/// runs per hunk). The String equality becomes a u32 compare; the tie-break
/// ladder's `token_is_ws`/`token_is_alnum` become array lookups (the
/// intern pool precomputed them). Used during hunk-collapse pairing.
pub fn lcs_pairs_ids(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
) -> FxHashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        let row = i * (m + 1);
        let next = row + m + 1;
        let ai = a[i];
        for j in (0..m).rev() {
            dp[row + j] = if ai == b[j] {
                dp[next + j + 1] + 1
            } else {
                dp[next + j].max(dp[row + j + 1])
            };
        }
    }
    let mut pairs = FxHashMap::default();
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;
    while i < n && j < m {
        if a[i] == b[j] {
            if id_ws[a[i] as usize]
                && in_add_run
                && !(i + 1 < n && j + 1 < m && a[i + 1] == b[j + 1] && !id_ws[a[i + 1] as usize])
            {
                j += 1;
                continue;
            }
            if in_add_run
                && dp[i * (m + 1) + j + 1] == dp[i * (m + 1) + j]
                && (j + 1..m).any(|k| {
                    b[k] == a[i] && dp[(i + 1) * (m + 1) + k + 1] + 1 == dp[i * (m + 1) + j]
                })
            {
                j += 1;
                continue;
            }
            pairs.insert(i, j);
            i += 1;
            j += 1;
            in_add_run = false;
        } else if dp[(i + 1) * (m + 1) + j] > dp[i * (m + 1) + j + 1] {
            i += 1;
        } else if dp[(i + 1) * (m + 1) + j] < dp[i * (m + 1) + j + 1] {
            j += 1;
            in_add_run = true;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
            j += 1;
            in_add_run = true;
        } else if !b[j + 1..].iter().any(|t| *t == a[i])
            || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
        {
            i += 1;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    pairs
}

/// The matched CHAR count of the LCS over interned ids — the window
/// scan's `sim` needs only this (NOT the pairs themselves). Same DP + the
/// SAME tie-break ladder as `lcs_pairs_ids`, but accumulates `b_token_chars[j]`
/// on each match instead of building a `HashMap` — the window scan's 42k
/// per-call `HashMaps` (alloc + per-pair inserts) were the remaining scan
/// cost. Behavior-identical to `lcs_pairs_ids(...).values().map(char).sum`.
pub fn lcs_matched_chars_ids(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    scratch: &mut Vec<u64>,
) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return 0;
    }
    if a == b {
        let mut total = 0;
        for (idx, &id) in b.iter().enumerate() {
            if let Some(&is_ws) = id_ws.get(id as usize)
                && !is_ws
            {
                total += b_token_chars.get(idx).copied().unwrap_or(0);
            }
        }
        return total;
    }
    bit_parallel::bit_parallel_suffix_matched_chars_scratch(
        a,
        b,
        id_ws,
        id_alnum,
        b_token_chars,
        scratch,
    )
}
