//! Token classifier — semantic syntax coloring of text spans.
//!
//! `classify_tokens` is the hot path: it splits a text fragment into tokens
//! via the hand-rolled scanner and maps each token to a semantic color name
//! (`syntaxKeyword`, `syntaxString`, …). The markdown classifiers
//! (`markdown_tokens`, `md_inner_tokens`) handle doc-comment prose and the
//! markdown default region.

use std::sync::OnceLock;

use regex::Regex;

use super::{
    profile::{LangProfile, MD_SGR_BOLD, MD_SGR_ITALIC, MD_SGR_STRIKE, MD_SGR_UNDERLINE},
    registry::for_each_syntax_token_span,
};

/// A classified token: the semantic color name + its text (+ an optional
/// SGR attribute — the markdown emphasis: bold/italic/underline/strike).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassifiedToken<'a> {
    pub color: &'static str,
    pub sgr: Option<u32>,
    pub text: &'a str,
}

/// Zero-allocation scanner visiting semantic tokens in a run of text.
pub fn for_each_classified_token<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_classified_token_dyn(text, profile, &mut f);
}

fn for_each_classified_token_dyn<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    let Some(profile) = profile else {
        f(ClassifiedToken {
            color: "text",
            sgr: None,
            text,
        });
        return;
    };
    if text.is_empty() {
        f(ClassifiedToken {
            color: "text",
            sgr: None,
            text,
        });
        return;
    }
    let mut last = 0usize;
    for_each_syntax_token_span(text, profile, |start, end| {
        if start > last {
            f(ClassifiedToken {
                color: "text",
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
            for_each_comment_with_backticks(tok, profile, f);
            last = end;
            return;
        }

        let color = if tok.starts_with('"')
            || tok.starts_with('\'')
            || tok.starts_with('`')
            || tok.starts_with("r\"")
        {
            "syntaxString"
        } else if tok.starts_with("0x")
            || tok.starts_with("0X")
            || tok.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            "syntaxNumber"
        } else if tok
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            if profile.is_keyword(tok) {
                "syntaxKeyword"
            } else if profile.types_by_case
                && tok.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            {
                "syntaxType"
            } else if next == Some('(') {
                "syntaxFunction"
            } else {
                "text"
            }
        } else if tok.chars().all(char::is_whitespace) {
            "text"
        } else {
            "syntaxOperator"
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
            color: "text",
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
    out
}

fn for_each_comment_with_backticks<'a>(
    tok: &'a str,
    profile: &LangProfile,
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
        && (tok.contains('`') || tok.contains("**") || tok.contains("~~") || tok.contains('['));
    let has_backtick = tok.contains('`');
    if !has_markdown && !has_backtick {
        f(ClassifiedToken {
            color: "syntaxComment",
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
            color: "syntaxComment",
            sgr: None,
            text: &tok[..prefix_len],
        });
    }

    let body = &tok[prefix_len..];
    if is_doc {
        // Doc comments: full markdown (emphasis, links, code) — the text
        // gaps between markdown spans are comment-colored.
        for_each_markdown_token_dyn(body, &mut |mut t| {
            if t.color == "text" {
                t.color = "syntaxComment";
            } else if t.color == "syntaxString" {
                let parent_sgr = t.sgr;
                for_each_classified_token_dyn(t.text, Some(inner_prof), &mut |mut it| {
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
                color: "syntaxComment",
                sgr: None,
                text: &body[last..m.start()],
            });
        }
        f(ClassifiedToken {
            color: "accent",
            sgr: None,
            text: "`",
        });
        let inner = &m.as_str()[1..m.as_str().len() - 1];
        for_each_classified_token_dyn(inner, Some(profile), f);
        f(ClassifiedToken {
            color: "accent",
            sgr: None,
            text: "`",
        });
        last = m.end();
    }
    if last < body.len() {
        f(ClassifiedToken {
            color: "syntaxComment",
            sgr: None,
            text: &body[last..],
        });
    }
}

pub fn for_each_md_inner_token<'a>(
    inner: &'a str,
    parent_sgr: u32,
    parent_color: &'static str,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    static INNER_CODE: OnceLock<Regex> = OnceLock::new();
    let re = INNER_CODE.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the inner code"));
    let mut last = 0usize;
    for m in re.find_iter(inner) {
        if m.start() > last {
            f(ClassifiedToken {
                color: parent_color,
                sgr: Some(parent_sgr),
                text: &inner[last..m.start()],
            });
        }
        let tok = m.as_str();
        // `code` inside bold/italic: code color wins, parent SGR stacks.
        f(ClassifiedToken {
            color: "accent",
            sgr: None,
            text: "`",
        });
        f(ClassifiedToken {
            color: "syntaxString",
            sgr: Some(parent_sgr),
            text: &tok[1..tok.len() - 1],
        });
        f(ClassifiedToken {
            color: "accent",
            sgr: None,
            text: "`",
        });
        last = m.end();
    }
    if last < inner.len() {
        f(ClassifiedToken {
            color: parent_color,
            sgr: Some(parent_sgr),
            text: &inner[last..],
        });
    }
}

pub fn md_inner_tokens<'a>(
    inner: &'a str,
    parent_sgr: u32,
    parent_color: &'static str,
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_md_inner_token(inner, parent_sgr, parent_color, |t| out.push(t));
    out
}

pub fn for_each_markdown_token<'a>(text: &'a str, mut f: impl FnMut(ClassifiedToken<'a>)) {
    for_each_markdown_token_dyn(text, &mut f);
}

fn for_each_markdown_token_dyn<'a>(text: &'a str, f: &mut dyn FnMut(ClassifiedToken<'a>)) {
    if text.is_empty() {
        return;
    }

    use pulldown_cmark::{BlockQuoteKind, Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options.insert(Options::ENABLE_GFM);

    let parser = Parser::new_ext(text, options);
    let mut cursor = 0usize;

    // Active formatting state stacks
    let mut active_sgr: Option<u32> = None;
    let mut active_color: &'static str = "text";
    let mut is_heading = false;
    let mut is_table_head = false;

    for (event, range) in parser.into_offset_iter() {
        let r_start = range.start.min(text.len());
        let r_end = range.end.min(text.len());

        match event {
            Event::Start(tag) => {
                match tag {
                    Tag::Heading { .. } => {
                        is_heading = true;
                        active_sgr = Some(MD_SGR_BOLD);
                        active_color = "accent";
                        // Emit any leading hashes/spaces
                        if cursor < r_end {
                            let head_slice = &text[cursor..r_end];
                            if let Some(pos) = head_slice.find(|c: char| c != '#' && c != ' ') {
                                let hash_end = cursor + pos;
                                let hashes = text[cursor..hash_end].trim_end();
                                let spaces = &text[cursor + hashes.len()..hash_end];
                                if !hashes.is_empty() {
                                    f(ClassifiedToken {
                                        color: "accent",
                                        sgr: None,
                                        text: hashes,
                                    });
                                }
                                if !spaces.is_empty() {
                                    f(ClassifiedToken {
                                        color: "text",
                                        sgr: None,
                                        text: spaces,
                                    });
                                }
                                cursor = hash_end;
                            }
                        }
                    }
                    Tag::Strong => {
                        if cursor < r_start {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &text[cursor..r_start],
                            });
                            cursor = r_start;
                        }
                        active_sgr = Some(MD_SGR_BOLD);
                        active_color = "mdBold";
                    }
                    Tag::Emphasis => {
                        if cursor < r_start {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &text[cursor..r_start],
                            });
                            cursor = r_start;
                        }
                        active_sgr = Some(MD_SGR_ITALIC);
                        active_color = "mdItalic";
                    }
                    Tag::Strikethrough => {
                        if cursor < r_start {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &text[cursor..r_start],
                            });
                            cursor = r_start;
                        }
                        active_sgr = Some(MD_SGR_STRIKE);
                        active_color = "mdStrike";
                    }
                    Tag::Link { .. } => {
                        if cursor < r_start {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &text[cursor..r_start],
                            });
                            cursor = r_start;
                        }
                        active_sgr = Some(MD_SGR_UNDERLINE);
                        active_color = "mdLink";
                    }
                    Tag::TableHead => {
                        is_table_head = true;
                        active_sgr = Some(MD_SGR_BOLD);
                        active_color = "accent";
                    }
                    Tag::BlockQuote(alert) => {
                        if cursor < r_start {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &text[cursor..r_start],
                            });
                            cursor = r_start;
                        }
                        if let Some(kind) = alert {
                            match kind {
                                BlockQuoteKind::Note | BlockQuoteKind::Tip => {
                                    active_color = "syntaxFunction";
                                }
                                BlockQuoteKind::Important | BlockQuoteKind::Warning => {
                                    active_color = "syntaxKeyword";
                                }
                                BlockQuoteKind::Caution => {
                                    active_color = "syntaxType";
                                }
                            }
                            active_sgr = Some(MD_SGR_BOLD);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    is_heading = false;
                    active_sgr = None;
                    active_color = "text";
                    if cursor < r_end {
                        f(ClassifiedToken {
                            color: "accent",
                            sgr: Some(MD_SGR_BOLD),
                            text: &text[cursor..r_end],
                        });
                        cursor = r_end;
                    }
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                    if cursor < r_end {
                        f(ClassifiedToken {
                            color: "accent",
                            sgr: None,
                            text: &text[cursor..r_end],
                        });
                        cursor = r_end;
                    }
                    active_sgr = None;
                    active_color = if is_heading || is_table_head {
                        "accent"
                    } else {
                        "text"
                    };
                }
                TagEnd::Link => {
                    if cursor < r_end {
                        let link_tail = &text[cursor..r_end];
                        if let Some(close_bracket) = link_tail.find("](") {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: &link_tail[..close_bracket + 2],
                            });
                            let url_and_paren = &link_tail[close_bracket + 2..];
                            if let Some(paren) = url_and_paren.rfind(')') {
                                f(ClassifiedToken {
                                    color: "mdUrl",
                                    sgr: Some(MD_SGR_UNDERLINE),
                                    text: &url_and_paren[..paren],
                                });
                                f(ClassifiedToken {
                                    color: "accent",
                                    sgr: None,
                                    text: &url_and_paren[paren..],
                                });
                            } else {
                                f(ClassifiedToken {
                                    color: "mdUrl",
                                    sgr: Some(MD_SGR_UNDERLINE),
                                    text: url_and_paren,
                                });
                            }
                        } else {
                            f(ClassifiedToken {
                                color: "accent",
                                sgr: None,
                                text: link_tail,
                            });
                        }
                        cursor = r_end;
                    }
                    active_sgr = None;
                    active_color = "text";
                }
                TagEnd::TableHead => {
                    is_table_head = false;
                    active_sgr = None;
                    active_color = "text";
                }
                TagEnd::BlockQuote(_) => {
                    active_sgr = None;
                    active_color = "text";
                }
                _ => {}
            },
            Event::Code(_) => {
                if cursor < r_start {
                    let gap = &text[cursor..r_start];
                    let (gap_color, gap_sgr) = if gap.contains('|') {
                        ("border", None)
                    } else if gap.starts_with('>')
                        || gap.starts_with('-')
                        || gap.starts_with('*')
                        || gap.starts_with('#')
                        || gap.starts_with('[')
                        || gap.ends_with(']')
                        || gap.starts_with('`')
                        || gap.starts_with('~')
                    {
                        ("accent", None)
                    } else {
                        (active_color, active_sgr)
                    };
                    f(ClassifiedToken {
                        color: gap_color,
                        sgr: gap_sgr,
                        text: gap,
                    });
                }
                let code_slice = &text[r_start..r_end];
                let num_ticks = code_slice.chars().take_while(|&c| c == '`').count();
                if num_ticks > 0 && code_slice.len() >= num_ticks * 2 {
                    f(ClassifiedToken {
                        color: "accent",
                        sgr: None,
                        text: &code_slice[..num_ticks],
                    });
                    f(ClassifiedToken {
                        color: "syntaxString",
                        sgr: active_sgr,
                        text: &code_slice[num_ticks..code_slice.len() - num_ticks],
                    });
                    f(ClassifiedToken {
                        color: "accent",
                        sgr: None,
                        text: &code_slice[code_slice.len() - num_ticks..],
                    });
                } else {
                    f(ClassifiedToken {
                        color: "syntaxString",
                        sgr: active_sgr,
                        text: code_slice,
                    });
                }
                cursor = r_end;
            }
            Event::Text(_) => {
                if cursor < r_start {
                    let gap = &text[cursor..r_start];
                    let (gap_color, gap_sgr) = if gap.contains('|') {
                        ("border", None)
                    } else if gap.starts_with('>')
                        || gap.starts_with('-')
                        || gap.starts_with('*')
                        || gap.starts_with('#')
                        || gap.starts_with('[')
                        || gap.ends_with(']')
                        || gap.starts_with('`')
                        || gap.starts_with('~')
                    {
                        ("accent", None)
                    } else {
                        (active_color, active_sgr)
                    };
                    f(ClassifiedToken {
                        color: gap_color,
                        sgr: gap_sgr,
                        text: gap,
                    });
                }
                let tok_slice = &text[r_start..r_end];
                f(ClassifiedToken {
                    color: active_color,
                    sgr: active_sgr,
                    text: tok_slice,
                });
                cursor = r_end;
            }
            Event::TaskListMarker(_) => {
                if cursor < r_start {
                    f(ClassifiedToken {
                        color: "accent",
                        sgr: None,
                        text: &text[cursor..r_start],
                    });
                }
                f(ClassifiedToken {
                    color: "accent",
                    sgr: Some(MD_SGR_BOLD),
                    text: &text[r_start..r_end],
                });
                cursor = r_end;
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                if cursor < r_start {
                    f(ClassifiedToken {
                        color: "accent",
                        sgr: None,
                        text: &text[cursor..r_start],
                    });
                }
                for_each_html_token(&text[r_start..r_end], f);
                cursor = r_end;
            }
            Event::Rule => {
                if cursor < r_start {
                    f(ClassifiedToken {
                        color: "accent",
                        sgr: None,
                        text: &text[cursor..r_start],
                    });
                }
                f(ClassifiedToken {
                    color: "mdRule",
                    sgr: Some(MD_SGR_BOLD),
                    text: &text[r_start..r_end],
                });
                cursor = r_end;
            }
            _ => {}
        }
    }

    if cursor < text.len() {
        let remaining = &text[cursor..];
        let color = if remaining.contains('|') {
            "border"
        } else if remaining.starts_with('>')
            || remaining.starts_with('-')
            || remaining.starts_with('*')
        {
            "accent"
        } else {
            active_color
        };
        f(ClassifiedToken {
            color,
            sgr: active_sgr,
            text: remaining,
        });
    }
}

