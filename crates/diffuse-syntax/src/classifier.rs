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
    profile::{
        LangProfile, MD_SGR_BOLD, MD_SGR_BOLD_UNDERLINE, MD_SGR_ITALIC, MD_SGR_STRIKE,
        MD_SGR_UNDERLINE,
    },
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

fn for_each_classified_token_dyn<'a>(
    text: &'a str,
    profile: Option<&LangProfile>,
    is_header: bool,
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
            for_each_comment_with_backticks(tok, profile, is_header, f);
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
        for_each_markdown_token_dyn_with_header(body, is_header, &mut |mut t| {
            if t.color == "text" {
                t.color = "syntaxComment";
            } else if t.color == "syntaxString" {
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
        for_each_classified_token_dyn(inner, Some(profile), false, f);
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
    for_each_markdown_token_dyn_with_header(text, false, &mut f);
}

pub fn for_each_markdown_token_with_header<'a>(
    text: &'a str,
    is_header: bool,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_markdown_token_dyn_with_header(text, is_header, &mut f);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerTag {
    Base,
    Heading,
    TableHead,
    Strong,
    Emphasis,
    Strikethrough,
    Link,
    BlockQuote,
}

#[derive(Clone, Copy, Debug)]
struct StyleLayer {
    color: &'static str,
    sgr: Option<u32>,
    tag: LayerTag,
}

struct StyleStack {
    layers: Vec<StyleLayer>,
}

impl StyleStack {
    fn new(is_header: bool) -> Self {
        let mut layers = Vec::with_capacity(8);
        if is_header {
            layers.push(StyleLayer {
                color: "mdItalic",
                sgr: Some(MD_SGR_BOLD_UNDERLINE),
                tag: LayerTag::TableHead,
            });
        } else {
            layers.push(StyleLayer {
                color: "text",
                sgr: None,
                tag: LayerTag::Base,
            });
        }
        Self { layers }
    }

    fn push(&mut self, color: &'static str, sgr: Option<u32>, tag: LayerTag) {
        self.layers.push(StyleLayer { color, sgr, tag });
    }

    fn pop_tag(&mut self, tag: LayerTag) {
        if let Some(pos) = self.layers.iter().rposition(|l| l.tag == tag) {
            self.layers.remove(pos);
        }
    }

    fn is_in_table_head(&self) -> bool {
        self.layers.iter().any(|l| l.tag == LayerTag::TableHead)
    }

    fn current_color(&self) -> &'static str {
        self.layers.last().map(|l| l.color).unwrap_or("text")
    }

    fn current_sgr(&self) -> Option<u32> {
        let has_bold = self
            .layers
            .iter()
            .any(|l| matches!(l.sgr, Some(MD_SGR_BOLD | MD_SGR_BOLD_UNDERLINE)));
        let has_underline = self
            .layers
            .iter()
            .any(|l| matches!(l.sgr, Some(MD_SGR_UNDERLINE | MD_SGR_BOLD_UNDERLINE)));

        if has_bold && has_underline {
            return Some(MD_SGR_BOLD_UNDERLINE);
        }
        if has_bold {
            return Some(MD_SGR_BOLD);
        }
        if has_underline {
            return Some(MD_SGR_UNDERLINE);
        }

        for layer in self.layers.iter().rev() {
            if let Some(s) = layer.sgr {
                return Some(s);
            }
        }
        None
    }
}

fn emit_styled_token<'a>(
    text: &'a str,
    color: &'static str,
    sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if text.is_empty() {
        return;
    }
    if text.chars().all(char::is_whitespace) {
        let (ws_color, ws_sgr) = if in_table_head {
            ("text", None)
        } else {
            (color, sgr)
        };
        f(ClassifiedToken {
            color: ws_color,
            sgr: ws_sgr,
            text,
        });
        return;
    }

    if in_table_head && matches!(sgr, Some(MD_SGR_UNDERLINE | MD_SGR_BOLD_UNDERLINE)) {
        let trimmed_start = text.trim_start();
        let leading_spaces_len = text.len() - trimmed_start.len();
        if leading_spaces_len > 0 {
            f(ClassifiedToken {
                color: "text",
                sgr: None,
                text: &text[..leading_spaces_len],
            });
        }

        let trimmed = trimmed_start.trim_end();
        let trailing_spaces_len = trimmed_start.len() - trimmed.len();

        if !trimmed.is_empty() {
            f(ClassifiedToken {
                color,
                sgr,
                text: trimmed,
            });
        }

        if trailing_spaces_len > 0 {
            f(ClassifiedToken {
                color: "text",
                sgr: None,
                text: &text[text.len() - trailing_spaces_len..],
            });
        }
    } else {
        f(ClassifiedToken { color, sgr, text });
    }
}

