//! Consolidated syntax and markdown token palette.
//!
//! Defines syntax token coloring across programming languages and markdown
//! structural tokens.

use yansi::Color;

use crate::syntax::color::TokenColor;

/// Complete token classification palette for syntax and markdown formatting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxPalette {
    pub text: Color,
    pub keyword: Color,
    pub typ: Color,
    pub function: Color,
    pub number: Color,
    pub literal: Color,
    pub string: Color,
    pub char_literal: Color,
    pub comment: Color,
    pub operator: Color,
    pub bold: Color,
    pub italic: Color,
    pub strike: Color,
    pub link: Color,
    pub url: Color,
    pub delim: Color,
    pub rule: Color,
    pub code: Color,
    pub table_border: Color,
    pub table_header: Color,
    pub link_border: Color,
    pub quote_border: Color,
    pub fence_border: Color,
}

impl SyntaxPalette {
    /// Canonical dark theme syntax palette.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            text: Color::Primary,
            keyword: Color::Fixed(147),
            typ: Color::Fixed(220),
            function: Color::Fixed(123),
            number: Color::Fixed(221),
            literal: Color::Fixed(221),
            string: Color::Fixed(120),
            char_literal: Color::Fixed(158),
            comment: Color::Fixed(150),
            operator: Color::Fixed(117),
            bold: Color::Fixed(214),
            italic: Color::Fixed(229),
            strike: Color::Fixed(250),
            link: Color::Fixed(117),
            url: Color::Fixed(250),
            delim: Color::Fixed(51),
            rule: Color::Fixed(220),
            code: Color::Fixed(81),
            table_border: Color::Fixed(243),
            table_header: Color::Fixed(229),
            link_border: Color::Fixed(243),
            quote_border: Color::Fixed(243),
            fence_border: Color::Fixed(243),
        }
    }

    /// Canonical light theme syntax palette.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            text: Color::Primary,
            keyword: Color::Rgb(170, 20, 120),
            typ: Color::Rgb(15, 110, 130),
            function: Color::Rgb(10, 85, 185),
            number: Color::Rgb(165, 75, 0),
            literal: Color::Rgb(165, 75, 0),
            string: Color::Rgb(20, 115, 40),
            char_literal: Color::Rgb(16, 128, 67),
            comment: Color::Rgb(105, 115, 130),
            operator: Color::Rgb(65, 75, 90),
            bold: Color::Rgb(180, 75, 0),
            italic: Color::Rgb(125, 60, 150),
            strike: Color::Rgb(130, 135, 145),
            link: Color::Rgb(10, 85, 185),
            url: Color::Rgb(100, 105, 115),
            delim: Color::Rgb(15, 110, 130),
            rule: Color::Rgb(185, 190, 200),
            code: Color::Rgb(12, 100, 125),
            table_border: Color::Rgb(101, 109, 118),
            table_header: Color::Rgb(125, 60, 150),
            link_border: Color::Rgb(101, 109, 118),
            quote_border: Color::Rgb(101, 109, 118),
            fence_border: Color::Rgb(101, 109, 118),
        }
    }

    /// Resolve the active [`Color`] for a semantic [`TokenColor`].
    ///
    /// Returns `None` for unstyled text (`TokenColor::Default`) matching
    /// `Color::Primary`, or for `TokenColor::Strike` which applies ANSI
    /// strikethrough/dim attributes rather than a dedicated foreground color.
    #[must_use]
    pub const fn token_color(&self, color: TokenColor) -> Option<Color> {
        match color {
            TokenColor::Default => match self.text {
                Color::Primary => None,
                colored => Some(colored),
            },
            TokenColor::Keyword => Some(self.keyword),
            TokenColor::Type => Some(self.typ),
            TokenColor::Function => Some(self.function),
            TokenColor::Number => Some(self.number),
            TokenColor::Literal => Some(self.literal),
            TokenColor::String => Some(self.string),
            TokenColor::CharLiteral => Some(self.char_literal),
            TokenColor::Comment => Some(self.comment),
            TokenColor::Operator => Some(self.operator),
            TokenColor::Bold => Some(self.bold),
            TokenColor::Italic => Some(self.italic),
            TokenColor::Strike => None,
            TokenColor::Link => Some(self.link),
            TokenColor::Url => Some(self.url),
            TokenColor::Delimiter => Some(self.delim),
            TokenColor::Rule => Some(self.rule),
            TokenColor::Code => Some(self.code),
            TokenColor::TableBorder => Some(self.table_border),
            TokenColor::TableHeader => Some(self.table_header),
            TokenColor::LinkBorder => Some(self.link_border),
            TokenColor::QuoteBorder => Some(self.quote_border),
            TokenColor::FenceBorder => Some(self.fence_border),
        }
    }
}

impl Default for SyntaxPalette {
    #[inline]
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_palette_presets() {
        let dark = SyntaxPalette::dark();
        assert_eq!(dark.keyword, Color::Fixed(147));
        assert_eq!(dark.string, Color::Fixed(120));

        let light = SyntaxPalette::light();
        assert_eq!(light.keyword, Color::Rgb(170, 20, 120));
        assert_eq!(light.string, Color::Rgb(20, 115, 40));

        let def = SyntaxPalette::default();
        assert_eq!(def, dark);
    }

    #[test]
    fn test_syntax_palette_token_color_mapping() {
        let mut p = SyntaxPalette::dark();
        assert_eq!(p.token_color(TokenColor::Default), None);

        // When text is not Primary, it is returned
        p.text = Color::Fixed(120);
        assert_eq!(p.token_color(TokenColor::Default), Some(Color::Fixed(120)));

        assert_eq!(p.token_color(TokenColor::Keyword), Some(p.keyword));
        assert_eq!(p.token_color(TokenColor::Type), Some(p.typ));
        assert_eq!(p.token_color(TokenColor::Function), Some(p.function));
        assert_eq!(p.token_color(TokenColor::Number), Some(p.number));
        assert_eq!(p.token_color(TokenColor::Literal), Some(p.literal));
        assert_eq!(p.token_color(TokenColor::String), Some(p.string));
        assert_eq!(p.token_color(TokenColor::CharLiteral), Some(p.char_literal));
        assert_eq!(p.token_color(TokenColor::Comment), Some(p.comment));
        assert_eq!(p.token_color(TokenColor::Operator), Some(p.operator));
        assert_eq!(p.token_color(TokenColor::Bold), Some(p.bold));
        assert_eq!(p.token_color(TokenColor::Italic), Some(p.italic));
        assert_eq!(p.token_color(TokenColor::Strike), None);
        assert_eq!(p.token_color(TokenColor::Link), Some(p.link));
        assert_eq!(p.token_color(TokenColor::Url), Some(p.url));
        assert_eq!(p.token_color(TokenColor::Delimiter), Some(p.delim));
        assert_eq!(p.token_color(TokenColor::Rule), Some(p.rule));
        assert_eq!(p.token_color(TokenColor::Code), Some(p.code));
        assert_eq!(p.token_color(TokenColor::TableBorder), Some(p.table_border));
        assert_eq!(p.token_color(TokenColor::TableHeader), Some(p.table_header));
        assert_eq!(p.token_color(TokenColor::LinkBorder), Some(p.link_border));
        assert_eq!(p.token_color(TokenColor::QuoteBorder), Some(p.quote_border));
        assert_eq!(p.token_color(TokenColor::FenceBorder), Some(p.fence_border));
    }
}
