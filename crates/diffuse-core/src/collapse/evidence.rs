//! Pair evidence: the one content scale shared by correspondence and typing.
//!
//! [`pair_evidence`] is the union of the two evidence functions the engine used
//! to run separately: the micro-DP's `match_score` and the cluster graph's
//! `calculate_edge_explanation_ratio`. Both are retired into it. It scores
//! *content only*: no term here reads a line position, a row index, or a
//! diagonal distance, because displacement is priced by the solver's gap cost
//! (`PATH_GAP_COST`) and not by admissibility.
//!
//! The scoring currency is matched content characters: `MATCHED_CHAR_REWARD`
//! points per character explained by the pair. Every other term is sized in
//! that unit, so "a pair worth `k` characters may bridge `k` rows" is a
//! statement about the constants below.

use diffuse_syntax::{prelude::first_word, profile::LangProfile};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    collapse::{
        frame::robust_delim_delta,
        residual::{declaration_role, has_verbatim_rival},
    },
    lcs::{
        for_each_token, is_affix_pair, is_token_salient_in_context, line_has_salient_token,
        merged_runs_for_texts_with_profile, tokenize_line,
    },
    merge::{Run, RunKind},
};

/// Evidence of a pair of all-blank lines.
///
/// A blank line carries no content, so its pairing is a shape decision that
/// may not justify displacement: the weight is below one row of gap
/// (`PATH_GAP_COST`), which keeps a blank pair at offset zero without letting
/// it bridge a gap.
pub const BLANK_PAIR_WEIGHT: i64 = 5;

/// Evidence per content character the pair explains.
pub const MATCHED_CHAR_REWARD: i64 = 10;
/// Evidence charged per character the pair leaves unexplained.
pub const UNMATCHED_CHAR_PENALTY: i64 = 3;
/// Evidence charged per column of indentation difference.
pub const INDENT_PENALTY: i64 = 2;
/// Evidence of an exact whole-line match, on top of its characters.
pub const EXACT_MATCH_BASE: i64 = 1000;
/// Credit for an affix pair, before coverage scaling.
///
/// An affix pair explains only its shared prefix/suffix, so the credit is
/// multiplied by `smaller / larger`: a fragment carrying one token out of a
/// long line earns a fragment's credit, not a pair's.
pub const AFFIX_CREDIT: f64 = 900.0;
/// Reward for agreeing statement heads.
pub const HEAD_AGREE_BONUS: i64 = 80;
/// Reward for agreeing terminators on an agreeing head (`{`/`;`).
pub const TERMINATOR_AGREE_BONUS: i64 = 40;
/// Reward for an identical assignment target or field key.
pub const KEY_AGREE_BONUS: i64 = 200;
/// Reward for an identical prefix at equal indentation that opens a container.
pub const CONTAINER_OPEN_BONUS: i64 = 200;
/// Reward for the head line of a block that replaces one line.
///
/// A reflowed statement keeps its head at the original indentation and pushes
/// the payload onto the following lines. The head is then a prefix of the
/// replaced line, or an opener continuing its statement. It explains fewer
/// characters than the deepest payload row, so on content alone the
/// continuation would win the correspondence; the head is nonetheless the
/// line the block belongs to.
pub const HEAD_LINE_BONUS: i64 = 200;

/// Floor on matched content characters for a pair that shares no statement
/// head. Four characters is one short identifier, the smallest unit of
/// evidence that can name the same thing twice.
const MIN_MATCHED_SALIENT_CHARS: usize = 4;
/// Floor on the matched fraction of the smaller side. A pair must explain at
/// least this much of the line it is replacing to count as a modification.
const MIN_MATCHED_SALIENT_RATIO: f64 = 0.30;
/// Floor on surviving non-whitespace context relative to the larger side for a
/// multi-island (`>= 2` edit spans) modification without an agreeing field or
/// assignment key.
const MIN_MULTI_ISLAND_LARGER_RATIO: f64 = 0.25;
/// Companion floor relative to the smaller side so genuine 1-to-N expansion
/// heads (which cover a large fraction of the shorter line despite being a
/// small fraction of the longer one) are preserved.
const MIN_MULTI_ISLAND_SMALLER_RATIO: f64 = 0.45;

