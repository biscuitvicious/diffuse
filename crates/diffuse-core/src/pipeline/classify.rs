//! Token syntax classification, SGR mapping, and hunk row building.

use std::{borrow::Cow, mem};

use diffuse_syntax::{
    classifier::{
        ClassifiedToken, for_each_classified_token, for_each_classified_token_with_header,
        for_each_line_classified_token_with_next, for_each_markdown_token_with_header,
        is_line_alignment_row,
    },
    color::TokenColor,
    fence::{FenceRegion, region_at},
    profile::LangProfile,
    registry::lang_profile,
};

use crate::{
    merge::{RunKind, is_wholesale_mod, merged_runs_with_profile},
    parse::{Row, RowKind},
    wire::{MovSide, OutputRow, OutputRun},
};

#[inline]
pub fn token_cow(s: &str) -> Cow<'static, str> {
    match s {
        " " => Cow::Borrowed(" "),
        "  " => Cow::Borrowed("  "),
        "   " => Cow::Borrowed("   "),
        "    " => Cow::Borrowed("    "),
        "        " => Cow::Borrowed("        "),
        "(" => Cow::Borrowed("("),
        ")" => Cow::Borrowed(")"),
        "{" => Cow::Borrowed("{"),
        "}" => Cow::Borrowed("}"),
        "[" => Cow::Borrowed("["),
        "]" => Cow::Borrowed("]"),
        ";" => Cow::Borrowed(";"),
        "," => Cow::Borrowed(","),
        ":" => Cow::Borrowed(":"),
        "::" => Cow::Borrowed("::"),
        "." => Cow::Borrowed("."),
        "=" => Cow::Borrowed("="),
        "==" => Cow::Borrowed("=="),
        "!=" => Cow::Borrowed("!="),
        "<" => Cow::Borrowed("<"),
        ">" => Cow::Borrowed(">"),
        "+" => Cow::Borrowed("+"),
        "-" => Cow::Borrowed("-"),
        "*" => Cow::Borrowed("*"),
        "/" => Cow::Borrowed("/"),
        "&" => Cow::Borrowed("&"),
        "|" => Cow::Borrowed("|"),
        "!" => Cow::Borrowed("!"),
        "?" => Cow::Borrowed("?"),
        "->" => Cow::Borrowed("->"),
        "=>" => Cow::Borrowed("=>"),
        "\n" => Cow::Borrowed("\n"),
        "" => Cow::Borrowed(""),
        _ => Cow::Owned(s.to_string()),
    }
}

