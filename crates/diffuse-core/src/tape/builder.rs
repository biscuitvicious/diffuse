//! Construction of alignment tapes from raw text strings via LCS alignment.

use diffuse_algo::DelimiterClassifier;
use diffuse_syntax::profile::LangProfile;

use crate::{
    lcs::{
        TokenContainerContext, char_len, char_slice, filter_container_and_subword_pairs,
        lcs_pairs_anchored, raw_lcs_pairs, token_is_ws, token_product_cap, tokenize_line,
        unique_token_anchors,
    },
    tape::{AlignmentTape, TapeChunk},
    token::classify_whitespace,
};

#[inline]
fn truncate_for_trace(s: &str, max_len: usize) -> &str {
    match s.char_indices().nth(max_len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

impl<'a> AlignmentTape<'a> {
    /// Construct an alignment tape from old and new line text.
    pub fn from_texts(old_text: &'a str, new_text: &'a str, profile: Option<&LangProfile>) -> Self {
        Self::from_texts_with_cap(old_text, new_text, profile, token_product_cap())
    }

    /// Construct an alignment tape from old and new line text with explicit
    /// token cap.
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(
            old = %truncate_for_trace(old_text, 40),
            new = %truncate_for_trace(new_text, 40),
            strategy = tracing::field::Empty,
            anchors = tracing::field::Empty,
            matched_chars = tracing::field::Empty,
            ratio = tracing::field::Empty
        )
    )]
    pub fn from_texts_with_cap(
        old_text: &'a str,
        new_text: &'a str,
        profile: Option<&LangProfile>,
        cap: usize,
    ) -> Self {
        let mut tape = Self::default();

        if old_text.is_empty() && new_text.is_empty() {
            return tape;
        }
        if old_text == new_text {
            tape.push(TapeChunk::Keep(new_text));
            return tape;
        }

        let ot: Vec<&str> = tokenize_line(old_text).collect();
        let nt: Vec<&str> = tokenize_line(new_text).collect();

        if ot.len() * nt.len() > cap {
            if !old_text.is_empty() {
                tape.push(TapeChunk::Strike(old_text));
            }
            if !new_text.is_empty() {
                tape.push(TapeChunk::Fill(new_text));
            }
            return tape;
        }

        let mut prefix = 0usize;
        while prefix < ot.len() && prefix < nt.len() && ot[prefix] == nt[prefix] {
            prefix += 1;
        }
        if prefix > 0
            && (ot[prefix..].iter().all(|t| token_is_ws(t))
                != nt[prefix..].iter().all(|t| token_is_ws(t)))
        {
            while prefix > 0 && token_is_ws(ot[prefix - 1]) {
                prefix -= 1;
            }
        }

        let mut suffix = 0usize;
        while suffix < (ot.len() - prefix)
            && suffix < (nt.len() - prefix)
            && ot[ot.len() - 1 - suffix] == nt[nt.len() - 1 - suffix]
        {
            suffix += 1;
        }
        if suffix > 0
            && (ot[..ot.len() - suffix].iter().all(|t| token_is_ws(t))
                != nt[..nt.len() - suffix].iter().all(|t| token_is_ws(t)))
        {
            while suffix > 0 && token_is_ws(ot[ot.len() - suffix]) {
                suffix -= 1;
            }
        }

        let mut o_pos = 0usize;
        let mut o_spans = Vec::with_capacity(ot.len());
        for t in &ot {
            let len = char_len(t);
            o_spans.push((o_pos, o_pos + len));
            o_pos += len;
        }

        let mut n_pos = 0usize;
        let mut n_spans = Vec::with_capacity(nt.len());
        for t in &nt {
            let len = char_len(t);
            n_spans.push((n_pos, n_pos + len));
            n_pos += len;
        }

        let ot_last_eq_nt_last = !ot.is_empty() && !nt.is_empty() && ot.last() == nt.last();
        let ot_first_eq_nt_first = !ot.is_empty() && !nt.is_empty() && ot.first() == nt.first();

        let is_unambiguous_prefix =
            (prefix == ot.len() || prefix == nt.len()) && !ot_last_eq_nt_last;
        let is_unambiguous_suffix =
            (suffix == ot.len() || suffix == nt.len()) && !ot_first_eq_nt_first;

        // Anchors are computed lazily (see below): the pure prefix/suffix
        // branches never consult them, and the low-ratio fallback only needs
        // `has_anchors` when the ratio actually drops below threshold (rare).
        // Same decisions, none of the entropy/phrase/container work on the
        // common path.
        let mut anchors: Option<Vec<(usize, usize)>> = None;

        let span = tracing::Span::current();
        let mut sorted_pairs = if is_unambiguous_prefix {
            span.record("strategy", "pure_prefix");
            let mut pairs = Vec::with_capacity(prefix);
            for i in 0..prefix {
                pairs.push((i, i));
            }
            pairs
        } else if is_unambiguous_suffix {
            span.record("strategy", "pure_suffix");
            let mut pairs = Vec::with_capacity(suffix);
            for k in 0..suffix {
                pairs.push((ot.len() - suffix + k, nt.len() - suffix + k));
            }
            pairs
        } else {
            let computed = unique_token_anchors(&ot, &nt, profile);
            let has_anchors = !computed.is_empty();
            let p_map = if has_anchors {
                span.record("anchors", tracing::field::debug(&computed));
                span.record("strategy", "anchored");
                lcs_pairs_anchored(&ot, &nt, &computed)
            } else {
                span.record("strategy", "raw");
                raw_lcs_pairs(&ot, &nt)
            };
            anchors = Some(computed);
            let mut pairs: Vec<(usize, usize)> = p_map.into_iter().collect();
            pairs.sort_by_key(|&(o, _)| o);
            pairs
        };

        // Level B defense-in-depth: Delimiter Container Invariance & Subword
        // Cohesion. Purge any matched pair where both tokens sit inside active
        // but mismatched delimiter containers (e.g. `()` vs `{}` or `[]` vs
        // `()`). Also purge any subword match with discordant boundary context
        // across differing container depths or enclosing containers, which
        // would slice compound identifiers open across container boundaries.
        let classifier = diffuse_algo::UniversalDelimiterClassifier;
        let c_old = diffuse_algo::track_enclosing_containers(&ot, &classifier);
        let c_new = diffuse_algo::track_enclosing_containers(&nt, &classifier);
        let d_old = diffuse_algo::track_nesting_depths_with_prev(&[], &ot, &classifier);
        let d_new = diffuse_algo::track_nesting_depths_with_prev(&[], &nt, &classifier);

        filter_container_and_subword_pairs(
            &mut sorted_pairs,
            &TokenContainerContext {
                tokens: &ot,
                containers: &c_old,
                depths: &d_old,
                lines: None,
            },
            &TokenContainerContext {
                tokens: &nt,
                containers: &c_new,
                depths: &d_new,
                lines: None,
            },
        );

        // Delimiter Container Invariance & Inner Salience Admission Gate.
        // Purge any delimiter pair where:
        // 1. The caller expression sits inside incompatible container contexts
        //    (e.g. Paren in `Vec::with_capacity` vs Bracket in `vec![...]`).
        // 2. The container contents share NO common salient tokens across
        //    distinct callers (e.g. `is_none()` vs `is_some_and(|c|
        //    c.is_quoted())`, or `foo(a)` vs `bar(b)`).
        let find_matching_close = |tokens: &[&str], open_idx: usize| -> Option<usize> {
            let cat = classifier.classify(&tokens[open_idx])?;
            let open_id = match cat {
                diffuse_algo::DelimiterCategory::Open(id) => id,
                _ => return None,
            };
            let mut depth = 1usize;
            for (offset, &tok) in tokens[open_idx + 1..].iter().enumerate() {
                match classifier.classify(&tok) {
                    Some(diffuse_algo::DelimiterCategory::Open(id)) if id == open_id => {
                        depth += 1;
                    }
                    Some(diffuse_algo::DelimiterCategory::Close(id)) if id == open_id => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(open_idx + 1 + offset);
                        }
                    }
                    _ => {}
                }
            }
            None
        };

        let mut purged_indices = Vec::new();
        for (i, &(o1, n1)) in sorted_pairs.iter().enumerate() {
            if let Some(diffuse_algo::DelimiterCategory::Open(id_old)) =
                classifier.classify(&ot[o1])
                && let Some(diffuse_algo::DelimiterCategory::Open(id_new)) =
                    classifier.classify(&nt[n1])
                && id_old == id_new
            {
                let close_old = find_matching_close(&ot, o1);
                let close_new = find_matching_close(&nt, n1);
                if let (Some(o2), Some(n2)) = (close_old, close_new)
                    && let Some(j) = sorted_pairs.iter().position(|&(o, n)| o == o2 && n == n2)
                {
                    let prev_caller_a = (0..o1).rev().find(|&idx| !token_is_ws(ot[idx]));
                    let prev_caller_b = (0..n1).rev().find(|&idx| !token_is_ws(nt[idx]));
                    let has_caller_a =
                        prev_caller_a.is_some_and(|pa| crate::lcs::is_subword_token(ot[pa]));
                    let has_caller_b =
                        prev_caller_b.is_some_and(|pb| crate::lcs::is_subword_token(nt[pb]));

                    if has_caller_a && has_caller_b {
                        let pa = prev_caller_a.unwrap();
                        let pb = prev_caller_b.unwrap();

                        let is_empty_old = o2 == o1 + 1;
                        let is_empty_new = n2 == n1 + 1;

                        if is_empty_old && is_empty_new {
                            // Condition A: Incompatible caller containers for
                            // empty delimiter pairs
                            let c_caller_a =
                                c_old[pa].and_then(crate::token::ContainerKind::from_id);
                            let c_caller_b =
                                c_new[pb].and_then(crate::token::ContainerKind::from_id);
                            if !crate::token::ContainerKind::is_compatible_with(
                                c_caller_a, c_caller_b,
                            ) {
                                purged_indices.push(i);
                                purged_indices.push(j);
                                continue;
                            }
                        } else if is_empty_old != is_empty_new {
                            // Condition B: An empty delimiter pair cannot match
                            // against a non-empty container across distinct
                            // call sites. Function declarations (`fn
                            // foo(...)` vs `fn bar()`) share formal
                            // parameter list boundaries and are exempt.
                            let is_fn_decl = |tokens: &[&str], subword_idx: usize| -> bool {
                                let mut idx = subword_idx;
                                while idx > 0 && crate::lcs::is_subword_token(tokens[idx - 1]) {
                                    idx -= 1;
                                }
                                let prev = (0..idx).rev().find(|&i| !token_is_ws(tokens[i]));
                                prev.is_some_and(|p| tokens[p] == "fn")
                            };

                            let is_fn_decl_a = is_fn_decl(&ot, pa);
                            let is_fn_decl_b = is_fn_decl(&nt, pb);

                            if !(is_fn_decl_a && is_fn_decl_b) {
                                let same_caller =
                                    ot[pa] == nt[pb] && sorted_pairs.contains(&(pa, pb));
                                if !same_caller {
                                    purged_indices.push(i);
                                    purged_indices.push(j);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Depth Discordance & Bilateral Grounding Gate.
        // Purge any matched pair at discordant Dyck container depths across
        // distinct enclosing callers that lacks external supporting matches.
        for (i, &(oi, ni)) in sorted_pairs.iter().enumerate() {
            if token_is_ws(ot[oi]) || classifier.classify(&ot[oi]).is_some() {
                continue;
            }
            let is_quoted = c_old[oi]
                .and_then(crate::token::ContainerKind::from_id)
                .is_some_and(|c| c.is_quoted())
                || c_new[ni]
                    .and_then(crate::token::ContainerKind::from_id)
                    .is_some_and(|c| c.is_quoted());
            if is_quoted {
                continue;
            }

            let depth_discordant = d_old[oi] != d_new[ni]
                && d_old[oi] >= 1
                && d_new[ni] >= 1
                && crate::token::get_enclosing_caller(&ot, oi)
                    != crate::token::get_enclosing_caller(&nt, ni);

            if depth_discordant {
                let mut left_bound_a = oi;
                while left_bound_a > 0
                    && (crate::lcs::is_subword_token(ot[left_bound_a - 1])
                        || ot[left_bound_a - 1] == "::")
                {
                    left_bound_a -= 1;
                }
                let mut left_bound_b = ni;
                while left_bound_b > 0
                    && (crate::lcs::is_subword_token(nt[left_bound_b - 1])
                        || nt[left_bound_b - 1] == "::")
                {
                    left_bound_b -= 1;
                }

                let mut right_bound_a = oi + 1;
                while right_bound_a < ot.len()
                    && (crate::lcs::is_subword_token(ot[right_bound_a])
                        || ot[right_bound_a] == "::")
                {
                    right_bound_a += 1;
                }
                let mut right_bound_b = ni + 1;
                while right_bound_b < nt.len()
                    && (crate::lcs::is_subword_token(nt[right_bound_b])
                        || nt[right_bound_b] == "::")
                {
                    right_bound_b += 1;
                }

                let is_support_token = |tok: &str| {
                    !token_is_ws(tok)
                        && classifier.classify(&tok).is_none()
                        && (crate::lcs::token_is_alnum(tok)
                            || profile.is_some_and(|p| p.is_operator_anchor(tok)))
                        && !crate::token::is_function_word(tok)
                };

                let left_supported = sorted_pairs
                    .iter()
                    .any(|&(o, n)| o < left_bound_a && n < left_bound_b && is_support_token(ot[o]));
                let right_supported = sorted_pairs.iter().any(|&(o, n)| {
                    o >= right_bound_a && n >= right_bound_b && is_support_token(ot[o])
                });

                if !left_supported && !right_supported {
                    purged_indices.push(i);
                }
            }
        }

        if !purged_indices.is_empty() {
            purged_indices.sort_unstable();
            purged_indices.dedup();
            for &idx in purged_indices.iter().rev() {
                sorted_pairs.remove(idx);
            }
        }

        let token_weight = |tokens: &[&str], idx: usize| -> f64 {
            let t = tokens[idx];
            if !token_is_ws(t) {
                char_len(t) as f64
            } else {
                classify_whitespace(tokens, idx).token_weight(char_len(t))
            }
        };
        let matched_chars: f64 = sorted_pairs
            .iter()
            .map(|&(i, j)| {
                let w_old = token_weight(&ot, i);
                let w_new = token_weight(&nt, j);
                w_old.min(w_new)
            })
            .sum();
        let new_weight: f64 = (0..nt.len()).map(|j| token_weight(&nt, j)).sum();
        let ratio = if new_weight > 0.0 {
            matched_chars / new_weight
        } else {
            0.0
        };
        span.record("matched_chars", matched_chars);
        span.record("ratio", ratio);

        if ratio < 0.25 && old_text.trim() != new_text.trim() {
            // Anchors were skipped on the pure-prefix/suffix fast paths;
            // compute them only now that the fallback test actually needs
            // `has_anchors` (reordered short-circuit, identical outcome).
            let has_anchors = match anchors {
                Some(ref computed) => !computed.is_empty(),
                None => !unique_token_anchors(&ot, &nt, profile).is_empty(),
            };
            if !has_anchors {
                tracing::debug!(
                    ratio,
                    "Subword match ratio below threshold, falling back to whole-line diff"
                );
                if !old_text.is_empty() {
                    tape.push(TapeChunk::Strike(old_text));
                }
                if !new_text.is_empty() {
                    tape.push(TapeChunk::Fill(new_text));
                }
                return tape;
            }
        }

        let mut old_tok_idx = 0usize;
        let mut new_tok_idx = 0usize;

        let emit_gap = |tape: &mut AlignmentTape<'a>,
                        old_from: usize,
                        old_to: usize,
                        new_from: usize,
                        new_to: usize| {
            let has_old = old_to > old_from;
            let has_new = new_to > new_from;

            if has_old
                && has_new
                && (new_to == nt.len() || nt[new_to..].iter().all(|t| token_is_ws(t)))
            {
                let new_tail_opt = (new_from..new_to).rev().find(|&idx| !token_is_ws(nt[idx]));
                if let Some(new_tail_idx) = new_tail_opt {
                    let tail_token = nt[new_tail_idx];
                    let old_match_opt = (old_from..old_to).find(|&idx| ot[idx] == tail_token);
                    if let Some(old_match_idx) = old_match_opt {
                        let mut o_m_start = old_match_idx;
                        let mut n_m_start = new_tail_idx;
                        if o_m_start > old_from
                            && n_m_start > new_from
                            && token_is_ws(ot[o_m_start - 1])
                            && ot[o_m_start - 1] == nt[n_m_start - 1]
                        {
                            o_m_start -= 1;
                            n_m_start -= 1;
                        }
                        let o_m_end = old_match_idx + 1;
                        let n_m_end = new_tail_idx + 1;

                        let old_keep =
                            char_slice(old_text, o_spans[o_m_start].0, o_spans[o_m_end - 1].1);
                        let new_keep =
                            char_slice(new_text, n_spans[n_m_start].0, n_spans[n_m_end - 1].1);

                        if old_keep == new_keep && !old_keep.is_empty() {
                            if o_m_start > old_from {
                                let strike_before = char_slice(
                                    old_text,
                                    o_spans[old_from].0,
                                    o_spans[o_m_start - 1].1,
                                );
                                if !strike_before.is_empty() {
                                    tape.push(TapeChunk::Strike(strike_before));
                                }
                            }
                            if n_m_start > new_from {
                                let fill_before = char_slice(
                                    new_text,
                                    n_spans[new_from].0,
                                    n_spans[n_m_start - 1].1,
                                );
                                if !fill_before.is_empty() {
                                    tape.push(TapeChunk::Fill(fill_before));
                                }
                            }

                            tape.push(TapeChunk::Keep(new_keep));

                            if old_to > o_m_end {
                                let strike_after =
                                    char_slice(old_text, o_spans[o_m_end].0, o_spans[old_to - 1].1);
                                if !strike_after.is_empty() {
                                    tape.push(TapeChunk::Strike(strike_after));
                                }
                            }
                            if new_to > n_m_end {
                                let fill_after =
                                    char_slice(new_text, n_spans[n_m_end].0, n_spans[new_to - 1].1);
                                if !fill_after.is_empty() {
                                    tape.push(TapeChunk::Fill(fill_after));
                                }
                            }
                            return;
                        }
                    }
                }
            }

            if has_old {
                let o_slice = char_slice(old_text, o_spans[old_from].0, o_spans[old_to - 1].1);
                tape.push(TapeChunk::Strike(o_slice));
            }
            if has_new {
                let n_slice = char_slice(new_text, n_spans[new_from].0, n_spans[new_to - 1].1);
                tape.push(TapeChunk::Fill(n_slice));
            }
        };

        for &(oi, ni) in &sorted_pairs {
            if old_tok_idx < oi || new_tok_idx < ni {
                emit_gap(&mut tape, old_tok_idx, oi, new_tok_idx, ni);
            }
            tape.push(TapeChunk::Keep(nt[ni]));
            old_tok_idx = oi + 1;
            new_tok_idx = ni + 1;
        }

        if old_tok_idx < ot.len() || new_tok_idx < nt.len() {
            emit_gap(&mut tape, old_tok_idx, ot.len(), new_tok_idx, nt.len());
        }

        crate::ensure!(
            tape.project_old() == old_text,
            "AlignmentTape project_old must equal old_text: expected {:?}, got {:?}",
            old_text,
            tape.project_old()
        );
        crate::ensure!(
            tape.project_new() == new_text,
            "AlignmentTape project_new must equal new_text: expected {:?}, got {:?}",
            new_text,
            tape.project_new()
        );

        tape
    }
}