/// Precomputed per-row metadata shared across all candidate pairs scored in a
/// cluster so tokenization, salient checks, delimiter deltas, and snapshot
/// rival lookups run once per row (`O(n + m)`) instead of once per cell.
pub(crate) struct PreparedRow<'a> {
    pub(crate) text: &'a str,
    pub(crate) trimmed: &'a str,
    indent: usize,
    char_count: usize,
    is_comment: bool,
    has_alnum: bool,
    has_salient: bool,
    has_rival: bool,
    pub(crate) head: Option<&'a str>,
    pub(crate) assignment: Option<&'a str>,
    pub(crate) field: Option<&'a str>,
    delim_delta: i32,
    salient_counts: FxHashMap<&'a str, (usize, usize)>,
    pub(crate) salient_set: FxHashSet<&'a str>,
    salient_chars: usize,
}

impl<'a> PreparedRow<'a> {
    /// Builds precomputed evidence metadata for one cluster row using a
    /// prebuilt set of trimmed opposite-side snapshot lines for rival checks.
    pub(crate) fn from_cluster_row(
        text: &'a str,
        opposite_trimmed: Option<&FxHashSet<&str>>,
        profile: Option<&LangProfile>,
    ) -> Self {
        let trimmed = text.trim();
        let has_rival = !trimmed.is_empty()
            && declaration_role(trimmed).is_some()
            && opposite_trimmed.is_some_and(|set| set.contains(trimmed));
        Self::build(text, trimmed, has_rival, profile)
    }

    fn from_single_text(
        text: &'a str,
        opposite_content: Option<&[&str]>,
        profile: Option<&LangProfile>,
    ) -> Self {
        let trimmed = text.trim();
        let has_rival = !trimmed.is_empty()
            && declaration_role(trimmed).is_some()
            && has_verbatim_rival(trimmed, opposite_content);
        Self::build(text, trimmed, has_rival, profile)
    }

    fn build(
        text: &'a str,
        trimmed: &'a str,
        has_rival: bool,
        profile: Option<&LangProfile>,
    ) -> Self {
        if trimmed.is_empty() {
            return Self {
                text,
                trimmed,
                indent: indent_of(text),
                char_count: 0,
                is_comment: false,
                has_alnum: false,
                has_salient: false,
                has_rival: false,
                head: None,
                assignment: None,
                field: None,
                delim_delta: 0,
                salient_counts: FxHashMap::default(),
                salient_set: FxHashSet::default(),
                salient_chars: 0,
            };
        }

        let tokens: Vec<&'a str> = tokenize_line(text).collect();
        let mut salient_counts: FxHashMap<&'a str, (usize, usize)> = FxHashMap::default();
        let mut salient_set = FxHashSet::default();
        let mut salient_chars = 0usize;
        for (i, &token) in tokens.iter().enumerate() {
            if is_token_salient_in_context(&tokens, i, profile) {
                let len = token.chars().count();
                salient_counts
                    .entry(token)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, len));
                salient_set.insert(token);
                salient_chars += len;
            }
        }

        Self {
            text,
            trimmed,
            indent: indent_of(text),
            char_count: trimmed.chars().count(),
            is_comment: is_comment(text, profile),
            has_alnum: trimmed.chars().any(char::is_alphanumeric),
            has_salient: line_has_salient_token(text, profile),
            has_rival,
            head: first_word(trimmed).filter(|w| is_statement_head_keyword(w, profile)),
            assignment: assignment_target(trimmed),
            field: field_label(trimmed),
            delim_delta: robust_delim_delta(trimmed),
            salient_counts,
            salient_set,
            salient_chars,
        }
    }
}

