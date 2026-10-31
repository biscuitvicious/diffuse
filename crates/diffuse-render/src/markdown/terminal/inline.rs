//! Inline Markdown span rendering and task checkbox glyphs.

use std::borrow::Cow;

use diffuse_syntax::color::TokenColor;

use super::types::{InlineStyle, Run};
use crate::syntax::{
    markdown::{Inline, Span, TaskState},
    profile::{SGR_BOLD, SGR_ITALIC, SGR_UNDERLINE},
};

pub(crate) fn task_glyph(state: TaskState) -> &'static str {
    match state {
        TaskState::Empty => "⬜",
        TaskState::InProgress => "🔄",
        TaskState::Checked => "✅",
    }
}

pub(crate) fn inline_runs(inline: &Inline<'_>, base: InlineStyle) -> Vec<Run> {
    let mut runs = Vec::new();
    inline_runs_into(inline, base, &mut runs);
    runs
}

pub(crate) fn inline_runs_into(inline: &Inline<'_>, base: InlineStyle, runs: &mut Vec<Run>) {
    for span in inline.spans() {
        match span {
            Span::Text(text) => {
                if !text.is_empty() {
                    runs.push(Run {
                        color: base.color,
                        sgr: base.sgr,
                        underline: base.underline,
                        strike: base.strike,
                        dim: base.dim,
                        text: Cow::Owned(text.to_string()),
                    });
                }
            }
            Span::Bold(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: TokenColor::Bold,
                    sgr: Some(SGR_BOLD),
                    underline: base.underline,
                    strike: base.strike,
                    dim: base.dim,
                },
                runs,
            ),
            Span::Italic(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: TokenColor::Italic,
                    sgr: Some(SGR_ITALIC),
                    underline: base.underline,
                    strike: base.strike,
                    dim: base.dim,
                },
                runs,
            ),
            Span::Strike(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: base.color,
                    sgr: base.sgr,
                    underline: base.underline,
                    strike: true,
                    dim: true,
                },
                runs,
            ),
            // Surrounding SGR stacks; the color classifies as code.
            Span::Code(code) => runs.push(Run {
                color: TokenColor::Code,
                sgr: base.sgr,
                underline: base.underline,
                strike: base.strike,
                dim: base.dim,
                text: Cow::Owned(code.to_string()),
            }),
            Span::Link { label, url } => {
                inline_runs_into(
                    label,
                    InlineStyle {
                        color: TokenColor::Link,
                        sgr: Some(SGR_UNDERLINE),
                        underline: base.underline,
                        strike: base.strike,
                        dim: base.dim,
                    },
                    runs,
                );
                runs.push(Run {
                    color: TokenColor::LinkBorder,
                    sgr: None,
                    underline: false,
                    strike: base.strike,
                    dim: base.dim,
                    text: Cow::Borrowed(" ("),
                });
                runs.push(Run {
                    color: TokenColor::Link,
                    sgr: Some(SGR_UNDERLINE),
                    underline: base.underline,
                    strike: base.strike,
                    dim: base.dim,
                    text: Cow::Owned(url.to_string()),
                });
                runs.push(Run {
                    color: TokenColor::LinkBorder,
                    sgr: None,
                    underline: false,
                    strike: base.strike,
                    dim: base.dim,
                    text: Cow::Borrowed(")"),
                });
            }
            Span::Task(state) => {
                let mut glyph = String::with_capacity(8);
                glyph.push_str(task_glyph(*state));
                glyph.push(' ');
                runs.push(Run {
                    color: TokenColor::Delimiter,
                    sgr: None,
                    underline: false,
                    strike: false,
                    dim: false,
                    text: Cow::Owned(glyph),
                });
            }
        }
    }
}