fn for_each_html_token<'a>(html: &'a str, f: &mut dyn FnMut(ClassifiedToken<'a>)) {
    static HTML_TAG: OnceLock<Regex> = OnceLock::new();
    let re = HTML_TAG
        .get_or_init(|| Regex::new(r"<(/?[a-zA-Z0-9_-]+)([^>]*)>").expect("html tag regex"));
    let mut last = 0usize;
    let mut in_bold = false;
    let mut in_code = false;

    for m in re.find_iter(html) {
        if m.start() > last {
            let inner_text = &html[last..m.start()];
            let (color, sgr) = if in_code {
                ("syntaxString", None)
            } else if in_bold {
                ("mdBold", Some(MD_SGR_BOLD))
            } else {
                ("text", None)
            };
            f(ClassifiedToken {
                color,
                sgr,
                text: inner_text,
            });
        }

        let tag_str = m.as_str();
        if tag_str.starts_with("<b") || tag_str.starts_with("<strong") {
            in_bold = true;
        } else if tag_str.starts_with("</b") || tag_str.starts_with("</strong") {
            in_bold = false;
        } else if tag_str.starts_with("<code") {
            in_code = true;
        } else if tag_str.starts_with("</code") {
            in_code = false;
        }

        f(ClassifiedToken {
            color: "syntaxKeyword",
            sgr: None,
            text: tag_str,
        });

        last = m.end();
    }

    if last < html.len() {
        let remaining = &html[last..];
        let (color, sgr) = if in_code {
            ("syntaxString", None)
        } else if in_bold {
            ("mdBold", Some(MD_SGR_BOLD))
        } else {
            ("text", None)
        };
        f(ClassifiedToken {
            color,
            sgr,
            text: remaining,
        });
    }
}

pub fn markdown_tokens<'a>(text: &'a str) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_markdown_token(text, |t| out.push(t));
    out
}

pub fn for_each_line_classified_token<'a>(
    text: &'a str,
    file_profile: Option<&LangProfile>,
    region: Option<&super::fence::FenceRegion>,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    use super::registry::lang_profile;
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
                            color: "syntaxComment",
                            sgr: None,
                            text: &text[..m.end()],
                        });
                        for_each_classified_token(&text[m.end()..], Some(p), f);
                        return;
                    }
                    for_each_classified_token(text, Some(p), f);
                }
                _ => for_each_classified_token(text, profile, f),
            }
        }
        None => {
            if file_profile.is_some_and(|p| p.markdown) {
                for_each_markdown_token(text, f);
            } else {
                for_each_classified_token(text, file_profile, f);
            }
        }
    }
}

pub fn classify_line<'a>(
    text: &'a str,
    file_profile: Option<&LangProfile>,
    region: Option<&super::fence::FenceRegion>,
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_line_classified_token(text, file_profile, region, |t| out.push(t));
    out
}