/// Evidence that the pair is admissible and how much it explains, in matched
/// content characters. `None` means the two lines must not correspond.
pub fn pair_evidence(
    u_text: &str,
    v_text: &str,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Option<i64> {
    let u = PreparedRow::from_single_text(u_text, new_content, profile);
    let v = PreparedRow::from_single_text(v_text, old_content, profile);
    pair_evidence_prepared(&u, &v, profile)
}

/// Scores a pair of precomputed cluster rows using a single borrowed alignment
/// tape, rejecting compound-gate failures before building any tape.
pub(crate) fn pair_evidence_prepared(
    u: &PreparedRow<'_>,
    v: &PreparedRow<'_>,
    profile: Option<&LangProfile>,
) -> Option<i64> {
    if u.trimmed.is_empty() && v.trimmed.is_empty() {
        return Some(BLANK_PAIR_WEIGHT);
    }
    if u.trimmed.is_empty() || v.trimmed.is_empty() {
        return None;
    }
    if u.is_comment != v.is_comment {
        return None;
    }

    // A pure-punctuation line (zero alphanumeric characters) must not pair with
    // a line that carries salient identifiers (e.g. `(` vs `x(` or `{` vs
    // `fn foo() {`). Structural delimiter-to-keyword transitions such as `}` vs
    // `} else {` remain eligible because keywords are not salient.
    if (!u.has_alnum && v.has_salient) || (!v.has_alnum && u.has_salient) {
        return None;
    }

    let indent_cost = (u.indent.abs_diff(v.indent) as i64) * INDENT_PENALTY;

    if u.trimmed == v.trimmed {
        return Some(u.char_count as i64 * MATCHED_CHAR_REWARD + EXACT_MATCH_BASE - indent_cost);
    }

    // A declaration that still exists verbatim on the opposite side is a
    // relocation candidate owned by the move pre-pass; pairing it as a
    // modification would steal that line. Running this before alignment also
    // avoids building an LCS tape for vetoed declaration pairs.
    if u.has_rival || v.has_rival {
        return None;
    }

    let head_agrees = u.head.is_some() && u.head == v.head;
    let key_agrees = (u.assignment.is_some() && u.assignment == v.assignment)
        || (u.field.is_some() && u.field == v.field);
    let head_link = head_agrees || key_agrees;

    // Pre-tape compound token-bag gate: without a shared statement head,
    // assignment target, or field label, a pair carrying salient identifiers on
    // both sides must share at least one short identifier's worth of salient
    // content covering the minimum fraction of the smaller side. Checking this
    // on precomputed token tables skips building an alignment tape for most
    // unrelated candidate pairs.
    let min_salient = u.salient_chars.min(v.salient_chars);
    if !head_link && u.has_salient && v.has_salient && min_salient > 0 {
        let (smaller, larger) = if u.salient_counts.len() <= v.salient_counts.len() {
            (&u.salient_counts, &v.salient_counts)
        } else {
            (&v.salient_counts, &u.salient_counts)
        };
        let matched_salient: usize = smaller
            .iter()
            .filter_map(|(&token, &(count_a, len))| {
                let &(count_b, _) = larger.get(token)?;
                Some(count_a.min(count_b) * len)
            })
            .sum();
        if matched_salient < MIN_MATCHED_SALIENT_CHARS
            || (matched_salient as f64 / min_salient as f64) < MIN_MATCHED_SALIENT_RATIO
        {
            return None;
        }
    }

    // Build one borrowed profiled alignment tape (no `WordSpans`, no per-run
    // `String` clones) and reuse it for wholesale rejection, admissibility,
    // matched-character scoring, and salient context checks.
    let runs = merged_runs_for_texts_with_profile(u.text, v.text, profile);
    let matched = matched_chars(&runs);
    if matched == 0 {
        return None;
    }

    let runs_have_salient_ctx = u.has_salient && v.has_salient && has_salient_ctx(&runs, profile);

    // An affix pair shares a verbatim run carrying salient content, so it is
    // admissible without asking the general predicate. Bare punctuation affixes
    // (`(`, `{`, `;`) must still pass `admissible_from_runs`.
    let shared_affix = (u.trimmed.len() > v.trimmed.len()
        && v.has_salient
        && (u.trimmed.starts_with(v.trimmed) || u.trimmed.ends_with(v.trimmed)))
        || (v.trimmed.len() > u.trimmed.len()
            && u.has_salient
            && (v.trimmed.starts_with(u.trimmed) || v.trimmed.ends_with(u.trimmed)));
    if !(shared_affix
        || admissible_from_runs_prechecked(
            u.text,
            v.text,
            &runs,
            u.has_salient,
            v.has_salient,
            runs_have_salient_ctx,
            profile,
        ))
    {
        return None;
    }

    if !head_link && u.has_salient && v.has_salient && !runs_have_salient_ctx {
        return None;
    }

    let smaller = u.char_count.min(v.char_count);
    let larger = u.char_count.max(v.char_count);

    let affix = is_affix_pair(u.trimmed, v.trimmed);
    let raw_content = if affix {
        smaller as i64 * MATCHED_CHAR_REWARD - (larger - smaller) as i64 * UNMATCHED_CHAR_PENALTY
    } else {
        matched as i64 * MATCHED_CHAR_REWARD
            - larger.saturating_sub(matched) as i64 * UNMATCHED_CHAR_PENALTY
    };

    let equal_indent = u.indent == v.indent;
    let is_prefix =
        equal_indent && (u.trimmed.starts_with(v.trimmed) || v.trimmed.starts_with(u.trimmed));
    let block_head_stem = equal_indent
        .then(|| {
            block_head_prefix_len(u.trimmed, v.trimmed, profile)
                .or_else(|| block_head_prefix_len(v.trimmed, u.trimmed, profile))
        })
        .flatten();
    let is_chain_head = is_prefix
        && !u.is_comment
        && !v.is_comment
        && u.delim_delta == 0
        && v.delim_delta == 0
        && u.has_salient
        && v.has_salient
        && profile.is_some_and(|p| !p.continuation_prefixes.is_empty());

    // Post-tape salient coverage & multi-island fragmentation gate:
    // 1. Re-enforce the salient floor on surviving `Ctx` runs (catching cases
    //    where the pre-tape bag matched subwords that the alignment tape
    //    refused to align across unrelated compound identifiers).
    // 2. Reject multi-island (`>= 2` edit spans) replacements without an
    //    agreeing field/assignment key when surviving anchors are overwhelmed
    //    by edits (preferring clean `Del` + `Add` over fragmented confetti).
    if u.has_salient
        && v.has_salient
        && min_salient > 0
        && !affix
        && block_head_stem.is_none()
        && !is_chain_head
    {
        let ctx_sal = ctx_salient_chars(&runs, profile);
        let min_sal_floor = MIN_MATCHED_SALIENT_CHARS.min(min_salient);
        if !head_link
            && (ctx_sal < min_sal_floor
                || (ctx_sal as f64 / min_salient as f64) < MIN_MATCHED_SALIENT_RATIO)
        {
            return None;
        }
        // Even with an agreeing statement-head keyword (`if`, `for`, `fn`,
        // `let`), a line with zero surviving salient context whose matched
        // scaffolding cannot even cover its own unmatched penalty
        // (`raw_content <= 0`) is an unrelated statement replacement rather
        // than an edit.
        if !key_agrees && ctx_sal == 0 && raw_content <= 0 {
            return None;
        }
        if !key_agrees && count_edit_islands(&runs) >= 2 {
            let nws_ctx = matched_non_ws_chars(&runs);
            let low_coverage = (nws_ctx as f64 / larger.max(1) as f64)
                < MIN_MULTI_ISLAND_LARGER_RATIO
                && (nws_ctx as f64 / smaller.max(1) as f64) < MIN_MULTI_ISLAND_SMALLER_RATIO;
            if ctx_sal < min_sal_floor || low_coverage {
                return None;
            }
        }
    }

    // A headless wrapper stub (e.g. `assert_eq!(` or `Some(` against a full
    // statement) that leaves more of the line unexplained than its own content
    // justifies (`raw_content <= 0`) belongs to the enclosing-frame layer
    // (`reflow.rs`), not to line correspondence: granting affix or opener
    // bonuses to it would let the path solver split one expanded statement
    // across two adjacent lines. Equal-indent brace block heads (e.g.
    // `.map(|(i, s)| {`) and non-delimited chain heads (`!runs`) are genuine
    // statement heads rather than enclosing-frame stubs.
    if !head_link && block_head_stem.is_none() && !is_chain_head && raw_content <= 0 {
        return None;
    }

    let mut score = if affix {
        let coverage = smaller as f64 / larger.max(1) as f64;
        raw_content + (AFFIX_CREDIT * coverage) as i64 - indent_cost
    } else if let Some(stem_len) = block_head_stem {
        let coverage = stem_len as f64 / larger.max(1) as f64;
        raw_content + (AFFIX_CREDIT * coverage) as i64 - indent_cost
    } else {
        raw_content - indent_cost
    };

    if head_agrees {
        score += HEAD_AGREE_BONUS;
        let same_terminator = (u.trimmed.ends_with('{') && v.trimmed.ends_with('{'))
            || (u.trimmed.ends_with(';') && v.trimmed.ends_with(';'));
        if same_terminator {
            score += TERMINATOR_AGREE_BONUS;
        }
    }
    if key_agrees {
        score += KEY_AGREE_BONUS;
    }
    if equal_indent {
        let opens_container = u.delim_delta > 0 || v.delim_delta > 0;
        if (is_prefix || block_head_stem.is_some()) && opens_container {
            score += CONTAINER_OPEN_BONUS;
        } else if is_prefix || (opens_container && head_agrees) {
            score += HEAD_LINE_BONUS;
        }
    }

    (score > 0).then_some(score)
}

/// Whether `del_text` and `add_text` are admissible as a modification pair.
#[cfg(test)]
pub(crate) fn is_admissible_mod_pair(
    del_text: &str,
    add_text: &str,
    profile: Option<&LangProfile>,
) -> bool {
    if del_text == add_text {
        return true;
    }

    let runs = merged_runs_for_texts_with_profile(del_text, add_text, profile);
    admissible_from_runs(del_text, add_text, &runs, profile)
}

/// The admissibility rule over a borrowed alignment that the caller has
/// already built.
#[cfg(test)]
pub(crate) fn admissible_from_runs(
    del_text: &str,
    add_text: &str,
    runs: &[Run<'_>],
    profile: Option<&LangProfile>,
) -> bool {
    if del_text == add_text {
        return true;
    }

    let del_has_salient = line_has_salient_token(del_text, profile);
    let add_has_salient = line_has_salient_token(add_text, profile);
    let runs_have_salient_ctx =
        del_has_salient && add_has_salient && has_salient_ctx(runs, profile);

    admissible_from_runs_prechecked(
        del_text,
        add_text,
        runs,
        del_has_salient,
        add_has_salient,
        runs_have_salient_ctx,
        profile,
    )
}

fn admissible_from_runs_prechecked(
    del_text: &str,
    add_text: &str,
    runs: &[Run<'_>],
    del_has_salient: bool,
    add_has_salient: bool,
    runs_have_salient_ctx: bool,
    profile: Option<&LangProfile>,
) -> bool {
    if del_has_salient && add_has_salient {
        if !runs_have_salient_ctx {
            // Safety fallback: if neither line shares any salient context,
            // check whether this is an entity declaration statement (let, fn,
            // const, def, struct, …). Declaration keywords do not confer
            // statement identity when both the declared entity name and value
            // expressions are replaced, unless common context accounts for the
            // majority (>= 50%) of the line.
            let is_decl_head = runs.first().is_some_and(|run| {
                if run.kind != RunKind::Ctx {
                    return false;
                }
                let mut found = false;
                for_each_token(run.text, |t| {
                    if !found && profile.is_some_and(|p| p.is_declaration_keyword(t)) {
                        found = true;
                    }
                });
                found
            });
            if is_decl_head {
                let ctx_chars = matched_chars(runs);
                let total_chars = del_text
                    .trim()
                    .chars()
                    .count()
                    .max(add_text.trim().chars().count());
                if ctx_chars * 2 < total_chars {
                    return false;
                }
            }
        }
    } else {
        // Context must not be purely whitespace
        let has_non_ws_ctx = runs
            .iter()
            .any(|run| run.kind == RunKind::Ctx && !run.text.trim().is_empty());
        if !has_non_ws_ctx && (!del_text.trim().is_empty() || !add_text.trim().is_empty()) {
            return false;
        }
    }
    true
}

/// Whether `word` is a statement-head keyword in the active language profile.
pub(crate) fn is_statement_head_keyword(word: &str, profile: Option<&LangProfile>) -> bool {
    profile.is_some_and(|p| p.is_statement_head(word))
}

/// The assignment target before `=` (`key = …`), when the line has one.
pub(crate) fn assignment_target(trimmed: &str) -> Option<&str> {
    let key = trimmed[..trimmed.find('=')?].trim();
    (!key.is_empty()).then_some(key)
}

/// The field label before `:` (`key: value`), when the line has a single-word
/// label rather than a prose clause.
pub(crate) fn field_label(trimmed: &str) -> Option<&str> {
    let key = trimmed[..trimmed.find(':')?].trim();
    (!key.is_empty() && !key.contains(' ')).then_some(key)
}

/// When `head` opens a brace block (`... {`) whose salient stem before `{` is
/// a strict prefix of `single_line` (e.g. `Row::Del(x) => {` vs
/// `Row::Del(x) => expr`), returns the stem's character length.
fn block_head_prefix_len(
    head: &str,
    single_line: &str,
    profile: Option<&LangProfile>,
) -> Option<usize> {
    let stem = head.strip_suffix('{')?.trim_end();
    if stem.is_empty()
        || single_line.len() <= stem.len()
        || !single_line.starts_with(stem)
        || !line_has_salient_token(stem, profile)
    {
        return None;
    }
    Some(stem.chars().count())
}

fn indent_of(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

fn is_comment(text: &str, profile: Option<&LangProfile>) -> bool {
    profile.is_some_and(|p| p.is_comment_line(text))
}

/// Characters explained by a set of borrowed runs from a single alignment tape,
/// excluding leading horizontal indentation so matched counts share
/// trimmed-line coordinates with [`PreparedRow::char_count`].
fn matched_chars(runs: &[Run<'_>]) -> usize {
    let mut seen_code = false;
    let mut total = 0usize;
    for run in runs {
        let text = if seen_code {
            run.text
        } else {
            let stripped = run.text.trim_start();
            if !stripped.is_empty() {
                seen_code = true;
            }
            stripped
        };
        if run.kind == RunKind::Ctx && !text.trim().is_empty() {
            total += text.chars().count();
        }
    }
    total
}

fn has_salient_ctx(runs: &[Run<'_>], profile: Option<&LangProfile>) -> bool {
    runs.iter()
        .any(|run| run.kind == RunKind::Ctx && line_has_salient_token(run.text, profile))
}

/// Counts salient token characters surviving inside `Ctx` runs of an alignment
/// tape, using the same tokenization and context-aware salience rules as
/// [`PreparedRow`].
fn ctx_salient_chars(runs: &[Run<'_>], profile: Option<&LangProfile>) -> usize {
    let mut total = 0usize;
    for run in runs {
        if run.kind != RunKind::Ctx {
            continue;
        }
        let tokens: Vec<&str> = tokenize_line(run.text).collect();
        for (i, &tok) in tokens.iter().enumerate() {
            if is_token_salient_in_context(&tokens, i, profile) {
                total += tok.chars().count();
            }
        }
    }
    total
}

/// Counts non-whitespace characters surviving in `Ctx` runs.
fn matched_non_ws_chars(runs: &[Run<'_>]) -> usize {
    runs.iter()
        .filter(|run| run.kind == RunKind::Ctx)
        .map(|run| run.text.chars().filter(|c| !c.is_whitespace()).count())
        .sum()
}

/// Counts maximal contiguous non-`Ctx` edit regions separated by non-whitespace
/// `Ctx` anchors.
fn count_edit_islands(runs: &[Run<'_>]) -> usize {
    let mut islands = 0usize;
    let mut in_edit = false;
    for run in runs {
        if run.kind == RunKind::Ctx {
            if !run.text.trim().is_empty() {
                in_edit = false;
            }
        } else if !in_edit {
            islands += 1;
            in_edit = true;
        }
    }
    islands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_punctuation_affix_does_not_pair_with_unrelated_statement() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        assert!(
            pair_evidence("    }", "    };", None, None, rs).is_some(),
            "closing brace with semicolon still pairs via admissibility"
        );
        assert!(
            pair_evidence("    (", "    x(", None, None, rs).is_none(),
            "bare punctuation prefix must not bypass admissibility to pair with identifier"
        );
    }

    #[test]
    fn statement_head_uses_lang_profile() {
        let py = diffuse_syntax::registry::lang_profile("py");
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Under Python profile, `def` is a statement head so `def alpha():` and
        // `def beta():` share a head link (scoring higher than under Rust where
        // `def` is an ordinary identifier).
        let py_score = pair_evidence("def alpha(x):", "def beta(x):", None, None, py)
            .expect("admissible under py");
        let rs_score = pair_evidence("def alpha(x):", "def beta(x):", None, None, rs)
            .expect("admissible under rs");
        assert!(
            py_score > rs_score,
            "Python `def` must receive statement-head bonus under py profile: py={py_score}, rs={rs_score}"
        );
    }

    #[test]
    fn head_line_bonus_is_not_double_counted_for_prefix_openers() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Prefix opener at equal indent (`let x = foo(` is a prefix of
        // `let x = foo(a, b);` and opens `(` with agreeing `let` head):
        // HEAD_LINE_BONUS must apply at most once, not twice.
        let prefix_score =
            pair_evidence("    let x = foo(a, b);", "    let x = foo(", None, None, rs)
                .expect("admissible");
        // Base affix score (~12*10 - 6*3 + 900*(12/18) = 702) + HEAD_AGREE (80)
        // + CONTAINER_OPEN (200) + single HEAD_LINE (200) = ~1182.
        // Double-counting HEAD_LINE_BONUS pushes it to ~1382.
        assert!(
            prefix_score < 1300,
            "HEAD_LINE_BONUS must not be double-counted, got {prefix_score}"
        );
    }

    #[test]
    fn admissible_mod_pair_handles_identical_decl_head_and_whitespace_only() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        assert!(is_admissible_mod_pair("let x = 1;", "let x = 1;", rs));
        assert!(admissible_from_runs("let x = 1;", "let x = 1;", &[], rs));
        // Declaration keyword alone with completely replaced name and value (<
        // 50% ctx)
        assert!(!is_admissible_mod_pair(
            "let alpha_name = compute_first_value();",
            "let beta_title = derive_second_result();",
            rs
        ));
        // Declaration head where common context is >= 50% of the line remains
        // admissible
        assert!(is_admissible_mod_pair("let a = ();", "let b = ();", rs));
        // Salient-free lines sharing only whitespace are rejected
        assert!(!is_admissible_mod_pair("    {", "    )", rs));
        assert!(is_admissible_mod_pair("    }", "    };", rs));
    }

    #[test]
    fn prepared_row_scoring_matches_standalone_pair_evidence() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old_snap = ["fn keep_me() {", "    let value = 1;"];
        let new_snap = ["fn keep_me() {", "    let value = 2;"];
        let old_set: FxHashSet<&str> = old_snap.iter().map(|l| l.trim()).collect();
        let new_set: FxHashSet<&str> = new_snap.iter().map(|l| l.trim()).collect();

        let cases = [
            ("", ""),
            ("", "non_empty();"),
            ("// old comment", "let code = 1;"),
            ("    let x = 1;", "        let x = 1;"),
            ("fn keep_me() {", "fn renamed() {"),
            ("    alpha_one(beta);", "    gamma_two(delta);"),
            (
                "    let total = compute(old_arg);",
                "    let total = compute(new_arg);",
            ),
            ("    struct Foo {", "    enum Bar {"),
            (
                "    foo(alpha_long_token);",
                "    foo(alpha_long_token, extra);",
            ),
            ("    }", "    };"),
        ];
        for (u_text, v_text) in cases {
            let direct = pair_evidence(u_text, v_text, Some(&old_snap), Some(&new_snap), rs);
            let u = PreparedRow::from_cluster_row(u_text, Some(&new_set), rs);
            let v = PreparedRow::from_cluster_row(v_text, Some(&old_set), rs);
            let prepared = pair_evidence_prepared(&u, &v, rs);
            assert_eq!(
                prepared, direct,
                "PreparedRow score must match pair_evidence for ({u_text:?}, {v_text:?})"
            );
        }
    }

    #[test]
    fn headless_wrapper_stub_below_raw_content_floor_is_rejected() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // A short headless wrapper prefix (`assert_eq!(`) covering < 23% of a
        // full statement belongs to enclosing-frame segmentation, not to a
        // standalone Diag correspondence.
        assert_eq!(
            pair_evidence(
                "    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 0), 0);",
                "    assert_eq!(",
                None,
                None,
                rs,
            ),
            None,
            "tiny headless wrapper stub must not score as a standalone Diag"
        );
        // Genuine substantial head expansions (signature / struct / let head)
        // remain strongly positive.
        assert!(
            pair_evidence(
                "    pub(crate) fn scrollbar_scroll_for_row(track_h: usize, max_scroll: usize, row: usize) -> usize {",
                "    pub(crate) fn scrollbar_scroll_for_row(",
                None,
                None,
                rs,
            )
            .is_some()
        );
        assert!(
            pair_evidence(
                "    MouseScrollUp(usize),",
                "    MouseScrollUp {",
                None,
                None,
                rs,
            )
            .is_some()
        );
    }

    #[test]
    fn indentation_depth_does_not_inflate_matched_content_or_decl_guard() {
        let rs = diffuse_syntax::registry::lang_profile("rs");

        // Equal-indent lines must score identically at 0 and 16 spaces of
        // indent.
        let unindented = pair_evidence("foo(alpha, beta);", "foo(alpha, gamma);", None, None, rs);
        let indented = pair_evidence(
            "                foo(alpha, beta);",
            "                foo(alpha, gamma);",
            None,
            None,
            rs,
        );
        assert_eq!(
            indented, unindented,
            "leading indentation must not inflate matched_chars"
        );

        // Fragmented method-call replacement with only `self.(...n...);` shared
        // must be rejected regardless of indentation depth.
        for pad in ["", "                ", "                                "] {
            let old = format!("{pad}self.scroll_to_visual(self.scroll.saturating_sub(n));");
            let new = format!("{pad}self.on_mouse_scroll(true, n, row, col);");
            assert_eq!(
                pair_evidence(&old, &new, None, None, rs),
                None,
                "indentation ({} cols) must not subsidize fragmented method replacement",
                pad.len()
            );
        }

        // Deeply indented unrelated `let` declarations must fail the 50%
        // context guard just like unindented ones.
        assert!(!is_admissible_mod_pair(
            "                    let top = stack.pop();",
            "                    let mut out = vec![0u8; items.len()];",
            rs,
        ));
    }

    #[test]
    fn pre_tape_salient_bag_caps_repeated_token_multiplicity() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // `foo` appears 4 times on `u` (12 chars if uncapped) but only once on
        // `v` (3 chars out of 34 salient chars = 8.8% < 30%). Capping by
        // multiset intersection rejects both directions symmetrically.
        let u = "foo(foo, foo, foo, alpha_one, beta_two);";
        let v = "foo(gamma_three, delta_four, eps_five);";
        assert_eq!(pair_evidence(u, v, None, None, rs), None);
        assert_eq!(pair_evidence(v, u, None, None, rs), None);
    }

    #[test]
    fn equal_indent_block_head_and_chain_head_remain_admissible() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Brace block head (`Row::Del(x) => {`) must beat its deeper-indented
        // body line when replacing a 1-line match arm.
        let one_line = "            Row::Del(del_line) => Row::Add(AddLine::new(del_line.num, del_line.text.clone())),";
        let head_score = pair_evidence(
            one_line,
            "            Row::Del(del_line) => {",
            None,
            None,
            rs,
        )
        .expect("brace block head must be admissible");
        let body_score = pair_evidence(
            one_line,
            "                let mut add = AddLine::new(del_line.num, del_line.text.clone());",
            None,
            None,
            rs,
        )
        .expect("body line scores");
        assert!(
            head_score > body_score,
            "brace block head ({head_score}) must beat deeper body ({body_score})"
        );

        // Closure block head (`.map(|(i, s)| {`) and non-delimited chain head
        // (`!runs`) remain admissible even when shorter than 23% of the 1-line
        // form.
        assert!(
            pair_evidence(
                "        .map(|(i, s)| {",
                "        .map(|(i, s)| Row::Del(DelLine::new(i as u32 + 1, s.as_str())))",
                None,
                None,
                rs,
            )
            .is_some()
        );
        assert!(
            pair_evidence(
                "        !runs",
                "        !runs.iter().any(|run| run.kind == RunKind::Ctx)",
                None,
                None,
                rs,
            )
            .is_some()
        );
    }

    #[test]
    fn post_tape_salient_floor_rejects_weak_surviving_anchors() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Pre-tape bag matches `"scroll"`, but the alignment tape rejects it
        // across different method names, leaving only 1-char `"n"` in Ctx.
        assert_eq!(
            pair_evidence(
                "                self.scroll_to_visual(self.scroll + n);",
                "                self.on_mouse_scroll(false, n, row, col);",
                None,
                None,
                rs,
            ),
            None
        );
        // Same method rename WITH shared salient argument stays paired.
        assert!(
            pair_evidence(
                "                self.scroll_to_visual(target_vis);",
                "                self.on_mouse_scroll(target_vis);",
                None,
                None,
                rs,
            )
            .is_some()
        );
        // Compound-identifier rename with strong surviving subword Ctx stays
        // paired.
        assert!(
            pair_evidence(
                "        line_hash: n.line_hash,",
                "        content_hash: n.content_hash,",
                None,
                None,
                rs,
            )
            .is_some()
        );
    }

    #[test]
    fn multi_island_fragmentation_and_decl_compat_reject_confetti_while_preserving_valid_edits() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Unrelated declarations and fragmented multi-island replacements
        // split.
        for (old, new) in [
            (
                "        let top = stack.pop();",
                "        let mut out = vec![0u8; items.len()];",
            ),
            (
                "pub fn note_anchors_cleanly(note: &ReviewNote, line_content: &str) -> bool {",
                "pub fn label(self) -> String {",
            ),
            (
                "fn wholesale_pairs_render_as_baseline_rows() {",
                "fn subword_diff_token_spans() {",
            ),
            (
                "    let thumb_row = header_offset + vh / 2;",
                "    let (thumb_off, _) = AppState::scrollbar_thumb_for(track_h, vh, total, scrolled, header_vis)",
            ),
            (
                "        if m.start() > last {",
                "        if is_ws_char(c) {",
            ),
        ] {
            assert_eq!(
                pair_evidence(old, new, None, None, rs),
                None,
                "expected split for ({old:?}, {new:?})"
            );
        }

        // Valid edits remain paired: short 3-char identifier (`min_salient`
        // clamp), 1-to-N expansion head (`smaller` ratio protection), shared
        // field key (`key_agrees`), and single-island control-flow swap.
        for (old, new) in [
            ("impl Run {", "impl<'a> Run<'a> {"),
            (
                "    while share < dch.len() && share < ach.len() && dch[share] == ach[share] {",
                "    while share < d_bytes.len()",
            ),
            (
                "        location: NoteLocation::line(side_val, start_line, end_line),",
                "        location: provisional_location,",
            ),
            (
                "        if scrollbar_seek {",
                "        if is_on_scrollbar {",
            ),
        ] {
            assert!(
                pair_evidence(old, new, None, None, rs).is_some(),
                "expected pair for ({old:?}, {new:?})"
            );
        }
    }
}