fn emit_table_line_slice<'a>(
    slice: &'a str,
    active_color: &'static str,
    active_sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if is_table_alignment_row(slice) {
        f(ClassifiedToken {
            color: "muted",
            sgr: None,
            text: slice,
        });
        return;
    }
    let mut last = 0usize;
    for (i, c) in slice.char_indices() {
        if c == '|' {
            if i > last {
                let cell_part = &slice[last..i];
                let trimmed = cell_part.trim();
                if !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '-' || ch == ':') {
                    f(ClassifiedToken {
                        color: "muted",
                        sgr: None,
                        text: cell_part,
                    });
                } else {
                    emit_styled_token(cell_part, active_color, active_sgr, in_table_head, f);
                }
            }
            f(ClassifiedToken {
                color: "muted",
                sgr: None,
                text: &slice[i..i + 1],
            });
            last = i + 1;
        }
    }
    if last < slice.len() {
        let cell_part = &slice[last..];
        let trimmed = cell_part.trim();
        if !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '-' || ch == ':') {
            f(ClassifiedToken {
                color: "muted",
                sgr: None,
                text: cell_part,
            });
        } else {
            emit_styled_token(cell_part, active_color, active_sgr, in_table_head, f);
        }
    }
}

fn emit_gap<'a>(
    gap: &'a str,
    active_color: &'static str,
    active_sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if gap.is_empty() {
        return;
    }
    if is_table_alignment_row(gap) {
        f(ClassifiedToken {
            color: "muted",
            sgr: None,
            text: gap,
        });
        return;
    }
    if gap.contains('\n') {
        let mut last = 0usize;
        for (i, c) in gap.char_indices() {
            if c == '\n' {
                if i > last {
                    emit_gap(&gap[last..i], active_color, active_sgr, in_table_head, f);
                }
                f(ClassifiedToken {
                    color: "text",
                    sgr: None,
                    text: &gap[i..i + 1],
                });
                last = i + 1;
            }
        }
        if last < gap.len() {
            emit_gap(&gap[last..], active_color, active_sgr, in_table_head, f);
        }
        return;
    }
    if gap.contains('|') {
        emit_table_line_slice(gap, active_color, active_sgr, in_table_head, f);
    } else if in_table_head {
        let trimmed_start = gap.trim_start();
        let leading_len = gap.len() - trimmed_start.len();
        if leading_len > 0 {
            emit_styled_token(&gap[..leading_len], "text", None, in_table_head, f);
        }
        let trimmed = trimmed_start.trim_end();
        let trailing_len = trimmed_start.len() - trimmed.len();
        if !trimmed.is_empty() {
            let color = if trimmed.starts_with('>')
                || trimmed.starts_with('-')
                || trimmed.starts_with('*')
                || trimmed.starts_with('_')
                || trimmed.starts_with('#')
                || trimmed.starts_with('[')
                || trimmed.ends_with(']')
                || trimmed.starts_with('`')
                || trimmed.starts_with('~')
            {
                "accent"
            } else {
                active_color
            };
            emit_styled_token(trimmed, color, active_sgr, in_table_head, f);
        }
        if trailing_len > 0 {
            emit_styled_token(
                &gap[gap.len() - trailing_len..],
                "text",
                None,
                in_table_head,
                f,
            );
        }
    } else {
        let (color, sgr) = if gap.starts_with('>')
            || gap.starts_with('-')
            || gap.starts_with('*')
            || gap.starts_with('_')
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
            color,
            sgr,
            text: gap,
        });
    }
}