/// The merged runs with the semantic syntax colors on the common/add runs.
pub fn colored_runs(
    r: &Row,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<OutputRun> {
    colored_runs_with_next(r, None, profile, regions)
}

pub fn colored_runs_with_next(
    r: &Row,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<OutputRun> {
    let mut out = Vec::new();
    let new_line = r.new_line();
    let region = new_line.and_then(|n| region_at(regions, n));

    let use_merged = matches!(r, Row::Mod { .. } | Row::Add { group: Some(_), .. })
        || !r.cached_runs().is_empty();

    if !use_merged {
        if matches!(r, Row::Add { .. } | Row::MovTgt { .. } | Row::Ctx(_)) {
            let kind = if matches!(r, Row::Add { .. } | Row::MovTgt { .. }) {
                RunKind::Add
            } else {
                RunKind::Ctx
            };
            for_each_ctx_line_token(r.text(), new_line, next_line, profile, regions, |t| {
                out.push(OutputRun {
                    kind,
                    color: t.color,
                    sgr: t.sgr,
                    text: token_cow(t.text),
                });
            });
        } else {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: TokenColor::Default,
                sgr: None,
                text: token_cow(r.text()),
            });
        }
        return out;
    }

    thread_local! {
        static CTX_SCRATCH: std::cell::RefCell<Vec<ClassifiedToken<'static>>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    let target_text: Cow<'_, str> =
        if matches!(r, Row::MovSrc { .. }) && !r.cached_runs().is_empty() {
            let mut s = String::new();
            for (kind, t) in r.cached_runs() {
                if !kind.is_del_like() {
                    s.push_str(t);
                }
            }
            Cow::Owned(s)
        } else {
            Cow::Borrowed(r.text())
        };

    CTX_SCRATCH.with(|cell| {
        let mut full_tokens = cell.borrow_mut();
        full_tokens.clear();
        for_each_ctx_line_token(&target_text, new_line, next_line, profile, regions, |t| {
            // Safety: tokens are only accessed within this block while target_text
            // is alive.
            let tok_static =
                unsafe { mem::transmute::<ClassifiedToken<'_>, ClassifiedToken<'static>>(t) };
            full_tokens.push(tok_static);
        });

        let mut add_pos = 0usize;
        for run in merged_runs_with_profile(r, profile) {
            if run.kind == RunKind::Del {
                push_output_run(
                    &mut out,
                    OutputRun {
                        kind: RunKind::Del,
                        color: TokenColor::Default,
                        sgr: None,
                        text: token_cow(run.text),
                    },
                );
            } else if run.kind == RunKind::MovedSrc {
                // Moved-out source text is old-side: emit it whole (no
                // new-side token slicing — its characters are absent from
                // the new line) for the wire; renderers skip it by default.
                push_output_run(
                    &mut out,
                    OutputRun {
                        kind: RunKind::MovedSrc,
                        color: TokenColor::Default,
                        sgr: None,
                        text: token_cow(run.text),
                    },
                );
            } else {
                let run_len = if run.text.is_ascii() {
                    run.text.len()
                } else {
                    run.text.chars().count()
                };
                let end_pos = add_pos + run_len;
                let mut had_frags = false;
                for_each_sliced_token(&full_tokens, add_pos, end_pos, |t| {
                    had_frags = true;
                    push_output_run(
                        &mut out,
                        OutputRun {
                            kind: run.kind,
                            color: t.color,
                            sgr: t.sgr,
                            text: token_cow(t.text),
                        },
                    );
                });
                add_pos = end_pos;
                if !had_frags {
                    let push_t = |t: ClassifiedToken| {
                        push_output_run(
                            &mut out,
                            OutputRun {
                                kind: run.kind,
                                color: t.color,
                                sgr: t.sgr,
                                text: token_cow(t.text),
                            },
                        );
                    };
                    match region {
                        Some(rg) => for_each_classified_token(
                            run.text,
                            rg.lang.as_deref().and_then(lang_profile),
                            push_t,
                        ),
                        None => {
                            if profile.is_some_and(|p| p.markdown) {
                                for_each_markdown_token_with_header(
                                    run.text,
                                    next_line.is_some_and(is_line_alignment_row),
                                    push_t,
                                );
                            } else {
                                for_each_classified_token_with_header(
                                    run.text,
                                    profile,
                                    next_line.is_some_and(is_line_alignment_row),
                                    push_t,
                                );
                            }
                        }
                    }
                }
            }
        }
        full_tokens.clear();
    });
    if out.is_empty() && r.text().is_empty() {
        if matches!(r, Row::Add { group: Some(_), .. }) {
            out.push(OutputRun {
                kind: RunKind::Add,
                color: TokenColor::Default,
                sgr: None,
                text: token_cow(""),
            });
        } else if matches!(r, Row::Del { group: Some(_), .. }) {
            out.push(OutputRun {
                kind: RunKind::Del,
                color: TokenColor::Default,
                sgr: None,
                text: token_cow(""),
            });
        }
    }
    out
}

#[inline(always)]
fn push_output_run(out: &mut Vec<OutputRun>, run: OutputRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.kind == run.kind
        && last.color == run.color
        && last.sgr == run.sgr
    {
        last.text.to_mut().push_str(&run.text);
        return;
    }
    out.push(run);
}

pub(crate) fn for_each_ctx_line_token<'a>(
    text: &'a str,
    new_line: Option<u32>,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    let is_delimiter = if regions.is_empty() {
        false
    } else if let Some(n) = new_line {
        let idx = regions.partition_point(|rg| rg.end + 1 < n);
        (idx < regions.len()
            && (n + 1 == regions[idx].start || (n > 0 && n - 1 == regions[idx].end)))
            || (idx + 1 < regions.len()
                && (n + 1 == regions[idx + 1].start || (n > 0 && n - 1 == regions[idx + 1].end)))
    } else {
        false
    };
    if is_delimiter {
        if let Some(strip) = profile.and_then(|p| p.fence_strip.as_ref())
            && let Some(m) = strip.find(text)
            && m.start() == 0
            && m.end() > 0
        {
            f(ClassifiedToken {
                color: TokenColor::Comment,
                sgr: None,
                text: &text[..m.end()],
            });
            f(ClassifiedToken {
                color: TokenColor::FenceBorder,
                sgr: None,
                text: &text[m.end()..],
            });
            return;
        }
        f(ClassifiedToken {
            color: TokenColor::FenceBorder,
            sgr: None,
            text,
        });
        return;
    }
    let region = new_line.and_then(|n| region_at(regions, n));
    for_each_line_classified_token_with_next(text, next_line, profile, region, f);
}

