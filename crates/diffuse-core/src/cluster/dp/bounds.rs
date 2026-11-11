//! Salient pruning filters, boundary checks, and fast intersection bounds.

use diffuse_syntax::{prelude::PropertySyntax, profile::LangProfile};

use crate::{cluster::ChangeCluster, similarity::TokenInterner};

use super::{
    arena::DpRowMeta,
    config::{
        MAX_OFFSET_KEYWORD_ZERO_SALIENT, MIN_RATIO_CHAR_SCALE, MIN_RATIO_CODE, MIN_RATIO_COMMENT,
        MIN_RATIO_COMMENT_OFFSET, MIN_RATIO_KEYWORD_ZERO_SALIENT, Weights,
    },
    tokens::DpTokens,
};

#[inline]
pub(crate) fn is_statement_head_keyword(word: &str, profile: Option<&LangProfile>) -> bool {
    if let Some(p) = profile {
        p.is_keyword(word)
            && !p.is_literal(word)
            && !matches!(word, "self" | "Self" | "this" | "super")
    } else {
        false
    }
}

#[inline(always)]
pub(crate) fn mask_overlap(a: &[u64; 4], b: &[u64; 4]) -> bool {
    ((a[0] & b[0]) | (a[1] & b[1]) | (a[2] & b[2]) | (a[3] & b[3])) != 0
}

/// Calculate the number of matched salient characters residing in a contiguous
/// (trimmed) prefix or suffix between two token sequences.
#[inline]
pub(crate) fn boundary_salient_chars(ot: &DpTokens, nt: &DpTokens) -> usize {
    let min_len = ot.ids.len().min(nt.ids.len());
    let mut prefix_tokens = 0;
    let mut prefix_salient = 0;
    while prefix_tokens < min_len && ot.ids[prefix_tokens] == nt.ids[prefix_tokens] {
        prefix_salient += nt.salient_token_chars[prefix_tokens];
        prefix_tokens += 1;
    }

    let mut suffix_tokens = 0;
    let mut suffix_salient = 0;
    let max_suffix = min_len.saturating_sub(prefix_tokens);
    let m = ot.ids.len();
    let n = nt.ids.len();
    while suffix_tokens < max_suffix
        && ot.ids[m - 1 - suffix_tokens] == nt.ids[n - 1 - suffix_tokens]
    {
        let nt_idx = n - 1 - suffix_tokens;
        suffix_salient += nt.salient_token_chars[nt_idx];
        suffix_tokens += 1;
    }

    prefix_salient + suffix_salient
}

#[inline(always)]
pub(crate) fn token_intersection_bound(
    a: &[(u32, u16)],
    b: &[(u32, u16)],
    char_lens: &[u32],
) -> usize {
    let mut i = 0;
    let mut j = 0;
    let mut bound = 0;
    while i < a.len() && j < b.len() {
        let (id_a, count_a) = a[i];
        let (id_b, count_b) = b[j];
        if id_a == id_b {
            let count = count_a.min(count_b) as usize;
            bound += (char_lens[id_a as usize] as usize) * count;
            i += 1;
            j += 1;
        } else if id_a < id_b {
            i += 1;
        } else {
            j += 1;
        }
    }
    bound
}