fn for_each_markdown_token_dyn_with_header<'a>(
    text: &'a str,
    is_header: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
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
    let mut stack = StyleStack::new(is_header);

    for (event, range) in parser.into_offset_iter() {
        let r_start = range.start.min(text.len());
        let r_end = range.end.min(text.len());

        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { .. } => {
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
                    stack.push("accent", Some(MD_SGR_BOLD), LayerTag::Heading);
                }
                Tag::Strong => {
                    if cursor < r_start {
                        let gap = &text[cursor..r_start];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push("mdBold", Some(MD_SGR_BOLD), LayerTag::Strong);
                }
                Tag::Emphasis => {
                    if cursor < r_start {
                        let gap = &text[cursor..r_start];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push("mdItalic", Some(MD_SGR_ITALIC), LayerTag::Emphasis);
                }
                Tag::Strikethrough => {
                    if cursor < r_start {
                        let gap = &text[cursor..r_start];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push("mdStrike", Some(MD_SGR_STRIKE), LayerTag::Strikethrough);
                }
                Tag::Link { .. } => {
                    if cursor < r_start {
                        let gap = &text[cursor..r_start];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push("mdLink", Some(MD_SGR_UNDERLINE), LayerTag::Link);
                }
                Tag::TableHead => {
                    stack.push("mdItalic", Some(MD_SGR_BOLD_UNDERLINE), LayerTag::TableHead);
                }
                Tag::BlockQuote(alert) => {
                    if cursor < r_start {
                        let gap = &text[cursor..r_start];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    let (alert_color, alert_sgr) = match alert {
                        Some(BlockQuoteKind::Note | BlockQuoteKind::Tip) => {
                            ("syntaxFunction", Some(MD_SGR_BOLD))
                        }
                        Some(BlockQuoteKind::Important | BlockQuoteKind::Warning) => {
                            ("syntaxKeyword", Some(MD_SGR_BOLD))
                        }
                        Some(BlockQuoteKind::Caution) => ("syntaxType", Some(MD_SGR_BOLD)),
                        None => ("text", None),
                    };
                    stack.push(alert_color, alert_sgr, LayerTag::BlockQuote);
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if cursor < r_end {
                        f(ClassifiedToken {
                            color: "accent",
                            sgr: Some(MD_SGR_BOLD),
                            text: &text[cursor..r_end],
                        });
                        cursor = r_end;
                    }
                    stack.pop_tag(LayerTag::Heading);
                }
                TagEnd::Strong => {
                    stack.pop_tag(LayerTag::Strong);
                    if cursor < r_end {
                        let gap = &text[cursor..r_end];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_end;
                    }
                }
                TagEnd::Emphasis => {
                    stack.pop_tag(LayerTag::Emphasis);
                    if cursor < r_end {
                        let gap = &text[cursor..r_end];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_end;
                    }
                }
                TagEnd::Strikethrough => {
                    stack.pop_tag(LayerTag::Strikethrough);
                    if cursor < r_end {
                        let gap = &text[cursor..r_end];
                        if gap.contains('|') {
                            emit_gap(
                                gap,
                                stack.current_color(),
                                stack.current_sgr(),
                                stack.is_in_table_head(),
                                f,
                            );
                        } else {
                            let sgr = if stack.is_in_table_head() {
                                stack.current_sgr()
                            } else {
                                None
                            };
                            f(ClassifiedToken {
                                color: "accent",
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_end;
                    }
                }
                TagEnd::Link => {
                    stack.pop_tag(LayerTag::Link);
                    if cursor < r_end {
                        let link_tail = &text[cursor..r_end];
                        let delim_sgr = if stack.is_in_table_head() {
                            stack.current_sgr()
                        } else {
                            None
                        };
                        if let Some(close_bracket) = link_tail.find("](") {
                            emit_styled_token(
                                &link_tail[..close_bracket + 2],
                                "accent",
                                delim_sgr,
                                stack.is_in_table_head(),
                                f,
                            );
                            let url_and_paren = &link_tail[close_bracket + 2..];
                            if let Some(paren) = url_and_paren.rfind(')') {
                                emit_styled_token(
                                    &url_and_paren[..paren],
                                    "mdUrl",
                                    Some(MD_SGR_UNDERLINE),
                                    stack.is_in_table_head(),
                                    f,
                                );
                                emit_styled_token(
                                    &url_and_paren[paren..],
                                    "accent",
                                    delim_sgr,
                                    stack.is_in_table_head(),
                                    f,
                                );
                            } else {
                                emit_styled_token(
                                    url_and_paren,
                                    "mdUrl",
                                    Some(MD_SGR_UNDERLINE),
                                    stack.is_in_table_head(),
                                    f,
                                );
                            }
                        } else {
                            emit_styled_token(
                                link_tail,
                                "accent",
                                delim_sgr,
                                stack.is_in_table_head(),
                                f,
                            );
                        }
                        cursor = r_end;
                    }
                }
                TagEnd::TableHead => {
                    if cursor < r_end {
                        emit_gap(
                            &text[cursor..r_end],
                            stack.current_color(),
                            stack.current_sgr(),
                            stack.is_in_table_head(),
                            f,
                        );
                        cursor = r_end;
                    }
                    stack.pop_tag(LayerTag::TableHead);
                }
                TagEnd::TableRow => {
                    if cursor < r_end {
                        emit_gap(
                            &text[cursor..r_end],
                            stack.current_color(),
                            stack.current_sgr(),
                            stack.is_in_table_head(),
                            f,
                        );
                        cursor = r_end;
                    }
                }
                TagEnd::BlockQuote(_) => {
                    stack.pop_tag(LayerTag::BlockQuote);
                }
                _ => {}
            },
            Event::Code(_) => {
                if cursor < r_start {
                    emit_gap(
                        &text[cursor..r_start],
                        stack.current_color(),
                        stack.current_sgr(),
                        stack.is_in_table_head(),
                        f,
                    );
                }
                let code_slice = &text[r_start..r_end];
                let num_ticks = code_slice.chars().take_while(|&c| c == '`').count();
                let code_sgr = stack.current_sgr();
                let tick_sgr = if stack.is_in_table_head() {
                    code_sgr
                } else {
                    None
                };
                if num_ticks > 0 && code_slice.len() >= num_ticks * 2 {
                    emit_styled_token(
                        &code_slice[..num_ticks],
                        "accent",
                        tick_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                    emit_styled_token(
                        &code_slice[num_ticks..code_slice.len() - num_ticks],
                        "syntaxString",
                        code_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                    emit_styled_token(
                        &code_slice[code_slice.len() - num_ticks..],
                        "accent",
                        tick_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                } else {
                    emit_styled_token(
                        code_slice,
                        "syntaxString",
                        code_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                }
                cursor = r_end;
            }
            Event::Text(_) => {
                if cursor < r_start {
                    emit_gap(
                        &text[cursor..r_start],
                        stack.current_color(),
                        stack.current_sgr(),
                        stack.is_in_table_head(),
                        f,
                    );
                }
                let tok_slice = &text[r_start..r_end];
                if is_table_alignment_row(text) || is_table_alignment_row(tok_slice) {
                    f(ClassifiedToken {
                        color: "muted",
                        sgr: None,
                        text: tok_slice,
                    });
                } else if tok_slice.contains('|') {
                    emit_table_row_text_tokens(
                        tok_slice,
                        stack.current_color(),
                        stack.current_sgr(),
                        stack.is_in_table_head(),
                        f,
                    );
                } else {
                    emit_styled_token(
                        tok_slice,
                        stack.current_color(),
                        stack.current_sgr(),
                        stack.is_in_table_head(),
                        f,
                    );
                }
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
        if is_table_alignment_row(remaining) || is_table_alignment_row(text) {
            f(ClassifiedToken {
                color: "muted",
                sgr: None,
                text: remaining,
            });
        } else if remaining.contains('|') {
            emit_table_row_text_tokens(
                remaining,
                stack.current_color(),
                stack.current_sgr(),
                stack.is_in_table_head(),
                f,
            );
        } else {
            emit_gap(
                remaining,
                stack.current_color(),
                stack.current_sgr(),
                stack.is_in_table_head(),
                f,
            );
        }
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
    f: impl FnMut(ClassifiedToken<'a>),
) {
    for_each_line_classified_token_with_next(text, None, file_profile, region, f);
}

pub fn for_each_line_classified_token_with_next<'a>(
    text: &'a str,
    next_line: Option<&str>,
    file_profile: Option<&LangProfile>,
    region: Option<&super::fence::FenceRegion>,
    mut f: impl FnMut(ClassifiedToken<'a>),
) {
    let is_header = next_line.is_some_and(is_line_alignment_row);
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
    region: Option<&super::fence::FenceRegion>,
) -> Vec<ClassifiedToken<'a>> {
    classify_line_with_next(text, None, file_profile, region)
}

pub fn classify_line_with_next<'a>(
    text: &'a str,
    next_line: Option<&str>,
    file_profile: Option<&LangProfile>,
    region: Option<&super::fence::FenceRegion>,
) -> Vec<ClassifiedToken<'a>> {
    let mut out = Vec::new();
    for_each_line_classified_token_with_next(text, next_line, file_profile, region, |t| {
        out.push(t)
    });
    out
}

pub fn markdown_document_tokens<'a>(text: &'a str) -> Vec<Vec<ClassifiedToken<'a>>> {
    if text.is_empty() {
        return vec![Vec::new()];
    }

    let mut lines: Vec<Vec<ClassifiedToken<'a>>> = vec![Vec::new()];

    for_each_markdown_token(text, |tok| {
        if !tok.text.contains('\n') {
            if !tok.text.is_empty() {
                lines.last_mut().unwrap().push(tok);
            }
            return;
        }

        let mut parts = tok.text.split('\n');
        if let Some(first) = parts.next()
            && !first.is_empty()
        {
            lines.last_mut().unwrap().push(ClassifiedToken {
                color: tok.color,
                sgr: tok.sgr,
                text: first,
            });
        }
        for part in parts {
            lines.push(Vec::new());
            if !part.is_empty() {
                lines.last_mut().unwrap().push(ClassifiedToken {
                    color: tok.color,
                    sgr: tok.sgr,
                    text: part,
                });
            }
        }
    });

    lines
}

pub fn is_line_alignment_row(line: &str) -> bool {
    let s = line.trim();
    let stripped = s.trim_start_matches(['/', '*', '#', '!', ' ']);
    is_table_alignment_row(stripped)
}

fn is_table_alignment_row(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() || !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    if !trimmed
        .chars()
        .all(|c| c == '|' || c == ':' || c == '-' || c.is_whitespace())
    {
        return false;
    }
    let cols: Vec<&str> = trimmed
        .split('|')
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .collect();
    !cols.is_empty() && cols.iter().all(|c| c.contains('-'))
}

fn emit_table_row_text_tokens<'a>(
    tok_slice: &'a str,
    active_color: &'static str,
    active_sgr: Option<u32>,
    is_header: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    let (cell_color, cell_sgr) = if is_header {
        ("mdItalic", Some(MD_SGR_BOLD_UNDERLINE))
    } else {
        (active_color, active_sgr)
    };
    emit_table_line_slice(tok_slice, cell_color, cell_sgr, is_header, f);
}
