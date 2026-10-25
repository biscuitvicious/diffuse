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
    /// Operator (`opr`)
    Operator,
    /// Keyword (`kwd`)
    Keyword,
    /// Function / method (`fun`)
    Function,
    /// Type / struct / enum (`typ`)
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
    /// Bold text (`bld`)
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
    /// Table border (`tbo`)
    TableBorder,
    /// Table header (`thd`)
    TableHeader,
    /// Link border / brackets (`lbo`)
    LinkBorder,
    /// Quote border / blockquote chevron (`qbo`)
    QuoteBorder,
    /// Fence border / code block fence (`fbo`)
    FenceBorder,
}

impl TokenColor {
    /// Returns `true` if this is the unstyled default text color.
    #[inline]
    pub const fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }

    /// The canonical 3-letter wire code, or `None` if `TokenColor::Default`.
    #[inline]
    pub const fn wire_code(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Operator => Some("opr"),
            Self::Keyword => Some("kwd"),
            Self::Function => Some("fun"),
            Self::Type => Some("typ"),
            Self::String => Some("str"),
            Self::CharLiteral => Some("chr"),
            Self::Number => Some("num"),
            Self::Literal => Some("lit"),
            Self::Comment => Some("com"),
            Self::Bold => Some("bld"),
            Self::Italic => Some("ita"),
            Self::Link => Some("lnk"),
            Self::Url => Some("url"),
            Self::Rule => Some("rul"),
            Self::Delimiter => Some("dlm"),
            Self::Strike => Some("stk"),
            Self::Code => Some("cde"),
            Self::TableBorder => Some("tbo"),
            Self::TableHeader => Some("thd"),
            Self::LinkBorder => Some("lbo"),
            Self::QuoteBorder => Some("qbo"),
            Self::FenceBorder => Some("fbo"),
        }
    }

    /// Parse from a wire string. Handles legacy `"txt"` and empty `""`
    /// gracefully as `Default`.
    pub fn from_wire_code(s: &str) -> Self {
        match s {
            "opr" => Self::Operator,
            "kwd" => Self::Keyword,
            "fun" => Self::Function,
            "typ" => Self::Type,
            "str" => Self::String,
            "chr" => Self::CharLiteral,
            "num" => Self::Number,
            "lit" => Self::Literal,
            "com" => Self::Comment,
            "bld" => Self::Bold,
            "ita" => Self::Italic,
            "lnk" => Self::Link,
            "url" => Self::Url,
            "rul" => Self::Rule,
            "dlm" => Self::Delimiter,
            "stk" => Self::Strike,
            "cde" => Self::Code,
            "tbo" => Self::TableBorder,
            "thd" => Self::TableHeader,
            "lbo" => Self::LinkBorder,
            "qbo" => Self::QuoteBorder,
            "fbo" => Self::FenceBorder,
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