/// Matched salient credit graded by identifier extent.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn graded_salient_bound(
    a: &[(u32, u16)],
    b: &[(u32, u16)],
    anchored_a: &[(u32, u16)],
    anchored_b: &[(u32, u16)],
    containers_a: &[(u32, u32, u16)],
    containers_b: &[(u32, u32, u16)],
    char_lens: &[u32],
    factor: f64,
) -> usize {
    let mut i = 0;
    let mut j = 0;
    let mut ca = 0;
    let mut cb = 0;
    let mut ka = 0;
    let mut kb = 0;
    let mut credit = 0.0_f64;
    while i < a.len() && j < b.len() {
        let (id_a, count_a) = a[i];
        let (id_b, count_b) = b[j];
        if id_a == id_b {
            let total = count_a.min(count_b);
            while ca < anchored_a.len() && anchored_a[ca].0 < id_a {
                ca += 1;
            }
            while cb < anchored_b.len() && anchored_b[cb].0 < id_b {
                cb += 1;
            }
            let anchored_count = |arr: &[(u32, u16)], ptr: usize, id: u32| {
                arr.get(ptr).filter(|p| p.0 == id).map_or(0, |p| p.1)
            };
            while ka < containers_a.len() && containers_a[ka].0 < id_a {
                ka += 1;
            }
            while kb < containers_b.len() && containers_b[kb].0 < id_b {
                kb += 1;
            }
            let mut container_full = 0;
            let mut pa = ka;
            let mut pb = kb;
            while pa < containers_a.len()
                && containers_a[pa].0 == id_a
                && pb < containers_b.len()
                && containers_b[pb].0 == id_b
            {
                match containers_a[pa].1.cmp(&containers_b[pb].1) {
                    std::cmp::Ordering::Less => pa += 1,
                    std::cmp::Ordering::Greater => pb += 1,
                    std::cmp::Ordering::Equal => {
                        container_full += containers_a[pa].2.min(containers_b[pb].2);
                        pa += 1;
                        pb += 1;
                    }
                }
            }
            let anchored = (anchored_count(anchored_a, ca, id_a)
                + anchored_count(anchored_b, cb, id_b)
                + container_full)
                .min(total);
            let buried = total - anchored;
            let len = char_lens[id_a as usize] as f64;
            credit += len * (f64::from(anchored) + factor * f64::from(buried));
            i += 1;
            j += 1;
        } else if id_a < id_b {
            i += 1;
        } else {
            j += 1;
        }
    }
    credit as usize
}

/// Whether `longer` starts with `shorter` at a token boundary.
pub(crate) fn is_prefix_at_boundary(longer: &str, shorter: &str) -> bool {
    if let Some(rest) = longer.strip_prefix(shorter) {
        rest.is_empty()
            || shorter.ends_with(|c: char| !c.is_alphanumeric() && c != '_')
            || rest.starts_with(|c: char| c.is_whitespace() || (!c.is_alphanumeric() && c != '_'))
    } else {
        false
    }
}

#[inline]
pub(crate) fn is_reflow_candidate(text: &str) -> bool {
    !text.trim().is_empty()
}

pub(crate) fn is_match_arm_line(text: &str, profile: Option<&LangProfile>) -> bool {
    let Some(p) = profile else {
        return false;
    };
    if p.name != "rust" {
        return false;
    }
    if !p.operators.contains(&"=>") {
        return false;
    }
    let t = text.trim();
    if t.is_empty()
        || p.is_comment_line(t)
        || t.starts_with("//")
        || t.starts_with("/*")
        || t.starts_with('*')
    {
        return false;
    }
    let mut in_quote = false;
    let mut escaped = false;
    let chars: Vec<char> = t.chars().collect();
    for i in 0..chars.len() {
        if escaped {
            escaped = false;
            continue;
        }
        if chars[i] == '\\' {
            escaped = true;
            continue;
        }
        if chars[i] == '"' {
            in_quote = !in_quote;
            continue;
        }
        if !in_quote && chars[i] == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
            return true;
        }
    }
    false
}

pub(crate) fn is_statement_start(text: &str, profile: Option<&LangProfile>) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if let Some(p) = profile {
        if p.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.scope_stop_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.import_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.stmt_head_prefixes.iter().any(|&pf| t.starts_with(pf))
        {
            return true;
        }
        if (p.property_syntax == PropertySyntax::Equals
            || (p.hash_comment && !p.line_comment && !p.block_comment))
            && p.extract_property_key(t).is_some()
        {
            return true;
        }
        if p.section_headers && t.starts_with('[') && t.ends_with(']') {
            return true;
        }
        if t.starts_with("case ") || t.starts_with("default:") {
            return true;
        }
        if is_match_arm_line(t, profile) {
            return true;
        }
        return false;
    }
    // Fallback check for section headers when no profile is available:
    // `[section]`
    t.starts_with('[') && t.ends_with(']')
}

