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
    registry::syntax_token_spans,
};

/// A classified token: the semantic color name + its text (+ an optional
/// SGR attribute — the markdown emphasis: bold/italic/underline/strike).
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedToken {
    pub color: String,
    pub sgr: Option<u32>,
    pub text: String,
}

/// Classify a run of text into semantic tokens (plain `text` when there is
/// no profile).
pub fn classify_tokens(text: &str, profile: Option<&LangProfile>) -> Vec<ClassifiedToken> {
    let Some(profile) = profile else {
        return vec![ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text.into(),
        }];
    };
    if text.is_empty() {
        return vec![ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text.into(),
        }];
    }
    // The hand-rolled scanner produces the same spans as the regex's
    // find_iter (see `syntax_token_spans`) — the regex crate's NFA path
    // on this alternation was the whole classify cost (~50× slower).
    let spans = syntax_token_spans(text, profile);
    let mut out: Vec<ClassifiedToken> = Vec::with_capacity(spans.len());
    let mut last = 0usize;
    for (start, end) in spans {
        if start > last {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: text[last..start].into(),
            });
        }
        let tok = &text[start..end];
        let next = text[end..].chars().next();
        let is_comment = (profile.hash_comment && tok.starts_with('#'))
            || tok.starts_with("//")
            || tok.starts_with("/*");
        if is_comment {
            out.extend(tokenize_comment_with_backticks(tok, profile));
            last = end;
            continue;
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
        out.push(ClassifiedToken {
            color: color.into(),
            sgr: None,
            text: tok.into(),
        });
        last = end;
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text[last..].into(),
        });
    }
    out
}

fn tokenize_comment_with_backticks(tok: &str, profile: &LangProfile) -> Vec<ClassifiedToken> {
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
        return vec![ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: tok.into(),
        }];
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

    let mut out = Vec::new();
    if prefix_len > 0 {
        out.push(ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: tok[..prefix_len].into(),
        });
    }

    let body = &tok[prefix_len..];
    if is_doc {
        // Doc comments: full markdown (emphasis, links, code) — the text
        // gaps between markdown spans are comment-colored.
        for mut t in markdown_tokens(body) {
            if t.color == "text" {
                t.color = "syntaxComment".into();
            } else if t.color == "syntaxString" {
                let parent_sgr = t.sgr;
                let mut inner_toks = classify_tokens(&t.text, Some(inner_prof));
                for it in &mut inner_toks {
                    if it.sgr.is_none() {
                        it.sgr = parent_sgr;
                    }
                }
                out.extend(inner_toks);
                continue;
            }
            out.push(t);
        }
    } else {
        // Plain comments: backtick-only. Everything outside `` `code` `` is
        // comment-color; inside, the language's classification.
        out.extend(backtick_tokens(body, inner_prof));
    }
    out
}

/// Backtick-only extraction for plain (`//`, `/*`, `#`) comments: only
/// inline `` `code` `` spans render — accent backticks + language-classified
/// inner content; the surrounding comment text stays `syntaxComment`. No
/// markdown emphasis/link/strike (plain comments aren't markdown).
fn backtick_tokens(body: &str, profile: &LangProfile) -> Vec<ClassifiedToken> {
    static BACKTICK: OnceLock<Regex> = OnceLock::new();
    let re = BACKTICK.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the comment backtick"));
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(body) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "syntaxComment".into(),
                sgr: None,
                text: body[last..m.start()].into(),
            });
        }
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        let inner = &m.as_str()[1..m.as_str().len() - 1];
        out.extend(classify_tokens(inner, Some(profile)));
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        last = m.end();
    }
    if last < body.len() {
        out.push(ClassifiedToken {
            color: "syntaxComment".into(),
            sgr: None,
            text: body[last..].into(),
        });
    }
    out
}

/// Parse the inner content of a bold/italic/link span, stacking the parent
/// SGR on top of any inline code children. The child's *color* wins; the
/// parent's *SGR* (bold, italic, underline) is inherited — so e.g.
/// `**`code`**` renders as code-color + bold rather than mdBold-color + bold.
pub fn md_inner_tokens(inner: &str, parent_sgr: u32, parent_color: &str) -> Vec<ClassifiedToken> {
    static INNER_CODE: OnceLock<Regex> = OnceLock::new();
    let re = INNER_CODE.get_or_init(|| Regex::new(r"`[^`\n]+`").expect("the inner code"));
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(inner) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: parent_color.into(),
                sgr: Some(parent_sgr),
                text: inner[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        // `code` inside bold/italic: code color wins, parent SGR stacks.
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        out.push(ClassifiedToken {
            color: "syntaxString".into(),
            sgr: Some(parent_sgr),
            text: tok[1..tok.len() - 1].into(),
        });
        out.push(ClassifiedToken {
            color: "accent".into(),
            sgr: None,
            text: "`".into(),
        });
        last = m.end();
    }
    if last < inner.len() {
        out.push(ClassifiedToken {
            color: parent_color.into(),
            sgr: Some(parent_sgr),
            text: inner[last..].into(),
        });
    }
    out
}

