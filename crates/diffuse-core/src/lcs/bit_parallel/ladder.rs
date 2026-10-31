//! Shared Tie-Breaking Ladder Walk.
//!
//! Evaluates candidate match alignments in forward document order using suffix
//! dynamic programming lookups, applying whitespace penalties and alphanumeric
//! heuristics.

/// Shared tie-breaking ladder walk for suffix dynamic programming.
/// Evaluates candidates in forward document order using suffix DP lookups.
#[inline(always)]
pub(crate) fn suffix_walk_ladder<F>(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    dp: F,
) -> usize
where
    F: Fn(usize, usize) -> u32,
{
    let n = a.len();
    let m = b.len();
    if dp(0, 0) == 0 {
        return 0;
    }

    let mut matched = 0usize;
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
                && dp(i, j + 1) == dp(i, j)
                && (j + 1..m).any(|k| b[k] == a[i] && dp(i + 1, k + 1) + 1 == dp(i, j))
            {
                j += 1;
                continue;
            }
            matched += b_token_chars[j];
            i += 1;
            j += 1;
            in_add_run = false;
        } else {
            let down = dp(i + 1, j);
            let right = dp(i, j + 1);
            if down > right {
                i += 1;
            } else if down < right {
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
            } else if !b[j + 1..].contains(&a[i])
                || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
            {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        }
    }

    matched
}
