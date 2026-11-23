//! Semantic token color classifications and wire contract representations.

use std::{convert::Infallible, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Semantic token color classification across syntax and markdown renderers.
///
/// Encoded as a 1-byte enum (`#[repr(u8)]`) supporting `Copy` with zero
/// allocations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenColor {
    /// Unstyled base text color matching the line/row context (e.g. normal
    /// text, addition text, deletion text). Serializes to `null` on the
    /// wire.
    #[default]
    Default,

    // --- Syntax Colors ---
    /// Operator (`op`)
    Operator,
    /// Keyword (`kw`)
    Keyword,
    /// Function / method (`fn`)
    Function,
    /// Type / struct / enum (`ty`)
    Type,
    /// String literal (`str`)
    String,
    /// Character literal (`chr`)
    CharLiteral,
    /// Number / numeric literal (`num`)
    Number,
    /// Literal value token (`lit`)
    Literal,
    /// Comment (`com`)
    Comment,

    // --- Markdown & Structural Diff Colors ---
    /// Bold text (`bold`)
    Bold,
    /// Italic text (`ita`)
    Italic,
    /// Link text (`lnk`)
    Link,
    /// URL text (`url`)
    Url,
    /// Thematic rule / separator (`rul`)
    Rule,
    /// Delimiters & punctuation (`dlm`)
    Delimiter,
    /// Strikethrough text (`stk`)
    Strike,
    /// Inline code span (`cde`)
    Code,
    /// Table border (`tbd`)
    TableBorder,
    /// Table header (`thd`)
    TableHeader,
    /// Link border / brackets (`lbd`)
    LinkBorder,
    /// Quote border / blockquote chevron (`qbd`)
    QuoteBorder,
    /// Fence border / code block fence (`fbd`)
    FenceBorder,
}

impl TokenColor {
    /// Returns `true` if this is the unstyled default text color.
    #[inline]
    pub const fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }

    /// Returns `true` if this token color can absorb trailing whitespace.
    #[inline]
    pub const fn allows_trailing_whitespace(self) -> bool {
        matches!(self, Self::Keyword | Self::Operator | Self::Type)
    }

    /// Returns `true` if this token color can absorb leading whitespace.
    #[inline]
    pub const fn allows_leading_whitespace(self) -> bool {
        matches!(self, Self::Operator)
    }

    /// The canonical wire code, or `None` if `TokenColor::Default`.
    #[inline]
    pub const fn wire_code(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Operator => Some("op"),
            Self::Keyword => Some("kw"),
            Self::Function => Some("fn"),
            Self::Type => Some("ty"),
            Self::String => Some("str"),
            Self::CharLiteral => Some("chr"),
            Self::Number => Some("num"),
            Self::Literal => Some("lit"),
            Self::Comment => Some("com"),
            Self::Bold => Some("bold"),
            Self::Italic => Some("ita"),
            Self::Link => Some("lnk"),
            Self::Url => Some("url"),
            Self::Rule => Some("rul"),
            Self::Delimiter => Some("dlm"),
            Self::Strike => Some("stk"),
            Self::Code => Some("cde"),
            Self::TableBorder => Some("tbd"),
            Self::TableHeader => Some("thd"),
            Self::LinkBorder => Some("lbd"),
            Self::QuoteBorder => Some("qbd"),
            Self::FenceBorder => Some("fbd"),
        }
    }

    /// Parse from a wire string. Returns `Default` for unrecognized codes.
    pub fn from_wire_code(s: &str) -> Self {
        match s {
            "op" => Self::Operator,
            "kw" => Self::Keyword,
            "fn" => Self::Function,
            "ty" => Self::Type,
            "str" => Self::String,
            "chr" => Self::CharLiteral,
            "num" => Self::Number,
            "lit" => Self::Literal,
            "com" => Self::Comment,
            "bold" => Self::Bold,
            "ita" => Self::Italic,
            "lnk" => Self::Link,
            "url" => Self::Url,
            "rul" => Self::Rule,
            "dlm" => Self::Delimiter,
            "stk" => Self::Strike,
            "cde" => Self::Code,
            "tbd" => Self::TableBorder,
            "thd" => Self::TableHeader,
            "lbd" => Self::LinkBorder,
            "qbd" => Self::QuoteBorder,
            "fbd" => Self::FenceBorder,
            _ => Self::Default,
        }
    }
}

impl FromStr for TokenColor {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_wire_code(s))
    }
}

impl fmt::Display for TokenColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = self.wire_code() {
            f.write_str(code)
        } else {
            f.write_str("")
        }
    }
}

impl Serialize for TokenColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.wire_code() {
            Some(code) => serializer.serialize_str(code),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for TokenColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<&str>::deserialize(deserializer)?;
        Ok(opt.map(Self::from_wire_code).unwrap_or(Self::Default))
    }
}
