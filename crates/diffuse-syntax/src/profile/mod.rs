//! Language profile data model — the static description of a programming
//! language's syntactic shape (comment style, block delimiters, keywords,
//! function signature patterns, and fence/region rules).

pub mod block;
pub mod delimiters;
pub mod lang;
pub mod literal;

pub use block::{
    BRACE_EXACT_DELIMITERS, BRACE_TAIL_DELIMITERS, BRACE_TRAILING_DELIMITERS, BlockDelimStyle,
    CloseDelimiter, INDENT_COLON_EXACT_DELIMITERS, INDENT_COLON_TAIL_DELIMITERS,
    INDENT_COLON_TRAILING_DELIMITERS, KEYWORDS_EXACT_DELIMITERS, KEYWORDS_TAIL_DELIMITERS,
    KEYWORDS_TRAILING_DELIMITERS, PropertySyntax, default_is_continuation_phrase,
};
pub use delimiters::{
    is_closing_delimiter_char, is_delimiter_char, is_delimiter_run, is_opening_delimiter_char,
    is_statement_terminator_char,
};
pub use lang::LangProfile;
pub use literal::{CharLiteralProfile, SingleQuoteKind, UnicodeEscapeStyle};

/// The ANSI SGR attributes used in styled runs.
pub const SGR_BOLD: u32 = 1;
pub const SGR_DIM: u32 = 2;
pub const SGR_ITALIC: u32 = 3;
pub const SGR_UNDERLINE: u32 = 4;
pub const SGR_STRIKE: u32 = 9;
pub const SGR_BOLD_UNDERLINE: u32 = 14;
