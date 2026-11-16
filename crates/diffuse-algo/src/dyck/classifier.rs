//! Delimiter categories and classification traits for Dyck languages.

/// Category of a delimiter symbol in a formal Dyck language.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DelimiterCategory {
    /// An opening delimiter with an ID (e.g. 1 for `(`, 2 for `[`, 3 for `{`).
    Open(u8),
    /// A closing delimiter with a matching ID.
    Close(u8),
    /// A symmetric delimiter that toggles container state (e.g. quotes `'`,
    /// `"`, `` ` ``).
    Symmetric(u8),
    /// A separator / punctuation mark (e.g. `.`, `,`, `;`, `:`).
    Separator,
    /// A delimiter that is trimmed when leading (e.g. leftover closing `>`),
    /// but permitted in balanced bodies.
    LeadingCloser,
    /// Whitespace token.
    Whitespace,
    /// An escape character (e.g. `\`).
    Escape,
}

/// Trait classifying items into formal delimiter categories.
pub trait DelimiterClassifier<T> {
    /// Classifies an item into its delimiter category, or `None` if ordinary
    /// content.
    fn classify(&self, item: &T) -> Option<DelimiterCategory>;
}

impl<F, T> DelimiterClassifier<T> for F
where
    F: Fn(&T) -> Option<DelimiterCategory>,
{
    #[inline]
    fn classify(&self, item: &T) -> Option<DelimiterCategory> {
        self(item)
    }
}

/// Universal classifier for ASCII and Unicode structural delimiters, paired
/// brackets, directional quotation marks, and statement terminators.
#[derive(Clone, Copy, Debug, Default)]
pub struct UniversalDelimiterClassifier;

impl UniversalDelimiterClassifier {
    /// Classifies a character into its delimiter category.
    #[inline]
    pub fn classify_char(&self, ch: char) -> Option<DelimiterCategory> {
        match ch {
            // Parentheses / Round brackets (ASCII & CJK Fullwidth & Math White)
            '(' | '（' | '⦅' => Some(DelimiterCategory::Open(1)),
            ')' | '）' | '⦆' => Some(DelimiterCategory::Close(1)),
            // Square brackets (ASCII & CJK Fullwidth)
            '[' | '［' => Some(DelimiterCategory::Open(2)),
            ']' | '］' => Some(DelimiterCategory::Close(2)),
            // Curly braces (ASCII & CJK Fullwidth & Math White)
            '{' | '｛' | '⦃' => Some(DelimiterCategory::Open(3)),
            '}' | '｝' | '⦄' => Some(DelimiterCategory::Close(3)),
            '>' => Some(DelimiterCategory::LeadingCloser),
            '〈' | '⟨' => Some(DelimiterCategory::Open(5)),
            '〉' | '⟩' => Some(DelimiterCategory::Close(5)),
            '《' | '⟪' => Some(DelimiterCategory::Open(6)),
            '》' | '⟫' => Some(DelimiterCategory::Close(6)),
            // Lenticular and Tortoise Shell brackets (CJK)
            '【' => Some(DelimiterCategory::Open(20)),
            '】' => Some(DelimiterCategory::Close(20)),
            '〔' => Some(DelimiterCategory::Open(21)),
            '〕' => Some(DelimiterCategory::Close(21)),
            '〖' => Some(DelimiterCategory::Open(22)),
            '〗' => Some(DelimiterCategory::Close(22)),
            '〘' => Some(DelimiterCategory::Open(23)),
            '〙' => Some(DelimiterCategory::Close(23)),
            '〚' => Some(DelimiterCategory::Open(24)),
            '〛' => Some(DelimiterCategory::Close(24)),
            // CJK Corner brackets (Quotation marks)
            '「' => Some(DelimiterCategory::Open(27)),
            '」' => Some(DelimiterCategory::Close(27)),
            '『' => Some(DelimiterCategory::Open(28)),
            '』' => Some(DelimiterCategory::Close(28)),
            // Guillemets
            '«' => Some(DelimiterCategory::Open(30)),
            '»' => Some(DelimiterCategory::Close(30)),
            '‹' => Some(DelimiterCategory::Open(31)),
            '›' => Some(DelimiterCategory::Close(31)),
            // Mathematical White Square brackets
            '⟦' => Some(DelimiterCategory::Open(32)),
            '⟧' => Some(DelimiterCategory::Close(32)),
            // Directional quotes (Curved / Smart quotes)
            '‘' | '‚' => Some(DelimiterCategory::Open(40)),
            '’' => Some(DelimiterCategory::Close(40)),
            '“' | '„' => Some(DelimiterCategory::Open(41)),
            '”' => Some(DelimiterCategory::Close(41)),
            // Symmetric ASCII quotes
            '"' => Some(DelimiterCategory::Symmetric(10)),
            '\'' => Some(DelimiterCategory::Symmetric(11)),
            '`' => Some(DelimiterCategory::Symmetric(12)),
            // Escape character
            '\\' => Some(DelimiterCategory::Escape),
            // Separators & Terminators
            '.' | ',' | ';' | ':' | '，' | '、' | '；' | '؛' | ';' | '：' | '。' | '．' => {
                Some(DelimiterCategory::Separator)
            }
            c if c.is_whitespace() => Some(DelimiterCategory::Whitespace),
            _ => None,
        }
    }

    /// Classifies a string slice into its delimiter category.
    #[inline]
    pub fn classify_str(&self, s: &str) -> Option<DelimiterCategory> {
        let mut chars = s.chars();
        if let (Some(first), None) = (chars.next(), chars.next()) {
            return self.classify_char(first);
        }
        if s.chars().all(|c| c.is_whitespace()) {
            return Some(DelimiterCategory::Whitespace);
        }
        None
    }
}

impl DelimiterClassifier<char> for UniversalDelimiterClassifier {
    #[inline]
    fn classify(&self, item: &char) -> Option<DelimiterCategory> {
        self.classify_char(*item)
    }
}

impl DelimiterClassifier<&str> for UniversalDelimiterClassifier {
    #[inline]
    fn classify(&self, item: &&str) -> Option<DelimiterCategory> {
        self.classify_str(item)
    }
}

impl DelimiterClassifier<String> for UniversalDelimiterClassifier {
    #[inline]
    fn classify(&self, item: &String) -> Option<DelimiterCategory> {
        self.classify_str(item)
    }
}
