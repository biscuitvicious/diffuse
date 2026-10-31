//! Semantic Token Classification & Line Token Scanner.
//!
//! Maps language source text to typed semantic tokens (`Keyword`, `Operator`,
//! `String`, `Comment`, etc.) using registered language profile tables.

use std::sync::OnceLock;

use regex::Regex;

use super::{
    markdown::{for_each_markdown_token_dyn_with_header, for_each_markdown_token_with_header},
    table::is_line_alignment_row,
};
use crate::{
    color::TokenColor,
    fence::FenceRegion,
    profile::{LangProfile, SingleQuoteKind},
    registry::{for_each_syntax_token_span, lang_profile},
};

/// A classified token: the semantic color name + its text (+ an optional
/// SGR attribute — the markdown emphasis: bold/italic/underline/strike).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassifiedToken<'a> {
    pub color: TokenColor,
    pub sgr: Option<u32>,
    pub text: &'a str,
}

/// Zero-allocation scanner visiting semantic tokens in a run of text.
pub fn for_each_classified_token<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_classified_token_with_header(text, profile, false, f);
}

pub fn for_each_classified_token_with_header<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    is_header: bool,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_classified_token_dyn(text, profile, is_header, &mut f);
}

pub(crate) fn for_each_classified_token_dyn<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    is_header: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    let Some(profile) = profile else {
        f(ClassifiedToken {
            color: TokenColor::Default,
            sgr: None,
            text,
        });
        return;
    };
    if text.is_empty() {
        f(ClassifiedToken {
            color: TokenColor::Default,
            sgr: None,
            text,
        });
        return;
    }
    let mut last = 0usize;
    for_each_syntax_token_span(text, profile, |start, end| {
        if start > last {
            f(ClassifiedToken {
                color: TokenColor::Default,
                sgr: None,
                text: &text[last..start],
            });
        }
        let tok = &text[start..end];
        let next = text[end..].chars().next();
        let is_comment = (profile.hash_comment && tok.starts_with('#'))
            || tok.starts_with("//")
            || tok.starts_with("/*");
        if is_comment {
            for_each_comment_with_backticks(tok, profile, is_header, f);
            last = end;
            return;
        }

        let color = if tok.starts_with('\'') {
            match profile.single_quote {
                SingleQuoteKind::Char(rules) => {
                    if tok.len() >= 2
                        && tok.ends_with('\'')
                        && rules.validate_inner(&tok[1..tok.len() - 1])
                    {
                        TokenColor::CharLiteral
                    } else {
                        TokenColor::Operator
                    }
                }
                SingleQuoteKind::String => {
                    if tok.len() >= 2 && tok.ends_with('\'') {
                        TokenColor::String
                    } else {
                        TokenColor::Operator
                    }
                }
                SingleQuoteKind::None => TokenColor::Operator,
            }
        } else if tok.starts_with('"') || tok.starts_with('`') || tok.starts_with("r\"") {
            TokenColor::String
        } else if tok.starts_with("0x")
            || tok.starts_with("0X")
            || tok.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            TokenColor::Number
        } else if tok
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        {
            if profile.is_keyword(tok) {
                TokenColor::Keyword
            } else if profile.is_literal(tok) {
                TokenColor::Literal
            } else if profile.types_by_case && tok.chars().next().is_some_and(|c| c.is_uppercase())
            {
                TokenColor::Type
            } else if next == Some('(') {
                TokenColor::Function
            } else {
                TokenColor::Default
            }
        } else if tok.chars().all(char::is_whitespace) {
            TokenColor::Default
        } else {
            TokenColor::Operator
        };
        f(ClassifiedToken {
            color,
            sgr: None,
            text: tok,
        });
        last = end;
    });
    if last < text.len() {
        f(ClassifiedToken {
            color: TokenColor::Default,
            sgr: None,
            text: &text[last..],
        });
    }
}

/// Classify a run of text into semantic tokens (plain `text` when there is
/// no profile).
pub fn classify_tokens<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_classified_token(text, profile, |t| out.push(t));

    if cfg!(debug_assertions) {
        let total_len: usize = out.iter().map(|t| t.text.len()).sum();
        debug_assert_eq!(
            total_len,
            text.len(),
            "Classified tokens must partition input text exactly"
        );
    }

    out
}

