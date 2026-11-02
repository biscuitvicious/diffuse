//! Universal and profile-aware delimiter predicates.

use super::lang::LangProfile;

/// Whether `c` is a recognized opening delimiter (ASCII or Unicode paired
/// bracket / quote opener).
#[inline]
pub fn is_opening_delimiter_char(c: char) -> bool {
    matches!(
        c,
        '(' | '['
            | '{'
            | '（'
            | '［'
            | '｛'
            | '〈'
            | '《'
            | '【'
            | '〔'
            | '〖'
            | '〘'
            | '〚'
            | '「'
            | '『'
            | '«'
            | '‹'
            | '⟦'
            | '⟨'
            | '⟪'
            | '⦃'
            | '⦅'
            | '‘'
            | '‚'
            | '“'
            | '„'
    )
}

/// Whether `c` is a recognized closing delimiter (ASCII or Unicode paired
/// bracket / quote closer).
#[inline]
pub fn is_closing_delimiter_char(c: char) -> bool {
    matches!(
        c,
        ')' | ']'
            | '}'
            | '）'
            | '］'
            | '｝'
            | '〉'
            | '》'
            | '】'
            | '〕'
            | '〖'
            | '〙'
            | '〛'
            | '」'
            | '』'
            | '»'
            | '›'
            | '⟧'
            | '⟩'
            | '⟫'
            | '⦄'
            | '⦆'
            | '’'
            | '”'
            | ';'
            | '；'
            | '؛'
            | ';'
            | ','
            | '，'
            | '、'
    )
}

/// Whether `c` is a statement terminator or closing statement delimiter.
#[inline]
pub fn is_statement_terminator_char(c: char) -> bool {
    matches!(
        c,
        ';' | '；' | '؛' | ';' | '。' | '．' | ')' | '）' | '}' | '｝' | ']' | '］' | '】' | '』'
    )
}

/// Whether `c` is a syntax delimiter, consulting `profile` when present or
/// checking universal paired brackets and terminators otherwise.
#[inline]
pub fn is_delimiter_char(c: char, profile: Option<&LangProfile>) -> bool {
    profile.map_or_else(
        || is_opening_delimiter_char(c) || is_closing_delimiter_char(c),
        |p| p.is_delimiter_char(c),
    )
}

/// Whether `text` consists solely of delimiter characters and optional
/// whitespace.
pub fn is_delimiter_run(text: &str, profile: Option<&LangProfile>) -> bool {
    let t = text.trim();
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_whitespace() || is_delimiter_char(c, profile))
}