/// Whether traceback cell $(i, j)$ touches the band edge.
pub(crate) fn cell_touches_band_edge(
    bounds: &[(usize, usize)],
    n: usize,
    i: usize,
    j: usize,
) -> bool {
    match bounds.get(i) {
        Some(&(start, end)) => (j == start && start > 0) || (j == end && end < n),
        None => true,
    }
}

/// Grid-independent preconditions for a mod pair `(i, j)` to reach subline
/// LCS scoring in the fill loop.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mod_pair_needs_scoring(
    i: usize,
    j: usize,
    cluster: &ChangeCluster,
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    old_meta: &[DpRowMeta],
    new_meta: &[DpRowMeta],
    interner: &TokenInterner,
    w: &Weights,
) -> bool {
    let ot = &old_tokens[i];
    let om = old_meta[i];
    let nt = &new_tokens[j];
    let nm = new_meta[j];
    if om.comment != nm.comment || !om.kind.is_compatible(nm.kind) {
        return false;
    }
    if ot.ids.is_empty() && nt.ids.is_empty() {
        return false;
    }
    if om.trimmed_id == nm.trimmed_id {
        return false;
    }
    if ot.ids.is_empty() || nt.ids.is_empty() {
        return false;
    }
    let token_overlap = mask_overlap(&ot.token_mask, &nt.token_mask);
    if !token_overlap {
        return false;
    }
    let min_chars = ot.chars.min(nt.chars);
    let char_bound =
        token_intersection_bound(&ot.token_multiset, &nt.token_multiset, &interner.char_lens)
            .min(min_chars);
    if char_bound == 0 {
        return false;
    }

    let total = ot.chars.max(nt.chars);
    let max_matched = char_bound;
    let statement_head_match = match (om.first_word, nm.first_word) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    let is_property_match = om.property_key.is_some() && om.property_key == nm.property_key;
    let indent_shift = (om.indent as isize - nm.indent as isize).unsigned_abs();
    let offset_diff = (i as isize - j as isize).unsigned_abs();

    let min_ratio = if (om.comment || nm.comment) && offset_diff > 0 {
        MIN_RATIO_COMMENT_OFFSET
    } else if om.comment || nm.comment {
        MIN_RATIO_COMMENT
    } else {
        MIN_RATIO_CODE
    };
    let min_exact = if (om.comment || nm.comment) && offset_diff > 0 {
        MIN_RATIO_COMMENT_OFFSET
    } else if om.comment || nm.comment {
        MIN_RATIO_COMMENT
    } else {
        MIN_RATIO_CHAR_SCALE
    };
    let is_char_scale_pair = ot.salient_chars == 0 || nt.salient_chars == 0;
    let boundary_salient =
        if w.boundary_salient_multiplier > 1.0 || w.contiguous_boundary_bonus > 0.0 {
            let bs = boundary_salient_chars(ot, nt);
            if bs >= w.min_containment_len { bs } else { 0 }
        } else {
            0
        };
    let salient_bound = if mask_overlap(&ot.salient_mask, &nt.salient_mask) {
        graded_salient_bound(
            &ot.salient_multiset,
            &nt.salient_multiset,
            &ot.salient_anchored_multiset,
            &nt.salient_anchored_multiset,
            &ot.salient_buried_containers,
            &nt.salient_buried_containers,
            &interner.char_lens,
            w.embedded_salient_factor,
        )
        .min(ot.salient_chars.min(nt.salient_chars))
    } else {
        0
    };
    let max_salient_similarity_ratio: Option<f64> =
        if ot.salient_chars == 0 || nt.salient_chars == 0 {
            None
        } else {
            let total_salient = ot.salient_chars.max(nt.salient_chars);
            let effective_bound = salient_bound as f64
                + (w.boundary_salient_multiplier - 1.0) * boundary_salient as f64;
            Some((effective_bound / total_salient as f64).min(1.0))
        };

    let same_indent_head_match = statement_head_match
        && indent_shift == 0
        && !om.comment
        && !nm.comment
        && om.head_is_keyword
        && nm.head_is_keyword
        && !om.survives_in_new;
    let keyword_head_admissible = same_indent_head_match
        && (salient_bound > 0
            || (offset_diff <= MAX_OFFSET_KEYWORD_ZERO_SALIENT
                && (char_bound as f64 / total as f64) >= MIN_RATIO_KEYWORD_ZERO_SALIENT));
    let can_have_containment = indent_shift <= w.max_indent_shift
        && (om.comment == nm.comment)
        && ((om.stripped_len >= w.min_containment_len && nm.stripped_len >= w.min_containment_len)
            || same_indent_head_match);

    let is_return_slot = om.is_return_tail
        && nm.is_return_tail
        && indent_shift == 0
        && om.slot_owner.is_some()
        && om.slot_owner == nm.slot_owner;

    let could_be_valid_mod_fast = if is_char_scale_pair {
        (char_bound as f64 / total as f64) >= min_exact
    } else {
        max_salient_similarity_ratio.is_some_and(|r| r >= min_ratio)
            || is_property_match
            || is_return_slot
            || keyword_head_admissible
    };

    let mut is_prefix_containment = None;
    if !could_be_valid_mod_fast {
        if !can_have_containment {
            return false;
        }
        let ot_strip = &cluster.old_rows[i].text()[om.indent..om.indent + om.stripped_len];
        let nt_strip = &cluster.new_rows[j].text()[nm.indent..nm.indent + nm.stripped_len];
        let is_pc =
            is_prefix_at_boundary(ot_strip, nt_strip) || is_prefix_at_boundary(nt_strip, ot_strip);
        if !is_pc {
            return false;
        }
        is_prefix_containment = Some(true);
    }

    let max_syn = if is_property_match {
        w.head_agree_bonus + 200.0
    } else if same_indent_head_match {
        w.same_indent_head_bonus
    } else if statement_head_match {
        w.head_agree_bonus
    } else {
        0.0
    };
    let max_close = if is_return_slot {
        w.slot_continuity_bonus
    } else {
        w.boundary_proximity_bonus * om.prox_close * nm.prox_close
    };
    let max_open = w.boundary_proximity_bonus * om.prox_open * nm.prox_open;
    let indent_penalty = w.sprawl_penalty * (indent_shift as f64) * w.indent_penalty_factor;
    let boundary_bonus =
        if w.contiguous_boundary_bonus > 0.0 && boundary_salient >= w.min_containment_len {
            w.contiguous_boundary_bonus
        } else {
            0.0
        };
    let base_delta = w.matched_char_reward * max_matched as f64
        - w.sprawl_penalty * (total - max_matched) as f64
        - w.row_cost
        - indent_penalty
        + max_syn
        + max_close
        + max_open
        + boundary_bonus;

    if base_delta <= -2.0 * w.row_cost {
        if is_prefix_containment == Some(false)
            || (!can_have_containment && is_prefix_containment.is_none())
            || (base_delta + max_syn.max(w.head_agree_bonus) <= -2.0 * w.row_cost)
        {
            return false;
        }
        let is_pc = match is_prefix_containment {
            Some(v) => v,
            None => {
                let ot_strip = &cluster.old_rows[i].text()[om.indent..om.indent + om.stripped_len];
                let nt_strip = &cluster.new_rows[j].text()[nm.indent..nm.indent + nm.stripped_len];
                is_prefix_at_boundary(ot_strip, nt_strip)
                    || is_prefix_at_boundary(nt_strip, ot_strip)
            }
        };
        if !is_pc {
            return false;
        }
    }

    ot.ids != nt.ids
}
