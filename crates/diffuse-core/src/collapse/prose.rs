//! Prose and comment window classification and cross-slot relocation hygiene.
//!
//! Shared by Stage 1 ([`crate::collapse::project`]) and Stage 2
//! ([`crate::collapse::relocate`]) to suppress coincidental punctuation,
//! stopword, and lone mid-sentence noun relocations inside prose and comment
//! windows while preserving multi-word wraps, multi-word slot-pair exchanges,
//! and exact whole-word line-boundary wraps.

use diffuse_syntax::profile::LangProfile;

use crate::lcs::line_has_salient_token;

/// Byte range of a candidate cross-slot match on one source line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LineSpan<'a> {
    pub(crate) line: &'a str,
    pub(crate) slot: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Whether `line` is a non-blank prose or comment line eligible for prose
/// boundary-wrap folding under `profile`.
pub(crate) fn is_prose_or_comment_line(line: &str, profile: Option<&LangProfile>) -> bool {
    if line.trim().is_empty() {
        return false;
    }
    let Some(p) = profile else {
        return false;
    };
    if p.markdown {
        return !looks_like_markdown_code_line(line, p);
    }
    p.is_comment_line(line)
}

/// Returns `(leading_indent, active_marker)` identifying a prose/comment line's
/// gutter signature so boundary-wrap folding only chains lines inside the same
/// comment or blockquote paragraph.
pub(crate) fn prose_prefix_sig<'a>(
    line: &'a str,
    profile: Option<&LangProfile>,
) -> (&'a str, &'a str) {
    let lead_len = line.len() - line.trim_start().len();
    let indent = &line[..lead_len];
    let rest = &line[lead_len..];
    if let Some(p) = profile {
        if p.markdown {
            if rest.starts_with("> ") {
                return (indent, "> ");
            }
            return (indent, "");
        }
        if let Some(marker) = p.leading_comment_prefix(line) {
            return (indent, marker);
        }
    }
    (indent, "")
}

/// Whether consecutive prose/comment `Diag` pairs `(prev_old, prev_new)` and
/// `(next_old, next_new)` exhibit a directional line-boundary wrap — either an
/// unmatched tail of `prev_old` wrapping onto the head of `next_new`, or an
/// unmatched tail of `prev_new` unwrapping from the head of `next_old`.
///
/// Admits a multi-word salient phrase (`>= 2` words, `>= 6` chars, allowing up
/// to one leading sentence-closing word at `off == 1` on the target head, such
/// as `snaps. The thumb`), or a single exact boundary salient word (`>= 4` core
/// chars at `off == 0`).
pub(crate) fn is_prose_boundary_wrap(
    prev_old: &str,
    prev_new: &str,
    next_old: &str,
    next_new: &str,
    profile: Option<&LangProfile>,
) -> bool {
    let (po_s, po_e) = line_body_span(prev_old, profile);
    let (pn_s, pn_e) = line_body_span(prev_new, profile);
    let (no_s, no_e) = line_body_span(next_old, profile);
    let (nn_s, nn_e) = line_body_span(next_new, profile);
    let po = &prev_old[po_s..po_e];
    let pn = &prev_new[pn_s..pn_e];
    let no = &next_old[no_s..no_e];
    let nn = &next_new[nn_s..nn_e];

    has_directional_boundary_wrap(po, pn, nn, no, profile)
        || has_directional_boundary_wrap(pn, po, no, nn, profile)
}

/// Whether a window's lines consist exclusively of prose or comments under
/// `profile`.
///
/// For Markdown profiles (`profile.markdown`), fenced or code-syntax lines
/// (`fn `, `let `, `pub `, `{`, `}`, `;`, fence delimiters) are excluded so
/// embedded code examples retain full code-reflow semantics. For code
/// languages, every non-blank line must satisfy
/// [`LangProfile::is_comment_line`].
pub(crate) fn is_prose_window<'a>(
    mut lines: impl Iterator<Item = &'a str>,
    profile: Option<&LangProfile>,
) -> bool {
    let Some(p) = profile else {
        return false;
    };
    if p.markdown {
        return !lines.any(|line| looks_like_markdown_code_line(line, p));
    }
    lines.all(|line| line.trim().is_empty() || p.is_comment_line(line))
}

