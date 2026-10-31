//! Markdown Token Streaming & Document Classification.
//!
//! Streams classified syntax tokens from Markdown prose, doc-comments, tables,
//! and raw HTML tags using `pulldown-cmark`.

use std::sync::OnceLock;

use regex::Regex;

use super::{
    style::{LayerTag, StyleStack, emit_styled_token},
    table::{emit_table_line_slice, emit_table_row_text_tokens, is_table_alignment_row},
    token::ClassifiedToken,
};
use crate::{
    color::TokenColor,
    profile::{SGR_BOLD, SGR_BOLD_UNDERLINE, SGR_ITALIC, SGR_STRIKE, SGR_UNDERLINE},
};

pub fn for_each_md_inner_token<'a>(
    inner: &'a str,
    parent_sgr: u32,
    parent_color: TokenColor,
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
            color: TokenColor::Delimiter,
            sgr: None,
            text: "`",
        });
        f(ClassifiedToken {
            color: TokenColor::Code,
            sgr: Some(parent_sgr),
            text: &tok[1..tok.len() - 1],
        });
        f(ClassifiedToken {
            color: TokenColor::Delimiter,
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
    parent_color: TokenColor,
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

pub(crate) fn emit_gap<'a>(
    gap: &'a str,
    active_color: TokenColor,
    active_sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if gap.is_empty() {
        return;
    }
    if is_table_alignment_row(gap) {
        f(ClassifiedToken {
            color: TokenColor::TableBorder,
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
                    color: TokenColor::Default,
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
            emit_styled_token(
                &gap[..leading_len],
                TokenColor::Default,
                None,
                in_table_head,
                f,
            );
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
                TokenColor::Delimiter
            } else {
                active_color
            };
            emit_styled_token(trimmed, color, active_sgr, in_table_head, f);
        }
        if trailing_len > 0 {
            emit_styled_token(
                &gap[gap.len() - trailing_len..],
                TokenColor::Default,
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
            (TokenColor::Delimiter, None)
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

pub(crate) fn for_each_markdown_token_dyn_with_header<'a>(
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
                                    color: TokenColor::Delimiter,
                                    sgr: None,
                                    text: hashes,
                                });
                            }
                            if !spaces.is_empty() {
                                f(ClassifiedToken {
                                    color: TokenColor::Default,
                                    sgr: None,
                                    text: spaces,
                                });
                            }
                            cursor = hash_end;
                        }
                    }
                    stack.push(TokenColor::Delimiter, Some(SGR_BOLD), LayerTag::Heading);
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
                                color: TokenColor::Delimiter,
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push(TokenColor::Bold, Some(SGR_BOLD), LayerTag::Strong);
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
                                color: TokenColor::Delimiter,
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push(TokenColor::Italic, Some(SGR_ITALIC), LayerTag::Emphasis);
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
                                color: TokenColor::Delimiter,
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push(
                        TokenColor::Strike,
                        Some(SGR_STRIKE),
                        LayerTag::Strikethrough,
                    );
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
                                color: TokenColor::Delimiter,
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    stack.push(TokenColor::Link, Some(SGR_UNDERLINE), LayerTag::Link);
                }
                Tag::TableHead => {
                    stack.push(
                        TokenColor::TableHeader,
                        Some(SGR_BOLD_UNDERLINE),
                        LayerTag::TableHead,
                    );
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
                                color: TokenColor::Delimiter,
                                sgr,
                                text: gap,
                            });
                        }
                        cursor = r_start;
                    }
                    let (alert_color, alert_sgr) = match alert {
                        Some(BlockQuoteKind::Note | BlockQuoteKind::Tip) => {
                            (TokenColor::Function, Some(SGR_BOLD))
                        }
                        Some(BlockQuoteKind::Important | BlockQuoteKind::Warning) => {
                            (TokenColor::Keyword, Some(SGR_BOLD))
                        }
                        Some(BlockQuoteKind::Caution) => (TokenColor::Type, Some(SGR_BOLD)),
                        None => (TokenColor::Default, None),
                    };
                    stack.push(alert_color, alert_sgr, LayerTag::BlockQuote);
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if cursor < r_end {
                        f(ClassifiedToken {
                            color: TokenColor::Delimiter,
                            sgr: Some(SGR_BOLD),
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
                                color: TokenColor::Delimiter,
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
                                color: TokenColor::Delimiter,
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
                                color: TokenColor::Delimiter,
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
                                TokenColor::Delimiter,
                                delim_sgr,
                                stack.is_in_table_head(),
                                f,
                            );
                            let url_and_paren = &link_tail[close_bracket + 2..];
                            if let Some(paren) = url_and_paren.rfind(')') {
                                emit_styled_token(
                                    &url_and_paren[..paren],
                                    TokenColor::Url,
                                    Some(SGR_UNDERLINE),
                                    stack.is_in_table_head(),
                                    f,
                                );
                                emit_styled_token(
                                    &url_and_paren[paren..],
                                    TokenColor::Delimiter,
                                    delim_sgr,
                                    stack.is_in_table_head(),
                                    f,
                                );
                            } else {
                                emit_styled_token(
                                    url_and_paren,
                                    TokenColor::Url,
                                    Some(SGR_UNDERLINE),
                                    stack.is_in_table_head(),
                                    f,
                                );
                            }
                        } else {
                            emit_styled_token(
                                link_tail,
                                TokenColor::Delimiter,
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
                        TokenColor::Delimiter,
                        tick_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                    emit_styled_token(
                        &code_slice[num_ticks..code_slice.len() - num_ticks],
                        TokenColor::Code,
                        code_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                    emit_styled_token(
                        &code_slice[code_slice.len() - num_ticks..],
                        TokenColor::Delimiter,
                        tick_sgr,
                        stack.is_in_table_head(),
                        f,
                    );
                } else {
                    emit_styled_token(
                        code_slice,
                        TokenColor::Code,
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
                        color: TokenColor::TableBorder,
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
                        color: TokenColor::Delimiter,
                        sgr: None,
                        text: &text[cursor..r_start],
                    });
                }
                f(ClassifiedToken {
                    color: TokenColor::Delimiter,
                    sgr: Some(SGR_BOLD),
                    text: &text[r_start..r_end],
                });
                cursor = r_end;
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                if cursor < r_start {
                    f(ClassifiedToken {
                        color: TokenColor::Delimiter,
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
                        color: TokenColor::Delimiter,
                        sgr: None,
                        text: &text[cursor..r_start],
                    });
                }
                f(ClassifiedToken {
                    color: TokenColor::Rule,
                    sgr: Some(SGR_BOLD),
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
                color: TokenColor::TableBorder,
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
                (TokenColor::Code, None)
            } else if in_bold {
                (TokenColor::Bold, Some(SGR_BOLD))
            } else {
                (TokenColor::Default, None)
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
            color: TokenColor::Keyword,
            sgr: None,
            text: tag_str,
        });

        last = m.end();
    }

    if last < html.len() {
        let remaining = &html[last..];
        let (color, sgr) = if in_code {
            (TokenColor::Code, None)
        } else if in_bold {
            (TokenColor::Bold, Some(SGR_BOLD))
        } else {
            (TokenColor::Default, None)
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
