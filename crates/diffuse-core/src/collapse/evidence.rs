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

use crate::{
    collapse::{
        frame::robust_delim_delta,
        residual::{are_roles_compatible, declaration_role, has_verbatim_rival},
    },
    lcs::{
        for_each_token, is_affix_pair, is_token_salient_in_context, line_has_salient_token,
        tokenize_line, word_spans_and_runs_with_profile,
    },
    merge::RunKind,
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

/// Evidence that the pair is admissible and how much it explains, in matched
/// content characters. `None` means the two lines must not correspond.
pub fn pair_evidence(
    u_text: &str,
    v_text: &str,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Option<i64> {
    let u_trim = u_text.trim();
    let v_trim = v_text.trim();

    if u_trim.is_empty() && v_trim.is_empty() {
        return Some(BLANK_PAIR_WEIGHT);
    }
    if u_trim.is_empty() || v_trim.is_empty() {
        return None;
    }
    if is_comment(u_text, profile) != is_comment(v_text, profile) {
        return None;
    }

    // A pure-punctuation line (zero alphanumeric characters) must not pair with
    // a line that carries salient identifiers (e.g. `(` vs `x(` or `{` vs
    // `fn foo() {`). Structural delimiter-to-keyword transitions such as `}` vs
    // `} else {` remain eligible because keywords are not salient.
    let u_has_alnum = u_trim.chars().any(char::is_alphanumeric);
    let v_has_alnum = v_trim.chars().any(char::is_alphanumeric);
    if (!u_has_alnum && line_has_salient_token(v_text, profile))
        || (!v_has_alnum && line_has_salient_token(u_text, profile))
    {
        return None;
    }

    let indent_cost = (indent_of(u_text).abs_diff(indent_of(v_text)) as i64) * INDENT_PENALTY;

    if u_trim == v_trim {
        let chars = u_trim.chars().count() as i64;
        return Some(chars * MATCHED_CHAR_REWARD + EXACT_MATCH_BASE - indent_cost);
    }

    // A declaration that still exists verbatim on the opposite side is a
    // relocation candidate owned by the move pre-pass; pairing it as a
    // modification would steal that line. Running this before alignment also
    // avoids building an LCS tape for vetoed declaration pairs.
    let u_decl = declaration_role(u_trim);
    if u_decl.is_some() && has_verbatim_rival(u_trim, new_content) {
        return None;
    }
    let v_decl = declaration_role(v_trim);
    if v_decl.is_some() && has_verbatim_rival(v_trim, old_content) {
        return None;
    }

    // Build one profiled alignment tape and reuse it for wholesale rejection,
    // admissibility, matched-character scoring, and salient context checks.
    let (_spans, runs) = word_spans_and_runs_with_profile(u_text, v_text, profile);
    let matched = matched_chars(&runs);
    if matched == 0 {
        return None;
    }

    // An affix pair shares a verbatim run carrying salient content, so it is
    // admissible without asking the general predicate. Bare punctuation affixes
    // (`(`, `{`, `;`) must still pass `admissible_from_runs`.
    let shared_affix = [u_trim, v_trim].iter().any(|longer| {
        [u_trim, v_trim].iter().any(|shorter| {
            longer.len() > shorter.len()
                && (longer.starts_with(shorter) || longer.ends_with(shorter))
                && line_has_salient_token(shorter, profile)
        })
    });
    let decl_compat = matches!((u_decl, v_decl), (Some(ur), Some(vr))
        if are_roles_compatible(ur, vr) && u_trim.ends_with('{') == v_trim.ends_with('{'));
    if !(shared_affix || admissible_from_runs(u_text, v_text, &runs, profile) || decl_compat) {
        return None;
    }

    let u_chars = u_trim.chars().count();
    let v_chars = v_trim.chars().count();
    let smaller = u_chars.min(v_chars);
    let larger = u_chars.max(v_chars);

    let mut score = if is_affix_pair(u_trim, v_trim) {
        let coverage = smaller as f64 / larger.max(1) as f64;
        smaller as i64 * MATCHED_CHAR_REWARD - (larger - smaller) as i64 * UNMATCHED_CHAR_PENALTY
            + (AFFIX_CREDIT * coverage) as i64
            - indent_cost
    } else {
        matched as i64 * MATCHED_CHAR_REWARD
            - larger.saturating_sub(matched) as i64 * UNMATCHED_CHAR_PENALTY
            - indent_cost
    };

    let head_agrees = shares_statement_head_keyword(u_trim, v_trim, profile);
    if head_agrees {
        score += HEAD_AGREE_BONUS;
        let same_terminator = (u_trim.ends_with('{') && v_trim.ends_with('{'))
            || (u_trim.ends_with(';') && v_trim.ends_with(';'));
        if same_terminator {
            score += TERMINATOR_AGREE_BONUS;
        }
    }
    let key_agrees =
        assignment_key(u_trim, v_trim).is_some() || field_key(u_trim, v_trim).is_some();
    if key_agrees {
        score += KEY_AGREE_BONUS;
    }
    if indent_of(u_text) == indent_of(v_text) {
        let is_prefix = u_trim.starts_with(v_trim) || v_trim.starts_with(u_trim);
        let opens_container = robust_delim_delta(u_trim) > 0 || robust_delim_delta(v_trim) > 0;
        if is_prefix && opens_container {
            score += CONTAINER_OPEN_BONUS;
        } else if is_prefix || (opens_container && head_agrees) {
            score += HEAD_LINE_BONUS;
        }
    }

    // Compound gate: without a shared statement head, assignment target, or
    // field label, a pair carrying salient identifiers on both sides must
    // align at least one short identifier's worth of salient content covering
    // the minimum fraction of the smaller side.
    let head_link = head_agrees || key_agrees;
    if !head_link
        && line_has_salient_token(u_text, profile)
        && line_has_salient_token(v_text, profile)
    {
        if !has_salient_ctx(&runs, profile) {
            return None;
        }
        let u_tokens: Vec<&str> = tokenize_line(u_text).collect();
        let v_tokens: Vec<&str> = tokenize_line(v_text).collect();
        let (matched_salient, u_salient, v_salient) =
            shared_salient_chars(&u_tokens, &v_tokens, profile);
        let min_salient = u_salient.min(v_salient);
        if min_salient > 0
            && (matched_salient < MIN_MATCHED_SALIENT_CHARS
                || (matched_salient as f64 / min_salient as f64) < MIN_MATCHED_SALIENT_RATIO)
        {
            return None;
        }
    }

    (score > 0).then_some(score)
}

/// Whether `del_text` and `add_text` are admissible as a modification pair.
pub(crate) fn is_admissible_mod_pair(
    del_text: &str,
    add_text: &str,
    profile: Option<&LangProfile>,
) -> bool {
    if del_text == add_text {
        return true;
    }

    let (_spans, runs) = word_spans_and_runs_with_profile(del_text, add_text, profile);
    admissible_from_runs(del_text, add_text, &runs, profile)
}

/// The admissibility rule over an alignment that the caller has already built,
/// so evidence scoring can share one tape instead of rebuilding it.
pub(crate) fn admissible_from_runs(
    del_text: &str,
    add_text: &str,
    runs: &[(RunKind, String)],
    profile: Option<&LangProfile>,
) -> bool {
    if del_text == add_text {
        return true;
    }

    let del_has_salient = line_has_salient_token(del_text, profile);
    let add_has_salient = line_has_salient_token(add_text, profile);

    if del_has_salient && add_has_salient {
        let has_salient_ctx = runs
            .iter()
            .any(|(kind, text)| *kind == RunKind::Ctx && line_has_salient_token(text, profile));
        if !has_salient_ctx {
            // Safety fallback: if neither line shares any salient context,
            // check whether this is an entity declaration statement (let, fn,
            // const, def, struct, …). Declaration keywords do not confer
            // statement identity when both the declared entity name and value
            // expressions are replaced, unless common context accounts for the
            // majority (>= 50%) of the line.
            let is_decl_head = runs.first().is_some_and(|(kind, text)| {
                if *kind != RunKind::Ctx {
                    return false;
                }
                let mut found = false;
                for_each_token(text, |t| {
                    if !found && profile.is_some_and(|p| p.is_declaration_keyword(t)) {
                        found = true;
                    }
                });
                found
            });
            if is_decl_head {
                let ctx_chars: usize = runs
                    .iter()
                    .filter(|(k, _)| *k == RunKind::Ctx)
                    .map(|(_, t)| t.chars().count())
                    .sum();
                let total_chars = del_text.chars().count().max(add_text.chars().count());
                if ctx_chars * 2 < total_chars {
                    return false;
                }
            }
        }
    } else {
        // Context must not be purely whitespace
        let has_non_ws_ctx = runs
            .iter()
            .any(|(kind, text)| *kind == RunKind::Ctx && !text.trim().is_empty());
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

fn indent_of(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

fn is_comment(text: &str, profile: Option<&LangProfile>) -> bool {
    profile.is_some_and(|p| p.is_comment_line(text))
}

/// Characters explained by a set of owned runs from a single alignment tape.
fn matched_chars(runs: &[(RunKind, String)]) -> usize {
    runs.iter()
        .filter(|(kind, text)| *kind == RunKind::Ctx && !text.trim().is_empty())
        .map(|(_, text)| text.chars().count())
        .sum()
}

fn has_salient_ctx(runs: &[(RunKind, String)], profile: Option<&LangProfile>) -> bool {
    runs.iter()
        .any(|(kind, text)| *kind == RunKind::Ctx && line_has_salient_token(text, profile))
}

fn shared_salient_chars(
    u_tokens: &[&str],
    v_tokens: &[&str],
    profile: Option<&LangProfile>,
) -> (usize, usize, usize) {
    let mut u_salient = 0usize;
    let mut v_salient = 0usize;
    for (i, token) in u_tokens.iter().enumerate() {
        if is_token_salient_in_context(u_tokens, i, profile) {
            u_salient += token.chars().count();
        }
    }
    for (j, token) in v_tokens.iter().enumerate() {
        if is_token_salient_in_context(v_tokens, j, profile) {
            v_salient += token.chars().count();
        }
    }
    let matched: usize = u_tokens
        .iter()
        .enumerate()
        .filter(|&(i, _)| is_token_salient_in_context(u_tokens, i, profile))
        .filter(|&(_, token)| v_tokens.contains(token))
        .map(|(_, token)| token.chars().count())
        .sum();
    (matched, u_salient, v_salient)
}

fn assignment_key<'a>(u_trim: &'a str, v_trim: &'a str) -> Option<&'a str> {
    let u_key = assignment_target(u_trim)?;
    let v_key = assignment_target(v_trim)?;
    (u_key == v_key).then_some(u_key)
}

fn field_key<'a>(u_trim: &'a str, v_trim: &'a str) -> Option<&'a str> {
    let u_key = field_label(u_trim)?;
    let v_key = field_label(v_trim)?;
    (u_key == v_key).then_some(u_key)
}

/// Whether both lines open with the same statement-head keyword.
fn shares_statement_head_keyword(
    u_trim: &str,
    v_trim: &str,
    profile: Option<&LangProfile>,
) -> bool {
    match (first_word(u_trim), first_word(v_trim)) {
        (Some(u_head), Some(v_head)) => {
            u_head == v_head && is_statement_head_keyword(u_head, profile)
        }
        _ => false,
    }
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
}
