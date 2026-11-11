//! Verbatim pre-seeding for unambiguous relocated lines before the DP walk.

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::{prelude::first_word, profile::LangProfile};

use crate::parse::{Hunk, Row};

use super::{ChangeCluster, filter::is_fragment_pair_rejected, survival::FnScopeCache};

/// Input bundle for [`verbatim_seeds`] (kept as a struct: nine fields
/// would trip `clippy::too_many_arguments`, and the call sites —
/// one production, five tests — read better named than positional).
pub struct VerbatimSeedInput<'a, 'm> {
    pub cluster: &'a ChangeCluster<'m>,
    pub h: &'a Hunk,
    pub del_by_num: &'a FxHashMap<u32, usize>,
    pub add_by_num: &'a FxHashMap<u32, usize>,
    pub old_content: Option<&'m [&'m str]>,
    pub new_content: Option<&'m [&'m str]>,
    pub profile: Option<&'m LangProfile>,
    pub fn_cache: &'a mut FnScopeCache<'m>,
    pub hard: &'a FxHashSet<usize>,
}

/// Verbatim pre-seeding: map unambiguous trimmed-equal del/add pairs in
/// one cluster before the DP walk, so corridor exclusion and global
/// traceback rerouting cannot strand true moves (pairing content-similar
/// rivals over verbatim counterparts rows away).
///
/// A pair seeds only when ALL hold:
/// - trimmed text non-blank, occurring exactly once among the cluster's del
///   candidates and exactly once among its add candidates (ambiguity —
///   duplicated lines — stays with the DP, which alone can weigh position
///   against content);
/// - different indents (same-spot verbatim pairs already resolve through the
///   Mov path or trivial-Mod; seeding targets relocations);
/// - neither endpoint pre-marked moved, hard-guarded, or fragment rejected
///   (same policy as the walk arms — moves.rs owns relocations);
/// - no same-indent head-matching rival add in the cluster: where same-slot
///   competition exists (e.g. an in-place edit beside a moved twin), the DP's
///   positional and content evidence decides, not a verbatim fiat.
///
/// Returns global add-index → global del-index pairs for direct insertion
/// into `pair_of`. Callers keep DP-walk exclusivity: Mod/Mov arms skip
/// seeded dels, and block formation skips seeded members (rows fall back
/// to 1-to-1/standalone rendering — never double-rendered). Assembly
/// needs no changes — it builds Mods from `pair_of` regardless of
/// provenance (heuristic aux may still attach unseeded neighbors around
/// seeded anchors).
pub fn verbatim_seeds(mut input: VerbatimSeedInput<'_, '_>) -> FxHashMap<usize, usize> {
    let mut seeds = ngram_block_seeds(&mut input);
    let seeded_dels: FxHashSet<usize> = seeds.values().copied().collect();
    let seeded_adds: FxHashSet<usize> = seeds.keys().copied().collect();

    let VerbatimSeedInput {
        cluster,
        h,
        del_by_num,
        add_by_num,
        old_content,
        new_content,
        profile,
        fn_cache,
        hard,
    } = input;
    // Head word mirroring the DP's first-word classification (comment
    // bodies classify by remainder), for the same-slot-rival rule below.
    fn head_of<'t>(text: &'t str, profile: Option<&LangProfile>) -> Option<&'t str> {
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        if comment {
            let body = text
                .trim()
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_word(body)
        } else {
            first_word(text)
        }
    }
    fn indent_of(text: &str) -> usize {
        text.len() - text.trim_start().len()
    }
    // Candidate pools: trimmed text -> (global row index, indent, head).
    // Keys borrow cluster rows; only indices escape.
    struct Cand<'x> {
        gi: usize,
        indent: usize,
        head: Option<&'x str>,
    }
    let mut dels: FxHashMap<&str, Vec<Cand<'_>>> = FxHashMap::default();
    for r in &cluster.old_rows {
        if !matches!(r, Row::Del(_)) {
            continue;
        }
        let text = r.text();
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        let Some(num) = r.old_line() else {
            continue;
        };
        let Some(&gi) = del_by_num.get(&num) else {
            continue;
        };
        dels.entry(t).or_default().push(Cand {
            gi,
            indent: indent_of(text),
            head: head_of(text, profile),
        });
    }
    let mut adds: FxHashMap<&str, Vec<Cand<'_>>> = FxHashMap::default();
    for r in &cluster.new_rows {
        if !matches!(r, Row::Add(_)) {
            continue;
        }
        let text = r.text();
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        let Some(num) = r.new_line() else {
            continue;
        };
        let Some(&gi) = add_by_num.get(&num) else {
            continue;
        };
        adds.entry(t).or_default().push(Cand {
            gi,
            indent: indent_of(text),
            head: head_of(text, profile),
        });
    }
    for (t, ds) in &dels {
        if ds.len() != 1 {
            continue;
        }
        let Some(as_) = adds.get(t) else {
            continue;
        };
        if as_.len() != 1 {
            continue;
        }
        let d = &ds[0];
        let a = &as_[0];
        if seeded_dels.contains(&d.gi) || seeded_adds.contains(&a.gi) {
            continue;
        }
        // Relocations seed at their own level: same-spot verbatim pairs
        // already resolve through Mov/trivial-Mod.
        if d.indent == a.indent {
            continue;
        }
        // Same-slot competition belongs to the DP: a same-indent
        // head-matching rival add means positional and content evidence
        // coincide there (e.g. an in-place edit beside a moved twin), and
        // only the DP weighs both. (The twin itself can never falsely
        // block: same indent as the del would already have excluded it
        // above.)
        let rivaled = adds
            .values()
            .flatten()
            .any(|o| o.indent == d.indent && o.head.is_some() && o.head == d.head);
        if rivaled {
            continue;
        }
        if hard.contains(&d.gi) || hard.contains(&a.gi) {
            continue;
        }
        // (Moved rows never reach here: the pools above admit only
        // `Row::Del`/`Row::Add`, so moves.rs ownership is structural.)
        if is_fragment_pair_rejected(d.gi, a.gi, h, old_content, new_content, profile, fn_cache) {
            continue;
        }
        seeds.insert(a.gi, d.gi);
    }
    seeds
}