#[cfg(test)]
pub(crate) fn classify_ctx_line<'a>(
    text: &'a str,
    new_line: Option<u32>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_ctx_line_token(text, new_line, None, profile, regions, |t| out.push(t));
    out
}

fn for_each_sliced_token<'a>(
    tokens: &[ClassifiedToken<'a>],
    start_char: usize,
    end_char: usize,
    mut f: impl FnMut(ClassifiedToken<'a>),
) -> bool {
    let mut found = false;
    let mut pos = 0;
    for tok in tokens {
        let tok_len = if tok.text.is_ascii() {
            tok.text.len()
        } else {
            tok.text.chars().count()
        };
        let tok_start = pos;
        let tok_end = pos + tok_len;
        pos = tok_end;
        if tok_end <= start_char || tok_start >= end_char {
            continue;
        }
        let s = start_char.saturating_sub(tok_start);
        let e = end_char.min(tok_end) - tok_start;
        let piece: &'a str = if tok.text.is_ascii() {
            &tok.text[s..e]
        } else {
            crate::lcs::char_slice(tok.text, s, e)
        };
        if !piece.is_empty() {
            found = true;
            f(ClassifiedToken {
                color: tok.color,
                sgr: tok.sgr,
                text: piece,
            });
        }
    }
    found
}

/// Tokenizes and classifies a single context line into styled `OutputRun`s
/// based on file extension and line number.
pub fn context_line_runs(line_text: &str, line_num: Option<u32>, ext: &str) -> Vec<OutputRun> {
    let profile = lang_profile(ext);
    let mut runs = Vec::new();
    for_each_ctx_line_token(line_text, line_num, None, profile, &[], |tk| {
        runs.push(OutputRun {
            kind: crate::merge::RunKind::Ctx,
            color: tk.color,
            sgr: tk.sgr,
            text: tk.text.to_string().into(),
        });
    });
    runs
}

pub fn build_hunk_output_row(
    r: &Row,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
    regions: &[FenceRegion],
) -> OutputRow {
    let (kind, mov_side, new_line, old_line) = match r {
        Row::MovSrc { line, .. } => (RowKind::Mov, Some(MovSide::Src), None, Some(line.num)),
        Row::MovTgt { line, .. } => (RowKind::Mov, Some(MovSide::Tgt), Some(line.num), None),
        _ => (r.kind(), None, r.new_line(), r.old_line()),
    };
    OutputRow {
        kind,
        mov_side,
        new_line,
        old_line,
        runs: colored_runs_with_next(r, next_line, profile, regions),
        wholesale: matches!(r, Row::Mod { .. }) && is_wholesale_mod(r),
        group: r.group(),
    }
}
