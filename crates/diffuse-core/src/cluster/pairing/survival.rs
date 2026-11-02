//! Per-del pairing guards and surplus survival accounting.

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{
    parse::{Hunk, Row},
    scope::is_tail_before_closing,
};

/// Per-hunk memo of [`crate::scope::fn_of_str`] keyed by `(old_side, line)`:
/// the guarded-dels pass and the transition-walk pair checks query the same
/// rows' enclosing scopes repeatedly; each is resolved once per hunk.
pub type FnScopeCache<'a> = FxHashMap<(bool, u32), Option<&'a str>>;

pub(crate) fn memo_fn_of_str<'a>(
    cache: &mut FnScopeCache<'a>,
    old_side: bool,
    content: Option<&'a [&'a str]>,
    line: Option<u32>,
    profile: Option<&LangProfile>,
) -> Option<&'a str> {
    let line = line?;
    if let Some(cached) = cache.get(&(old_side, line)) {
        return *cached;
    }
    let computed = crate::scope::fn_of_str(content, Some(line), profile);
    cache.insert((old_side, line), computed);
    computed
}

/// Per-del pairing guards for one hunk: hard vetoes plus soft survival
/// signals. Relocated lines (`is_moved`) stay hard-guarded — moves.rs owns
/// relocations. Verbatim twins surviving elsewhere are *not* vetoed: they
/// are reported as soft signals carrying multiplicity accounting, and the
/// DP penalizes (rather than forbids) pairing them, so high-similarity
/// modifications still form while slot-coincidence pairings fall back to
/// `Del` + `Add`.
#[derive(Default)]
pub struct DelGuards {
    /// Global row indices that must never pair.
    pub hard: FxHashSet<usize>,
    /// Old line numbers with surplus verbatim survival elsewhere
    /// (`count_new >= count_old` in the resolved scope): pairing one as
    /// `Mod` pays `Weights::survives_elsewhere_penalty`.
    pub soft: FxHashSet<u32>,
}

/// True when `trim` survives with multiplicity intact in the resolved
/// scope: the new count covers a non-zero old count, so this del instance
/// is redundant (a surviving twin) rather than transformed. A modified
/// instance always drops the new count below the old one and is exempt.
/// The non-zero old count also keeps synthetic contents (which may omit
/// the del text entirely) from ever penalizing. Scope mirrors the old
/// veto: the enclosing function when both bodies resolve, otherwise the
/// whole file; unavailable content never penalizes.
fn surplus_survival<'a>(
    trim: &str,
    del_fn: Option<&str>,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    old_counts: &mut Option<FxHashMap<&'a str, usize>>,
    new_counts: &mut Option<FxHashMap<&'a str, usize>>,
) -> bool {
    // Count occurrences of one trimmed line.
    fn count_in(lines: &[&str], trim: &str) -> usize {
        lines.iter().filter(|l| l.trim() == trim).count()
    }
    if let Some(dfn) = del_fn
        && let (Some(old_body), Some(new_body)) = (
            crate::scope::fn_body_lines(old_content, dfn, profile),
            crate::scope::fn_body_lines(new_content, dfn, profile),
        )
    {
        // Surplus needs a non-zero baseline: a twin can only remain
        // where one existed.
        return count_in(new_body, trim) >= count_in(old_body, trim).max(1);
    }
    match (old_content, new_content) {
        (Some(old), Some(new)) => {
            let old_map = old_counts.get_or_insert_with(|| {
                let mut m = FxHashMap::default();
                for l in old.iter() {
                    let s: &'a str = l;
                    *m.entry(s.trim()).or_insert(0) += 1;
                }
                m
            });
            let new_map = new_counts.get_or_insert_with(|| {
                let mut m = FxHashMap::default();
                for l in new.iter() {
                    let s: &'a str = l;
                    *m.entry(s.trim()).or_insert(0) += 1;
                }
                m
            });
            new_map.get(trim).copied().unwrap_or(0)
                >= old_map.get(trim).copied().unwrap_or(0).max(1)
        }
        _ => false,
    }
}

