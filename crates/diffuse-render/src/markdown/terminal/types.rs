//! Data models for rendered visual lines, styled runs, and styling contexts.

use std::borrow::Cow;

use diffuse_syntax::color::TokenColor;

/// Static space padding: indent runs borrow slices, never allocate.
pub(crate) static SPACES: &str = "                                                                ";
/// Static quote bars (`▎`, U+258E, 3 bytes each): bar runs borrow slices.
pub(crate) static BARS: &str = "▎▎▎▎▎▎▎▎";

/// A rendered visual line of Markdown: styled runs whose total display
/// width fits the requested content width.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownLine {
    pub runs: Vec<MarkdownRun>,
}

/// A styled run in a rendered Markdown line. Mirrors [`OutputRun`] with an
/// added `underline` flag: heading titles stack bold and underline, which a
/// single SGR slot cannot express. The writer combines them into one escape.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownRun {
    pub color: TokenColor,
    pub sgr: Option<u32>,
    pub underline: bool,
    pub strike: bool,
    pub dim: bool,
    pub text: Cow<'static, str>,
}

/// A styled run with `'static` text: content copies (`Owned`) or static
/// pieces (`Borrowed` — bars, indents, bullets, markers). `underline` stacks
/// with `sgr` (heading titles are bold and underlined); the writer combines
/// them into one escape.
#[derive(Clone)]
pub(crate) struct Run {
    pub(crate) color: TokenColor,
    pub(crate) sgr: Option<u32>,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) dim: bool,
    pub(crate) text: Cow<'static, str>,
}

/// A visual line before flattening: bars, indent, and an optional leading
/// marker (list bullet/number, heading hashes) stay structural so parents
/// (quotes, list items) can compose without string surgery.
pub(crate) struct VisualLine {
    pub(crate) bars: usize,
    pub(crate) indent: usize,
    pub(crate) marker: Vec<Run>,
    pub(crate) content: Vec<Run>,
}

/// Inline style context; nested emphasis replaces color and SGR
/// (innermost span wins — a single SGR fits `OutputRun`), while `underline`
/// propagates so a whole heading title stays underlined through nested
/// emphasis.
#[derive(Clone, Copy)]
pub(crate) struct InlineStyle {
    pub(crate) color: TokenColor,
    pub(crate) sgr: Option<u32>,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) dim: bool,
}

/// Borrowed indent run from the static padding slice (`Owned` past 64).
pub(crate) fn borrowed_spaces(n: usize) -> Run {
    if n <= SPACES.len() {
        Run {
            color: TokenColor::Default,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
            text: Cow::Borrowed(&SPACES[..n]),
        }
    } else {
        Run {
            color: TokenColor::Default,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
            text: Cow::Owned(" ".repeat(n)),
        }
    }
}

pub(crate) fn owned_spaces(n: usize) -> Cow<'static, str> {
    if n <= SPACES.len() {
        Cow::Borrowed(&SPACES[..n])
    } else {
        Cow::Owned(" ".repeat(n))
    }
}