#[derive(Clone, Copy)]
struct CandRow<'a> {
    gi: usize,
    trimmed: &'a str,
    indent: usize,
    is_blank: bool,
}

#[derive(Debug)]
struct BlockCandidate {
    old_start: usize,
    new_start: usize,
    len: usize,
    non_blank_chars: usize,
}

/// Compute net brace balance for a line outside double-quoted strings.
fn brace_delta(text: &str) -> isize {
    let mut delta = 0isize;
    let mut in_str = false;
    let mut prev = '\0';
    for ch in text.chars() {
        if ch == '"' && prev != '\\' {
            in_str = !in_str;
        } else if !in_str {
            if ch == '{' {
                delta += 1;
            } else if ch == '}' {
                delta -= 1;
            }
        }
        prev = ch;
    }
    delta
}

/// Detect contiguous sequences of verbatim/trimmed matches ($L \ge 2$)
/// with consistent non-zero indent shift.
fn ngram_block_seeds(input: &mut VerbatimSeedInput<'_, '_>) -> FxHashMap<usize, usize> {
    let old_cands: Vec<Option<CandRow<'_>>> = input
        .cluster
        .old_rows
        .iter()
        .map(|r| {
            if let Row::Del(d) = r
                && let Some(&gi) = input.del_by_num.get(&d.num)
            {
                let text = r.text();
                let trimmed = text.trim();
                let indent = text.len() - text.trim_start().len();
                Some(CandRow {
                    gi,
                    trimmed,
                    indent,
                    is_blank: trimmed.is_empty(),
                })
            } else {
                None
            }
        })
        .collect();

    let new_cands: Vec<Option<CandRow<'_>>> = input
        .cluster
        .new_rows
        .iter()
        .map(|r| {
            if let Row::Add(a) = r
                && let Some(&gi) = input.add_by_num.get(&a.num)
            {
                let text = r.text();
                let trimmed = text.trim();
                let indent = text.len() - text.trim_start().len();
                Some(CandRow {
                    gi,
                    trimmed,
                    indent,
                    is_blank: trimmed.is_empty(),
                })
            } else {
                None
            }
        })
        .collect();

    let mut candidates = Vec::new();
    let m = old_cands.len();
    let n = new_cands.len();

    for i in 0..m {
        let Some(old_0) = old_cands[i] else { continue };
        if old_0.is_blank {
            continue;
        }
        let b0 = brace_delta(old_0.trimmed);
        if b0 < 0 {
            // Cannot start with an unmatched closing delimiter.
            continue;
        }

        for j in 0..n {
            let Some(new_0) = new_cands[j] else { continue };
            if new_0.is_blank {
                continue;
            }
            if old_0.trimmed != new_0.trimmed {
                continue;
            }

            let delta = new_0.indent as isize - old_0.indent as isize;
            if delta == 0 {
                continue;
            }

            if input.hard.contains(&old_0.gi) || input.hard.contains(&new_0.gi) {
                continue;
            }
            if is_fragment_pair_rejected(
                old_0.gi,
                new_0.gi,
                input.h,
                input.old_content,
                input.new_content,
                input.profile,
                input.fn_cache,
            ) {
                continue;
            }

            let mut running_balance = b0;
            let mut k = 1;
            while i + k < m && j + k < n {
                let (Some(old_k), Some(new_k)) = (old_cands[i + k], new_cands[j + k]) else {
                    break;
                };
                if old_k.is_blank != new_k.is_blank {
                    break;
                }
                if !old_k.is_blank {
                    if old_k.trimmed != new_k.trimmed {
                        break;
                    }
                    if new_k.indent as isize - old_k.indent as isize != delta {
                        break;
                    }
                    let bk = brace_delta(old_k.trimmed);
                    if running_balance + bk < 0 {
                        // Unmatched closing brace from an outer scope: stop before it.
                        break;
                    }
                    running_balance += bk;
                }
                if input.hard.contains(&old_k.gi) || input.hard.contains(&new_k.gi) {
                    break;
                }
                if is_fragment_pair_rejected(
                    old_k.gi,
                    new_k.gi,
                    input.h,
                    input.old_content,
                    input.new_content,
                    input.profile,
                    input.fn_cache,
                ) {
                    break;
                }
                k += 1;
            }

            while k > 0 && old_cands[i + k - 1].unwrap().is_blank {
                k -= 1;
            }

            if k < 2 {
                continue;
            }

            let non_blank_count = (0..k)
                .filter(|&idx| !old_cands[i + idx].unwrap().is_blank)
                .count();
            if non_blank_count < 2 {
                continue;
            }

            let non_blank_chars = (0..k)
                .map(|idx| old_cands[i + idx].unwrap().trimmed.len())
                .sum();

            candidates.push(BlockCandidate {
                old_start: i,
                new_start: j,
                len: k,
                non_blank_chars,
            });
        }
    }

    let unique_candidates: Vec<BlockCandidate> = candidates
        .into_iter()
        .filter(|cand| {
            let l = cand.len;
            let cand_old = &old_cands[cand.old_start..cand.old_start + l];

            for i2 in 0..=m.saturating_sub(l) {
                if i2 == cand.old_start {
                    continue;
                }
                let slice = &old_cands[i2..i2 + l];
                let matches_seq = (0..l).all(|k| match (cand_old[k], slice[k]) {
                    (Some(c1), Some(c2)) => c1.trimmed == c2.trimmed && c1.is_blank == c2.is_blank,
                    _ => false,
                });
                if matches_seq {
                    return false;
                }
            }

            for j2 in 0..=n.saturating_sub(l) {
                if j2 == cand.new_start {
                    continue;
                }
                let slice = &new_cands[j2..j2 + l];
                let matches_seq = (0..l).all(|k| match (cand_old[k], slice[k]) {
                    (Some(c1), Some(c2)) => c1.trimmed == c2.trimmed && c1.is_blank == c2.is_blank,
                    _ => false,
                });
                if matches_seq {
                    return false;
                }
            }

            true
        })
        .collect();

    let mut sorted_candidates = unique_candidates;
    sorted_candidates.sort_by(|a, b| {
        b.len
            .cmp(&a.len)
            .then_with(|| b.non_blank_chars.cmp(&a.non_blank_chars))
            .then_with(|| a.new_start.cmp(&b.new_start))
            .then_with(|| a.old_start.cmp(&b.old_start))
    });

    let mut seeds = FxHashMap::default();
    let mut seeded_old = FxHashSet::default();
    let mut seeded_new = FxHashSet::default();

    for cand in sorted_candidates {
        let overlaps = (0..cand.len).any(|k| {
            seeded_old.contains(&(cand.old_start + k)) || seeded_new.contains(&(cand.new_start + k))
        });
        if overlaps {
            continue;
        }

        for k in 0..cand.len {
            let o_cand = old_cands[cand.old_start + k].unwrap();
            let n_cand = new_cands[cand.new_start + k].unwrap();
            seeds.insert(n_cand.gi, o_cand.gi);
            seeded_old.insert(cand.old_start + k);
            seeded_new.insert(cand.new_start + k);
        }
    }

    seeds
}