pub fn classify_dels<'a>(
    h: &Hunk,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    fn_cache: &mut FnScopeCache<'a>,
) -> DelGuards {
    if !h
        .rows
        .iter()
        .any(|r| matches!(r, Row::Del(_) | Row::MovSrc { .. }))
    {
        return DelGuards::default();
    }

    let mut hard = FxHashSet::default();
    let mut soft = FxHashSet::default();
    let mut new_counts: Option<FxHashMap<&str, usize>> = None;
    let mut old_counts: Option<FxHashMap<&str, usize>> = None;
    let has_any_moved = h.rows.iter().any(|a| a.is_moved());

    let same_hunk_add_trimmed: FxHashSet<&str> = h
        .rows
        .iter()
        .filter_map(|a| match a {
            Row::Add(_) | Row::MovTgt { .. } => Some(a.text().trim()),
            _ => None,
        })
        .collect();

    let same_hunk_property_keys: FxHashSet<&str> = if let Some(p) = profile {
        h.rows
            .iter()
            .filter_map(|a| match a {
                Row::Add(_) | Row::MovTgt { .. } => p.extract_property_key(a.text()),
                _ => None,
            })
            .collect()
    } else {
        FxHashSet::default()
    };

    let moved_old_lines: FxHashSet<u32> = if has_any_moved {
        h.rows
            .iter()
            .filter_map(|a| if a.is_moved() { a.old_line() } else { None })
            .collect()
    } else {
        FxHashSet::default()
    };
    let moved_trimmed: FxHashSet<&str> = if has_any_moved {
        h.rows
            .iter()
            .filter_map(|a| {
                if a.is_moved() {
                    Some(a.text().trim())
                } else {
                    None
                }
            })
            .collect()
    } else {
        FxHashSet::default()
    };

    // Precompute add-tail candidates to eliminate O(D * A) scans in the
    // deletion loop.
    let mut has_any_add_tail = false;
    let mut add_tail_fns: FxHashSet<&str> = FxHashSet::default();
    for a in &h.rows {
        if matches!(a, Row::Add(_) | Row::MovTgt { .. })
            && is_tail_before_closing(a.new_line(), false, old_content, new_content, profile)
        {
            has_any_add_tail = true;
            if let Some(afn) = memo_fn_of_str(fn_cache, false, new_content, a.new_line(), profile) {
                add_tail_fns.insert(afn);
            }
        }
    }

    for (ri, r) in h.rows.iter().enumerate() {
        // Del and moved-source rows carry old line numbers by construction;
        // everything else skips classification entirely.
        let Some(line_num) = (match r {
            Row::Del(d) | Row::MovSrc { line: d, .. } => Some(d.num),
            _ => None,
        }) else {
            continue;
        };
        if r.is_moved() {
            hard.insert(ri);
            continue;
        }
        let r_trim = r.text().trim();
        let was_moved = has_any_moved
            && !r_trim.is_empty()
            && (r
                .old_line()
                .is_some_and(|num| moved_old_lines.contains(&num))
                || moved_trimmed.contains(r_trim));
        if was_moved {
            hard.insert(ri);
            continue;
        }
        let has_same_hunk_add = same_hunk_add_trimmed.contains(r_trim);
        let has_same_hunk_property_add = profile.is_some_and(|p| {
            p.extract_property_key(r.text())
                .is_some_and(|key| same_hunk_property_keys.contains(key))
        });
        let del_is_tail =
            is_tail_before_closing(r.old_line(), true, old_content, new_content, profile);
        let del_fn = if del_is_tail || (!has_same_hunk_add && !has_same_hunk_property_add) {
            memo_fn_of_str(fn_cache, true, old_content, r.old_line(), profile)
        } else {
            None
        };
        let tail_candidate = del_is_tail
            && has_any_add_tail
            && match del_fn {
                None => true,
                Some(dfn) => add_tail_fns.contains(dfn),
            };

        if !r_trim.is_empty()
            && !tail_candidate
            && !has_same_hunk_add
            && !has_same_hunk_property_add
            && surplus_survival(
                r_trim,
                del_fn,
                old_content,
                new_content,
                profile,
                &mut old_counts,
                &mut new_counts,
            )
        {
            soft.insert(line_num);
        }
    }
    DelGuards { hard, soft }
}