/// Returns the `(body_start, body_end)` byte offsets of `line` after stripping
/// leading whitespace, any active language comment prefix (or Markdown
/// blockquote `>`), and trailing whitespace.
pub(crate) fn line_body_span(line: &str, profile: Option<&LangProfile>) -> (usize, usize) {
    let trim_start = line.len() - line.trim_start().len();
    let rest = &line[trim_start..];
    let mut body_start = trim_start;

    if let Some(p) = profile {
        if p.markdown {
            if let Some(after) = rest.strip_prefix('>') {
                body_start += 1 + (after.len() - after.trim_start().len());
            }
        } else if let Some(marker) = p.leading_comment_prefix(line) {
            body_start += marker.len();
            let after = &line[body_start..];
            body_start += after.len() - after.trim_start().len();
        }
    }

    let body_end = line.trim_end().len();
    (body_start.min(body_end), body_end)
}

/// Counts alphabetic word segments in `text` (splitting on non-identifier,
/// non-hyphen characters).
pub(crate) fn count_alphabetic_words(text: &str) -> usize {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| !w.is_empty() && w.chars().any(char::is_alphabetic))
        .count()
}

/// Counts salient word segments in `text` under `profile`.
pub(crate) fn count_salient_words(text: &str, profile: Option<&LangProfile>) -> usize {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| !w.is_empty() && line_has_salient_token(w, profile))
        .count()
}

/// Decides whether relocating `phrase` between `old_span` and `new_span` inside
/// a prose/comment window carries sufficient evidence.
///
/// Admits a cross-slot move when `phrase` carries at least one salient token
/// and satisfies at least one of:
/// - contiguous multi-word phrase (`>= 2` alphabetic words),
/// - multi-word slot-pair exchange (`pair_salient_words >= 2` across the same
///   `(old_span.slot, new_span.slot)` pair), or
/// - exact whole-word line-boundary wrap (end of earlier line to start/end of
///   later line, or start of later line to end of earlier line).
pub(crate) fn prose_move_allowed(
    phrase: &str,
    old_span: LineSpan<'_>,
    new_span: LineSpan<'_>,
    pair_salient_words: usize,
    profile: Option<&LangProfile>,
) -> bool {
    if !line_has_salient_token(phrase, profile) {
        return false;
    }
    if count_alphabetic_words(phrase) >= 2 || pair_salient_words >= 2 {
        return true;
    }
    is_single_word_boundary_wrap(old_span, new_span, profile)
}

/// Whether a single-word match between `old_span` and `new_span` is a
/// whole-word directional line-boundary wrap.
fn is_single_word_boundary_wrap(
    old_span: LineSpan<'_>,
    new_span: LineSpan<'_>,
    profile: Option<&LangProfile>,
) -> bool {
    if !is_whole_word_span(old_span.line, old_span.start, old_span.end)
        || !is_whole_word_span(new_span.line, new_span.start, new_span.end)
    {
        return false;
    }

    let (ob_s, ob_e) = line_body_span(old_span.line, profile);
    let (nb_s, nb_e) = line_body_span(new_span.line, profile);

    let o_at_start = old_span.start <= ob_s;
    let o_at_end = old_span.end >= ob_e
        || old_span.line[old_span.end..ob_e]
            .chars()
            .all(|c| !c.is_alphanumeric());
    let n_at_start = new_span.start <= nb_s;
    let n_at_end = new_span.end >= nb_e
        || new_span.line[new_span.end..nb_e]
            .chars()
            .all(|c| !c.is_alphanumeric());

    let downward_wrap = old_span.slot < new_span.slot && o_at_end && (n_at_start || n_at_end);
    let upward_wrap = old_span.slot > new_span.slot && o_at_start && n_at_end;
    downward_wrap || upward_wrap
}