fn for_each_comment_with_backticks<'a>(
    tok: &'a str,
    profile: &LangProfile,
    is_header: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    // Doc comments (`///`, `//!`) are MARKDOWN — rustdoc processes them, so
    // `**bold**`, `*italic*`, `~~strike~~`, `[link](url)`, and `` `code` ``
    // all render. Plain comments (`//`, `/*`, `#`) are NOT markdown; the
    // convention (rust-analyzer, GitHub) is a uniform comment color. The one
    // exception: inline `` `code` `` spans in a plain comment still render as
    // Rust (accent backticks + classified inner) — they read as code refs,
    // not prose. No markdown emphasis/link/strike on plain comments.
    let is_doc = tok.starts_with("///") || tok.starts_with("//!");
    // Doc: any markdown trigger opens the splitter. Plain: only a backtick.
    let has_markdown = is_doc
        && (tok.contains('`')
            || tok.contains("**")
            || tok.contains("~~")
            || tok.contains('[')
            || tok.contains('|'));
    let has_backtick = tok.contains('`');
    if !has_markdown && !has_backtick {
        f(ClassifiedToken {
            color: TokenColor::Comment,
            sgr: None,
            text: tok,
        });
        return;
    }

    let inner_prof = profile;
    let prefix_len = if is_doc {
        3
    } else if tok.starts_with("//") || tok.starts_with("/*") {
        2
    } else if tok.starts_with('#') {
        1
    } else {
        0
    };

    if prefix_len > 0 {
        f(ClassifiedToken {
            color: TokenColor::Comment,
            sgr: None,
            text: &tok[..prefix_len],
        });
    }

    let body = &tok[prefix_len..];
    if is_doc {
        // Doc comments: full markdown (emphasis, links, code) — the text
        // gaps between markdown spans are comment-colored.
        for_each_markdown_token_dyn_with_header(body, is_header, &mut |mut t| {
            if t.color.is_default() {
                t.color = TokenColor::Comment;
            } else if t.color == TokenColor::Code {
                let parent_sgr = t.sgr;
                for_each_classified_token_dyn(t.text, Some(inner_prof), false, &mut |mut it| {
                    if it.sgr.is_none() {
                        it.sgr = parent_sgr;
                    }
                    f(it);
                });
                return;
            }
            f(t);
        });
    } else {
        // Plain comments: backtick-only. Everything outside `` `code` `` is
        // comment-color; inside, the language's classification.
        for_each_backtick_token_dyn(body, inner_prof, f);
    }
}

fn for_each_backtick_token_dyn<'a>(
    body: &'a str,
    profile: &LangProfile,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    static BACKTICK: OnceLock<Regex> = OnceLock::new();
    let re = BACKTICK.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the comment backtick"));
    let mut last = 0usize;
    for m in re.find_iter(body) {
        if m.start() > last {
            f(ClassifiedToken {
                color: TokenColor::Comment,
                sgr: None,
                text: &body[last..m.start()],
            });
        }
        f(ClassifiedToken {
            color: TokenColor::Delimiter,
            sgr: None,
            text: "`",
        });
        let inner = &m.as_str()[1..m.as_str().len() - 1];
        for_each_classified_token_dyn(inner, Some(profile), false, f);
        f(ClassifiedToken {
            color: TokenColor::Delimiter,
            sgr: None,
            text: "`",
        });
        last = m.end();
    }
    if last < body.len() {
        f(ClassifiedToken {
            color: TokenColor::Comment,
            sgr: None,
            text: &body[last..],
        });
    }
}

pub fn for_each_line_classified_token<'a>(
    text: &'a str,
    file_profile: Option<&LangProfile>,
    region: Option<&FenceRegion>,
    f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_line_classified_token_with_next(text, None, file_profile, region, f);
}

pub fn for_each_line_classified_token_with_next<'a>(
    text: &'a str,
    next_line: Option<&str>,
    file_profile: Option<&LangProfile>,
    region: Option<&FenceRegion>,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    let is_header = next_line.is_some_and(is_line_alignment_row);
    match region {
        Some(r) => {
            let profile = r.lang.as_deref().and_then(lang_profile);
            match (
                profile.as_ref(),
                file_profile.and_then(|p| p.fence_strip.as_ref()),
            ) {
                (Some(p), Some(strip)) => {
                    if let Some(m) = strip.find(text)
                        && m.start() == 0
                        && m.end() > 0
                    {
                        f(ClassifiedToken {
                            color: TokenColor::Comment,
                            sgr: None,
                            text: &text[..m.end()],
                        });
                        for_each_classified_token_with_header(
                            &text[m.end()..],
                            Some(p),
                            is_header,
                            f,
                        );
                        return;
                    }
                    for_each_classified_token_with_header(text, Some(p), is_header, f);
                }
                _ => for_each_classified_token_with_header(text, profile, is_header, f),
            }
        }
        None => {
            if file_profile.is_some_and(|p| p.markdown) {
                for_each_markdown_token_with_header(text, is_header, f);
            } else {
                for_each_classified_token_with_header(text, file_profile, is_header, f);
            }
        }
    }
}

pub fn classify_line<'a>(
    text: &'a str,
    file_profile: Option<&LangProfile>,
    region: Option<&FenceRegion>,
) -> Vec<ClassifiedToken<'a>> {
    classify_line_with_next(text, None, file_profile, region)
}

pub fn classify_line_with_next<'a>(
    text: &'a str,
    next_line: Option<&str>,
    file_profile: Option<&LangProfile>,
    region: Option<&FenceRegion>,
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_line_classified_token_with_next(text, next_line, file_profile, region, |t| {
        out.push(t)
    });
    out
}
