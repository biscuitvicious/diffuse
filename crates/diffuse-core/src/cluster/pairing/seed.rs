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
pub fn verbatim_seeds(input: VerbatimSeedInput<'_, '_>) -> FxHashMap<usize, usize> {
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
    let mut seeds = FxHashMap::default();
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