fn is_whole_word_span(line: &str, start: usize, end: usize) -> bool {
    let left_ok = !line[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    let right_ok = !line[end..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    left_ok && right_ok
}

fn has_directional_boundary_wrap(
    tail_src: &str,
    tail_other: &str,
    head_tgt: &str,
    head_other: &str,
    profile: Option<&LangProfile>,
) -> bool {
    let ts: Vec<&str> = tail_src.split_whitespace().collect();
    let to: Vec<&str> = tail_other.split_whitespace().collect();
    let ht: Vec<&str> = head_tgt.split_whitespace().collect();
    let ho: Vec<&str> = head_other.split_whitespace().collect();
    if ts.is_empty() || ht.is_empty() {
        return false;
    }
    if !to.is_empty() && ts.last() == to.last() {
        return false;
    }
    if !ho.is_empty() && ht.first() == ho.first() {
        return false;
    }

    let max_off = ht.len().saturating_sub(1).min(1);
    for off in 0..=max_off {
        let max_k = ts.len().min(ht.len() - off);
        for k in (1..=max_k).rev() {
            let suf = &ts[ts.len() - k..];
            let pre = &ht[off..off + k];
            let matches = if k >= 2 {
                suf == pre
            } else {
                off == 0 && suf[0] == pre[0]
            };
            if !matches {
                continue;
            }
            let has_salient = suf.iter().any(|w| line_has_salient_token(w, profile));
            if !has_salient {
                continue;
            }
            if k >= 2 {
                let char_len: usize =
                    suf.iter().map(|w| w.len()).sum::<usize>() + k.saturating_sub(1);
                if char_len >= 6 {
                    return true;
                }
            } else if word_core(suf[0]).len() >= 4 {
                return true;
            }
        }
    }
    false
}

fn word_core(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '`')
}

fn looks_like_markdown_code_line(line: &str, profile: &LangProfile) -> bool {
    if profile.fence.as_ref().is_some_and(|re| re.is_match(line)) {
        return true;
    }
    let t = line.trim();
    t.starts_with("fn ")
        || t.starts_with("let ")
        || t.starts_with("pub ")
        || t.ends_with('{')
        || t == "}"
        || t.ends_with(';')
}

#[cfg(test)]
mod tests {
    use diffuse_syntax::registry::lang_profile;

    use super::*;

    #[test]
    fn is_prose_window_handles_none_comments_and_markdown_fences() {
        let rs = lang_profile("rs");
        let py = lang_profile("py");
        let md = lang_profile("md");

        assert!(!is_prose_window(["/// hello"].into_iter(), None));
        assert!(is_prose_window(
            ["/// hello", "", "// world"].into_iter(),
            rs
        ));
        assert!(!is_prose_window(
            ["/// hello", "let x = 1;"].into_iter(),
            rs
        ));
        assert!(is_prose_window(["# comment", "  # next"].into_iter(), py));
        assert!(is_prose_window(["Plain prose", "> quoted"].into_iter(), md));
        assert!(!is_prose_window(
            ["```rust", "fn main() {}"].into_iter(),
            md
        ));
        assert!(!is_prose_window(["    let x = 1;"].into_iter(), md));
    }

    #[test]
    fn line_body_span_strips_profile_comment_and_blockquote_prefixes() {
        let rs = lang_profile("rs");
        let py = lang_profile("py");
        let md = lang_profile("md");

        let line_doc = "    /// hello world  ";
        let (s, e) = line_body_span(line_doc, rs);
        assert_eq!(&line_doc[s..e], "hello world");

        let line_block = " * block line";
        let (s, e) = line_body_span(line_block, rs);
        assert_eq!(&line_block[s..e], "block line");

        let line_py = "  # py note";
        let (s, e) = line_body_span(line_py, py);
        assert_eq!(&line_py[s..e], "py note");

        let line_md = "  > quoted text";
        let (s, e) = line_body_span(line_md, md);
        assert_eq!(&line_md[s..e], "quoted text");

        let plain = "  plain  ";
        let (s, e) = line_body_span(plain, None);
        assert_eq!(&plain[s..e], "plain");
    }

    #[test]
    fn prose_move_allowed_distinguishes_boundary_wraps_from_subwords_and_mid_sentence() {
        let rs = lang_profile("rs");
        let md = lang_profile("md");

        // Subword prefix (`byte-identical` inside `byte-identically.`) rejected
        let old_l = "- renders with a byte-identical";
        let new_l = "  byte-identically.";
        let o_start = old_l.find("byte-identical").unwrap();
        let n_start = new_l.find("byte-identical").unwrap();
        assert!(!prose_move_allowed(
            "byte-identical",
            LineSpan {
                line: old_l,
                slot: 1,
                start: o_start,
                end: o_start + "byte-identical".len(),
            },
            LineSpan {
                line: new_l,
                slot: 0,
                start: n_start,
                end: n_start + "byte-identical".len(),
            },
            1,
            md,
        ));

        // Upward single-word whole-word boundary wrap accepted
        let old_up = "/// exact mapping";
        let new_up = "/// position: exact";
        assert!(prose_move_allowed(
            "exact",
            LineSpan {
                line: old_up,
                slot: 1,
                start: 4,
                end: 9,
            },
            LineSpan {
                line: new_up,
                slot: 0,
                start: 14,
                end: 19,
            },
            1,
            rs,
        ));

        // Mid-sentence single word rejected when pair_salient_words == 1,
        // rescued when pair_salient_words >= 2.
        let mid_o = "/// so the thumb moves";
        let mid_n = "/// bottom of thumb here";
        let o_span = LineSpan {
            line: mid_o,
            slot: 1,
            start: 11,
            end: 16,
        };
        let n_span = LineSpan {
            line: mid_n,
            slot: 0,
            start: 14,
            end: 19,
        };
        assert!(!prose_move_allowed("thumb", o_span, n_span, 1, rs));
        assert!(prose_move_allowed("thumb", o_span, n_span, 2, rs));
    }

    #[test]
    fn prose_boundary_wrap_handles_phrases_single_words_and_rejections() {
        let rs = lang_profile("rs");
        let md = lang_profile("md");

        // Upward multi-word wrap at off == 1 (`snaps. The thumb`).
        assert!(is_prose_boundary_wrap(
            "/// yields `max_scroll`; everything between is proportional with no",
            "/// yields `max_scroll` (bottom of thumb at `track_h - 1`). The thumb",
            "/// snaps. The thumb follows through [`Self::scrollbar_thumb_for`],",
            "/// follows smoothly through [`Self::scrollbar_thumb_for`] with zero",
            rs,
        ));

        // Exact single-word boundary wrap at off == 0 (`exact`).
        assert!(is_prose_boundary_wrap(
            "/// Scroll position for track row `row`: one",
            "/// Scroll position for track row `row`: exact",
            "/// exact mapping used identically",
            "/// invertible mapping used identically",
            rs,
        ));

        // Rejected when tails already match on prev line, or heads already
        // match on next line, or single word is too short / non-salient.
        assert!(!is_prose_boundary_wrap(
            "/// foo bar baz",
            "/// qux bar baz",
            "/// baz next line",
            "/// other next line",
            rs,
        ));
        assert!(!is_prose_boundary_wrap(
            "/// ending with the",
            "/// ending with new",
            "/// the continuation",
            "/// other continuation",
            rs,
        ));

        assert_eq!(prose_prefix_sig("  /// doc", rs), ("  ", "///"));
        assert_eq!(prose_prefix_sig("  > quote", md), ("  ", "> "));
        assert_eq!(prose_prefix_sig("  plain", None), ("  ", ""));
        assert!(!is_prose_or_comment_line("   ", rs));
        assert!(!is_prose_or_comment_line("/// doc", None));
        assert!(!is_prose_or_comment_line("```rust", md));
    }
}
