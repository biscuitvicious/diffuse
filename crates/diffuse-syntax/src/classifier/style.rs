//! Markdown Style Layer Stack & Token Styler.
//!
//! Tracks nested markdown emphasis layers (headings, tables, bold, italic,
//! links, and blockquotes) and emits styled tokens with stacked SGR attributes.

use super::token::ClassifiedToken;
use crate::{
    color::TokenColor,
    profile::{SGR_BOLD, SGR_BOLD_UNDERLINE, SGR_UNDERLINE},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerTag {
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
pub(crate) struct StyleLayer {
    pub(crate) color: TokenColor,
    pub(crate) sgr: Option<u32>,
    pub(crate) tag: LayerTag,
}

pub(crate) struct StyleStack {
    layers: Vec<StyleLayer>,
}

impl StyleStack {
    pub(crate) fn new(is_header: bool) -> Self {
        let mut layers = Vec::with_capacity(8);
        if is_header {
            layers.push(StyleLayer {
                color: TokenColor::TableHeader,
                sgr: Some(SGR_BOLD_UNDERLINE),
                tag: LayerTag::TableHead,
            });
        } else {
            layers.push(StyleLayer {
                color: TokenColor::Default,
                sgr: None,
                tag: LayerTag::Base,
            });
        }
        Self { layers }
    }

    pub(crate) fn push(&mut self, color: TokenColor, sgr: Option<u32>, tag: LayerTag) {
        self.layers.push(StyleLayer { color, sgr, tag });
    }

    pub(crate) fn pop_tag(&mut self, tag: LayerTag) {
        if let Some(pos) = self.layers.iter().rposition(|l| l.tag == tag) {
            self.layers.remove(pos);
        }
    }

    pub(crate) fn is_in_table_head(&self) -> bool {
        self.layers.iter().any(|l| l.tag == LayerTag::TableHead)
    }

    pub(crate) fn current_color(&self) -> TokenColor {
        self.layers
            .last()
            .map(|l| l.color)
            .unwrap_or(TokenColor::Default)
    }

    pub(crate) fn current_sgr(&self) -> Option<u32> {
        let has_bold = self
            .layers
            .iter()
            .any(|l| matches!(l.sgr, Some(SGR_BOLD | SGR_BOLD_UNDERLINE)));
        let has_underline = self
            .layers
            .iter()
            .any(|l| matches!(l.sgr, Some(SGR_UNDERLINE | SGR_BOLD_UNDERLINE)));

        if has_bold && has_underline {
            return Some(SGR_BOLD_UNDERLINE);
        }
        if has_bold {
            return Some(SGR_BOLD);
        }
        if has_underline {
            return Some(SGR_UNDERLINE);
        }

        for layer in self.layers.iter().rev() {
            if let Some(s) = layer.sgr {
                return Some(s);
            }
        }
        None
    }
}

pub(crate) fn emit_styled_token<'a>(
    text: &'a str,
    color: TokenColor,
    sgr: Option<u32>,
    in_table_head: bool,
    f: &mut dyn FnMut(ClassifiedToken<'a>),
) {
    if text.is_empty() {
        return;
    }
    if text.chars().all(char::is_whitespace) {
        let (ws_color, ws_sgr) = if in_table_head {
            (TokenColor::Default, None)
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

    if in_table_head && matches!(sgr, Some(SGR_UNDERLINE | SGR_BOLD_UNDERLINE)) {
        let trimmed_start = text.trim_start();
        let leading_spaces_len = text.len() - trimmed_start.len();
        if leading_spaces_len > 0 {
            f(ClassifiedToken {
                color: TokenColor::Default,
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
                color: TokenColor::Default,
                sgr: None,
                text: &text[text.len() - trailing_spaces_len..],
            });
        }
    } else {
        f(ClassifiedToken { color, sgr, text });
    }
}