/// The markdown tokens — the md default region's classifier (the renderer's
/// `markdownTokens` ported): `#`-headings (the marker muted, the title
/// accent + bold), inline code (syntax-string), `**bold**`, `*italic*`,
/// `~~strike~~`, `[text](url)` — the SGR rides the token.
pub fn markdown_tokens(text: &str) -> Vec<ClassifiedToken> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static MD_INLINE: OnceLock<Regex> = OnceLock::new();
    let heading =
        HEADING.get_or_init(|| Regex::new(r"^(#{1,6})(\s+)(.*)$").expect("the md heading"));
    if let Some(h) = heading.captures(text) {
        return vec![
            ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: h[1].to_string(),
            },
            ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: h[2].to_string(),
            },
            ClassifiedToken {
                color: "accent".into(),
                sgr: Some(MD_SGR_BOLD),
                text: h[3].to_string(),
            },
        ];
    }
    let re = MD_INLINE.get_or_init(|| {
        Regex::new(
            r"(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\](?:\([^)\n]+\))?)",
        )
        .expect("the md inline")
    });
    let mut out: Vec<ClassifiedToken> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: text[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        if tok.starts_with('`') {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
            out.push(ClassifiedToken {
                color: "syntaxString".into(),
                sgr: None,
                text: tok[1..tok.len() - 1].into(),
            });
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
        } else if tok.starts_with("~~") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_STRIKE,
                "mdStrike",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
        } else if tok.starts_with("**") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_BOLD,
                "mdBold",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
        } else if tok.starts_with('[') {
            if let Some(close) = tok.find("](") {
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "[".into(),
                });
                out.extend(md_inner_tokens(&tok[1..close], MD_SGR_UNDERLINE, "mdLink"));
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "](".into(),
                });
                out.push(ClassifiedToken {
                    color: "mdUrl".into(),
                    sgr: Some(MD_SGR_UNDERLINE),
                    text: tok[close + 2..tok.len() - 1].into(),
                });
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: ")".into(),
                });
            } else {
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "[".into(),
                });
                out.extend(md_inner_tokens(
                    &tok[1..tok.len() - 1],
                    MD_SGR_UNDERLINE,
                    "mdLink",
                ));
                out.push(ClassifiedToken {
                    color: "accent".into(),
                    sgr: None,
                    text: "]".into(),
                });
            }
        } else {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
            out.extend(md_inner_tokens(
                &tok[1..tok.len() - 1],
                MD_SGR_ITALIC,
                "mdItalic",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
        }

        last = m.end();
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: text[last..].into(),
        });
    }
    out
}

/// The full-line classifier: the file's default region (md → the markdown
/// tokens, else the profile), or a fence region's language (with the file's
/// comment prefix stripped first — `/// let x = 5;` → `let x = 5;`).
pub fn classify_line(
    text: &str,
    file_profile: Option<&LangProfile>,
    region: Option<&super::fence::FenceRegion>,
) -> Vec<ClassifiedToken> {
    use super::registry::lang_profile;
    match region {
        Some(r) => {
            let profile = r.lang.as_deref().and_then(lang_profile);
            match (
                profile.as_ref(),
                file_profile.and_then(|p| p.fence_strip.as_ref()),
            ) {
                (Some(p), Some(strip)) => {
                    let mut out: Vec<ClassifiedToken> = Vec::new();
                    if let Some(m) = strip.find(text)
                        && m.start() == 0
                        && m.end() > 0
                    {
                        out.push(ClassifiedToken {
                            color: "syntaxComment".into(),
                            sgr: None,
                            text: text[..m.end()].into(),
                        });
                        out.extend(classify_tokens(&text[m.end()..], Some(p)));
                        return out;
                    }
                    classify_tokens(text, Some(p))
                }
                _ => classify_tokens(text, profile),
            }
        }
        None => {
            if file_profile.is_some_and(|p| p.markdown) {
                // The md default region is the markdown prose.
                markdown_tokens(text)
            } else {
                classify_tokens(text, file_profile)
            }
        }
    }
}
