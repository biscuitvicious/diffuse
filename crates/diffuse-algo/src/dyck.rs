//! Dyck Language Delimiter Balancer and Sequence Geometry.
//!
//! Academic foundation:
//! - Walther von Dyck (1882). "Gruppentheoretische Studien". Mathematische
//!   Annalen, 20(1), 1–44.
//! - Noam Chomsky (1956). "Three Models for the Description of Language". IRE
//!   Transactions on Information Theory, 2(3), 113–124.
//!
//! # Strict Architectural Separation
//! Operates over generic streams `Iterator<Item = T>` and slices `&[T]`.
//! Zero diffuse-domain dependencies. Stack allocation via [`SmallStack`] avoids
//! heap allocation for nesting depths up to 32.

use std::marker::PhantomData;

/// Fast stack with inline array storage avoiding heap allocations for up to `N`
/// elements, with graceful spillover to `Vec` for extreme depths.
#[derive(Clone, Debug)]
pub struct SmallStack<T: Copy + Default, const N: usize> {
    inline: [T; N],
    len: usize,
    spill: Vec<T>,
}

impl<T: Copy + Default, const N: usize> Default for SmallStack<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> SmallStack<T, N> {
    /// Creates a new empty `SmallStack`.
    #[inline]
    pub fn new() -> Self {
        Self {
            inline: [T::default(); N],
            len: 0,
            spill: Vec::new(),
        }
    }

    /// Number of elements in the stack.
    #[inline]
    pub fn len(&self) -> usize {
        self.len + self.spill.len()
    }

    /// Whether the stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.spill.is_empty()
    }

    /// Pushes a value onto the stack.
    #[inline]
    pub fn push(&mut self, val: T) {
        if self.len < N {
            self.inline[self.len] = val;
            self.len += 1;
        } else {
            self.spill.push(val);
        }
    }

    /// Pops a value from the stack.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if let Some(val) = self.spill.pop() {
            Some(val)
        } else if self.len > 0 {
            self.len -= 1;
            Some(self.inline[self.len])
        } else {
            None
        }
    }

    /// Returns a reference to the top element of the stack.
    #[inline]
    pub fn last(&self) -> Option<&T> {
        if let Some(val) = self.spill.last() {
            Some(val)
        } else if self.len > 0 {
            Some(&self.inline[self.len - 1])
        } else {
            None
        }
    }

    /// Clears all elements from the stack.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
        self.spill.clear();
    }
}

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

impl<S: AsRef<str>> DelimiterClassifier<S> for UniversalDelimiterClassifier {
    fn classify(&self, item: &S) -> Option<DelimiterCategory> {
        let s = item.as_ref();
        match s {
            // Parentheses / Round brackets (ASCII & CJK Fullwidth & Math White)
            "(" | "（" | "⦅" => Some(DelimiterCategory::Open(1)),
            ")" | "）" | "⦆" => Some(DelimiterCategory::Close(1)),
            // Square brackets (ASCII & CJK Fullwidth)
            "[" | "［" => Some(DelimiterCategory::Open(2)),
            "]" | "］" => Some(DelimiterCategory::Close(2)),
            // Curly braces (ASCII & CJK Fullwidth & Math White)
            "{" | "｛" | "⦃" => Some(DelimiterCategory::Open(3)),
            "}" | "｝" | "⦄" => Some(DelimiterCategory::Close(3)),
            ">" => Some(DelimiterCategory::LeadingCloser),
            "〈" | "⟨" => Some(DelimiterCategory::Open(5)),
            "〉" | "⟩" => Some(DelimiterCategory::Close(5)),
            "《" | "⟪" => Some(DelimiterCategory::Open(6)),
            "》" | "⟫" => Some(DelimiterCategory::Close(6)),
            // Lenticular and Tortoise Shell brackets (CJK)
            "【" => Some(DelimiterCategory::Open(20)),
            "】" => Some(DelimiterCategory::Close(20)),
            "〔" => Some(DelimiterCategory::Open(21)),
            "〕" => Some(DelimiterCategory::Close(21)),
            "〖" => Some(DelimiterCategory::Open(22)),
            "〗" => Some(DelimiterCategory::Close(22)),
            "〘" => Some(DelimiterCategory::Open(23)),
            "〙" => Some(DelimiterCategory::Close(23)),
            "〚" => Some(DelimiterCategory::Open(24)),
            "〛" => Some(DelimiterCategory::Close(24)),
            // CJK Corner brackets (Quotation marks)
            "「" => Some(DelimiterCategory::Open(27)),
            "」" => Some(DelimiterCategory::Close(27)),
            "『" => Some(DelimiterCategory::Open(28)),
            "』" => Some(DelimiterCategory::Close(28)),
            // Guillemets
            "«" => Some(DelimiterCategory::Open(30)),
            "»" => Some(DelimiterCategory::Close(30)),
            "‹" => Some(DelimiterCategory::Open(31)),
            "›" => Some(DelimiterCategory::Close(31)),
            // Mathematical White Square brackets
            "⟦" => Some(DelimiterCategory::Open(32)),
            "⟧" => Some(DelimiterCategory::Close(32)),
            // Directional quotes (Curved / Smart quotes)
            "‘" | "‚" => Some(DelimiterCategory::Open(40)),
            "’" => Some(DelimiterCategory::Close(40)),
            "“" | "„" => Some(DelimiterCategory::Open(41)),
            "”" => Some(DelimiterCategory::Close(41)),
            // Symmetric ASCII quotes
            "\"" => Some(DelimiterCategory::Symmetric(10)),
            "'" => Some(DelimiterCategory::Symmetric(11)),
            "`" => Some(DelimiterCategory::Symmetric(12)),
            // Separators & Terminators
            "." | "," | ";" | ":" | "，" | "、" | "；" | "؛" | ";" | "：" | "。" | "．" => {
                Some(DelimiterCategory::Separator)
            }
            w if w.chars().all(|c| c.is_whitespace()) => Some(DelimiterCategory::Whitespace),
            _ => None,
        }
    }
}

/// Default classifier for ASCII and standard code token strings.
#[derive(Clone, Copy, Debug, Default)]
pub struct AsciiDelimiterClassifier;

impl<S: AsRef<str>> DelimiterClassifier<S> for AsciiDelimiterClassifier {
    #[inline]
    fn classify(&self, item: &S) -> Option<DelimiterCategory> {
        UniversalDelimiterClassifier.classify(item)
    }
}

/// Trims leading and trailing separators and whitespace, and uses a pushdown
/// automaton to identify the longest prefix forming balanced Dyck containers.
///
/// Returns relative indices `(start, best_end)` into `items`.
pub fn trim_balanced_boundaries<T, C>(items: &[T], classifier: &C) -> (usize, usize)
where
    C: DelimiterClassifier<T>,
{
    if items.is_empty() {
        return (0, 0);
    }

    let mut start = 0;
    while start < items.len()
        && classifier.classify(&items[start]) == Some(DelimiterCategory::Whitespace)
    {
        start += 1;
    }

    let mut end = items.len();
    while end > start && classifier.classify(&items[end - 1]) == Some(DelimiterCategory::Whitespace)
    {
        end -= 1;
    }

    // Trim leading separators and unmatched closing delimiters
    while start < end {
        let cat = classifier.classify(&items[start]);
        match cat {
            Some(
                DelimiterCategory::Separator
                | DelimiterCategory::Close(_)
                | DelimiterCategory::LeadingCloser,
            ) => {
                start += 1;
                while start < end
                    && classifier.classify(&items[start]) == Some(DelimiterCategory::Whitespace)
                {
                    start += 1;
                }
            }
            _ => break,
        }
    }

    if start >= end {
        return (0, 0);
    }

    let mut stack: SmallStack<u8, 32> = SmallStack::new();
    let mut best_end = start;

    for (offset, item) in items[start..end].iter().enumerate() {
        let idx = start + offset;
        let cat = classifier.classify(item);

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                stack.push(id);
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&id) {
                    stack.pop();
                } else {
                    break;
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                if stack.last() == Some(&id) {
                    stack.pop();
                } else {
                    stack.push(id);
                }
            }
            _ => {}
        }

        if stack.is_empty()
            && cat != Some(DelimiterCategory::Whitespace)
            && cat != Some(DelimiterCategory::Separator)
        {
            best_end = idx + 1;
        }
    }

    (start, best_end)
}

/// Computes enclosing container IDs for each item in `items`, continuing
/// from the delimiter state established by `prev_items`. Writes into `out`.
///
/// Returns 0 for items outside any container, or the delimiter ID of the active
/// enclosing container (e.g. 1 for `()`, 2 for `[]`, 3 for `{}`).
pub fn track_enclosing_containers_into<T, C>(
    prev_items: &[T],
    items: &[T],
    classifier: &C,
    out: &mut [u8],
) where
    C: DelimiterClassifier<T>,
{
    debug_assert_eq!(items.len(), out.len());
    let mut stack: SmallStack<(u8, bool), 32> = SmallStack::new();

    for item in prev_items {
        let cat = classifier.classify(item);
        if let Some(&(top_id, true)) = stack.last() {
            if let Some(DelimiterCategory::Symmetric(id)) = cat
                && id == top_id
            {
                stack.pop();
            }
            continue;
        }

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                stack.push((id, false));
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&(id, false)) {
                    stack.pop();
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                stack.push((id, true));
            }
            _ => {}
        }
    }

    for (item, slot) in items.iter().zip(out.iter_mut()) {
        let cat = classifier.classify(item);
        if let Some(&(top_id, true)) = stack.last() {
            if let Some(DelimiterCategory::Symmetric(id)) = cat
                && id == top_id
            {
                stack.pop();
                *slot = id;
            } else {
                *slot = top_id;
            }
            continue;
        }

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                stack.push((id, false));
                *slot = id;
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&(id, false)) {
                    stack.pop();
                    *slot = id;
                } else {
                    *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                stack.push((id, true));
                *slot = id;
            }
            _ => {
                *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
            }
        }
    }
}

/// Computes the active enclosing container ID for each item in a sequence.
///
/// Returns `None` for items at depth 0, or `Some(id)` representing the top
/// of the delimiter stack.
pub fn track_enclosing_containers<T, C>(items: &[T], classifier: &C) -> Vec<Option<u8>>
where
    C: DelimiterClassifier<T>,
{
    let mut out = vec![0u8; items.len()];
    track_enclosing_containers_into(&[], items, classifier, &mut out);
    out.into_iter()
        .map(|id| if id == 0 { None } else { Some(id) })
        .collect()
}

/// Computes delimiter nesting depths for each item in `items`, continuing
/// from the delimiter state established by `prev_items`. Writes into `out`.
///
/// Delimiter depths are assigned using a formal Dyck pushdown automaton:
/// - Opening delimiters (`(`, `[`, `{`) are assigned the depth of their
///   enclosing scope.
/// - Closing delimiters (`)`, `]`, `}`) are assigned the depth of the scope
///   they close to.
/// - Items inside containers are assigned the depth of the active container
///   stack.
pub fn track_nesting_depths_into<T, C>(
    prev_items: &[T],
    items: &[T],
    classifier: &C,
    out: &mut [u16],
) where
    C: DelimiterClassifier<T>,
{
    debug_assert_eq!(items.len(), out.len());
    let mut stack: SmallStack<(u8, bool), 32> = SmallStack::new();

    for item in prev_items {
        let cat = classifier.classify(item);
        if let Some(&(top_id, true)) = stack.last() {
            if let Some(DelimiterCategory::Symmetric(id)) = cat
                && id == top_id
            {
                stack.pop();
            }
            continue;
        }

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                stack.push((id, false));
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&(id, false)) {
                    stack.pop();
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                stack.push((id, true));
            }
            _ => {}
        }
    }

    for (item, slot) in items.iter().zip(out.iter_mut()) {
        let cat = classifier.classify(item);
        if let Some(&(top_id, true)) = stack.last() {
            if let Some(DelimiterCategory::Symmetric(id)) = cat
                && id == top_id
            {
                stack.pop();
                *slot = stack.len() as u16;
            } else {
                *slot = stack.len() as u16;
            }
            continue;
        }

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                *slot = stack.len() as u16;
                stack.push((id, false));
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&(id, false)) {
                    stack.pop();
                    *slot = stack.len() as u16;
                } else {
                    *slot = stack.len() as u16;
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                *slot = stack.len() as u16;
                stack.push((id, true));
            }
            _ => {
                *slot = stack.len() as u16;
            }
        }
    }
}

/// Computes the delimiter nesting depth for each item in a sequence.
pub fn track_nesting_depths_with_prev<T, C>(
    prev_items: &[T],
    items: &[T],
    classifier: &C,
) -> Vec<usize>
where
    C: DelimiterClassifier<T>,
{
    let mut out = vec![0u16; items.len()];
    track_nesting_depths_into(prev_items, items, classifier, &mut out);
    out.into_iter().map(|d| d as usize).collect()
}

/// Dedicated iterator adapter yielding balanced container ranges `(start, end)`
/// in a sequence.
pub struct DyckBalanced<I, C, T> {
    iter: I,
    classifier: C,
    idx: usize,
    _marker: PhantomData<T>,
}

impl<I, C, T> DyckBalanced<I, C, T> {
    pub fn new(iter: I, classifier: C) -> Self {
        Self {
            iter,
            classifier,
            idx: 0,
            _marker: PhantomData,
        }
    }
}

impl<I, C, T> Iterator for DyckBalanced<I, C, T>
where
    I: Iterator<Item = T>,
    C: DelimiterClassifier<T>,
{
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let mut stack: SmallStack<u8, 32> = SmallStack::new();
        let mut container_start = None;

        for item in self.iter.by_ref() {
            let curr_idx = self.idx;
            self.idx += 1;
            let cat = self.classifier.classify(&item);

            match cat {
                Some(DelimiterCategory::Open(id)) => {
                    if stack.is_empty() && container_start.is_none() {
                        container_start = Some(curr_idx);
                    }
                    stack.push(id);
                }
                Some(DelimiterCategory::Close(id)) => {
                    if stack.last() == Some(&id) {
                        stack.pop();
                        if stack.is_empty() && container_start.is_some() {
                            return Some((container_start.take().unwrap(), curr_idx + 1));
                        }
                    } else {
                        // Unmatched close resets state
                        stack.clear();
                        container_start = None;
                    }
                }
                Some(DelimiterCategory::Symmetric(id)) => {
                    if stack.last() == Some(&id) {
                        stack.pop();
                        if stack.is_empty() && container_start.is_some() {
                            return Some((container_start.take().unwrap(), curr_idx + 1));
                        }
                    } else {
                        if stack.is_empty() && container_start.is_none() {
                            container_start = Some(curr_idx);
                        }
                        stack.push(id);
                    }
                }
                _ => {}
            }
        }

        None
    }
}

/// Extension trait providing Dyck delimiter balancing over generic iterators.
pub trait DiffuseDyckExt: Iterator + Sized {
    fn dyck_balanced<C: DelimiterClassifier<Self::Item>>(
        self,
        classifier: C,
    ) -> DyckBalanced<Self, C, Self::Item>;
}

impl<I: Iterator + Sized> DiffuseDyckExt for I {
    fn dyck_balanced<C: DelimiterClassifier<Self::Item>>(
        self,
        classifier: C,
    ) -> DyckBalanced<Self, C, Self::Item> {
        DyckBalanced::new(self, classifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_stack_operations() {
        let mut stack: SmallStack<u8, 4> = SmallStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.last(), None);
        assert_eq!(stack.pop(), None);

        // Push within inline capacity
        stack.push(1);
        stack.push(2);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.last(), Some(&2));

        // Push exceeding inline capacity (triggers spillover)
        stack.push(3);
        stack.push(4);
        stack.push(5);
        stack.push(6);
        assert_eq!(stack.len(), 6);
        assert_eq!(stack.last(), Some(&6));

        // Pop from spillover back into inline
        assert_eq!(stack.pop(), Some(6));
        assert_eq!(stack.pop(), Some(5));
        assert_eq!(stack.last(), Some(&4));
        assert_eq!(stack.len(), 4);

        // Pop inline
        assert_eq!(stack.pop(), Some(4));
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
        assert!(stack.is_empty());

        // Test clear
        stack.push(10);
        stack.push(20);
        stack.clear();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
    }

    #[test]
    fn test_ascii_delimiter_classifier() {
        let c = AsciiDelimiterClassifier;
        assert_eq!(c.classify(&"("), Some(DelimiterCategory::Open(1)));
        assert_eq!(c.classify(&")"), Some(DelimiterCategory::Close(1)));
        assert_eq!(c.classify(&"["), Some(DelimiterCategory::Open(2)));
        assert_eq!(c.classify(&"]"), Some(DelimiterCategory::Close(2)));
        assert_eq!(c.classify(&"{"), Some(DelimiterCategory::Open(3)));
        assert_eq!(c.classify(&"}"), Some(DelimiterCategory::Close(3)));
        assert_eq!(c.classify(&"\""), Some(DelimiterCategory::Symmetric(10)));
        assert_eq!(c.classify(&"'"), Some(DelimiterCategory::Symmetric(11)));
        assert_eq!(c.classify(&"`"), Some(DelimiterCategory::Symmetric(12)));
        assert_eq!(c.classify(&";"), Some(DelimiterCategory::Separator));
        assert_eq!(c.classify(&"   "), Some(DelimiterCategory::Whitespace));
        assert_eq!(c.classify(&"identifier"), None);
    }

    #[test]
    fn test_trim_balanced_boundaries_empty_and_whitespace() {
        let c = AsciiDelimiterClassifier;
        let empty: Vec<&str> = vec![];
        assert_eq!(trim_balanced_boundaries(&empty, &c), (0, 0));

        let ws = vec!["  ", "\t", "\n"];
        assert_eq!(trim_balanced_boundaries(&ws, &c), (0, 0));
    }

    #[test]
    fn test_trim_balanced_boundaries_simple_and_nested() {
        let c = AsciiDelimiterClassifier;
        let tokens = vec!["(", "foo", ")"];
        assert_eq!(trim_balanced_boundaries(&tokens, &c), (0, 3));

        let nested = vec!["(", "[", "foo", "]", ")"];
        assert_eq!(trim_balanced_boundaries(&nested, &c), (0, 5));

        let symmetric = vec!["\"", "hello", "\""];
        assert_eq!(trim_balanced_boundaries(&symmetric, &c), (0, 3));
    }

    #[test]
    fn test_trim_balanced_boundaries_with_whitespace_and_separators() {
        let c = AsciiDelimiterClassifier;
        // Leading whitespace, leading separator and closing delimiter
        let tokens = vec![" ", ".", ")", "(", "foo", ")", ";", " "];
        // start should be index 3 ("("), best_end should be index 6 (after ")")
        assert_eq!(trim_balanced_boundaries(&tokens, &c), (3, 6));
    }

    #[test]
    fn test_trim_balanced_boundaries_mismatch_stops() {
        let c = AsciiDelimiterClassifier;
        // Mismatched closing bracket stops matching
        let tokens = vec!["(", "foo", ")", "]", "bar"];
        assert_eq!(trim_balanced_boundaries(&tokens, &c), (0, 3));
    }

    #[test]
    fn test_track_enclosing_containers() {
        let c = AsciiDelimiterClassifier;
        let tokens = vec!["`", "code", "`", "plain", "\"", "str", "\""];
        let containers = track_enclosing_containers(&tokens, &c);
        assert_eq!(
            containers,
            vec![
                Some(12), // `
                Some(12), // code
                Some(12), // ` (popped)
                None,     // plain
                Some(10), // "
                Some(10), // str
                Some(10), // " (popped)
            ]
        );
    }

    #[test]
    fn test_dyck_balanced_iterator() {
        let c = AsciiDelimiterClassifier;
        let tokens = vec!["(", "a", ")", "+", "[", "b", "]", "+", "\"", "c", "\""];
        let ranges: Vec<(usize, usize)> = tokens.into_iter().dyck_balanced(c).collect();
        assert_eq!(ranges, vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn test_custom_symbol_classifier() {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        enum TestToken {
            StartGroup,
            EndGroup,
            Comma,
            Word,
        }

        let classifier = |tok: &TestToken| match tok {
            TestToken::StartGroup => Some(DelimiterCategory::Open(1)),
            TestToken::EndGroup => Some(DelimiterCategory::Close(1)),
            TestToken::Comma => Some(DelimiterCategory::Separator),
            TestToken::Word => None,
        };

        let seq = vec![
            TestToken::Comma,
            TestToken::StartGroup,
            TestToken::Word,
            TestToken::EndGroup,
            TestToken::Comma,
        ];
        let (start, end) = trim_balanced_boundaries(&seq, &classifier);
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn test_dyck_defaults_and_edge_cases() {
        let default_stack: SmallStack<u8, 8> = SmallStack::default();
        assert!(default_stack.is_empty());

        let default_classifier = AsciiDelimiterClassifier;
        assert_eq!(
            default_classifier.classify(&">"),
            Some(DelimiterCategory::LeadingCloser)
        );

        // Leading closer trimmed
        let tokens = vec![">", "foo", ";"];
        assert_eq!(
            trim_balanced_boundaries(&tokens, &default_classifier),
            (1, 2)
        );

        // Track enclosing with unmatched closer
        let tokens_unmatched = vec![")", "(", "a", ")"];
        let res = track_enclosing_containers(&tokens_unmatched, &default_classifier);
        assert_eq!(res, vec![None, Some(1), Some(1), Some(1)]);

        // Dyck balanced iterator with unmatched close resetting
        let stream = vec![")", "(", "a", ")", "]", "\"", "b", "\""];
        let ranges: Vec<(usize, usize)> = stream
            .into_iter()
            .dyck_balanced(default_classifier)
            .collect();
        assert_eq!(ranges, vec![(1, 4), (5, 8)]);
    }

    #[test]
    fn test_track_nesting_depths() {
        let c = AsciiDelimiterClassifier;
        let tokens = vec!["path", "::", "{", "Path", ",", "PathBuf", "}", ","];
        let depths = track_nesting_depths_with_prev(&[], &tokens, &c);
        assert_eq!(depths, vec![0, 0, 0, 1, 1, 1, 0, 0]);

        let prev = vec!["use", "std", "::", "{"];
        let depths_with_prev = track_nesting_depths_with_prev(&prev, &tokens, &c);
        assert_eq!(depths_with_prev, vec![1, 1, 1, 2, 2, 2, 1, 1]);
    }

    #[test]
    fn test_universal_delimiter_classifier_unicode() {
        let u = UniversalDelimiterClassifier;
        assert_eq!(u.classify(&"（"), Some(DelimiterCategory::Open(1)));
        assert_eq!(u.classify(&"）"), Some(DelimiterCategory::Close(1)));
        assert_eq!(u.classify(&"【"), Some(DelimiterCategory::Open(20)));
        assert_eq!(u.classify(&"】"), Some(DelimiterCategory::Close(20)));
        assert_eq!(u.classify(&"«"), Some(DelimiterCategory::Open(30)));
        assert_eq!(u.classify(&"»"), Some(DelimiterCategory::Close(30)));
        assert_eq!(u.classify(&"⟦"), Some(DelimiterCategory::Open(32)));
        assert_eq!(u.classify(&"⟧"), Some(DelimiterCategory::Close(32)));
        assert_eq!(u.classify(&"“"), Some(DelimiterCategory::Open(41)));
        assert_eq!(u.classify(&"”"), Some(DelimiterCategory::Close(41)));
        assert_eq!(u.classify(&"，"), Some(DelimiterCategory::Separator));
        assert_eq!(u.classify(&"；"), Some(DelimiterCategory::Separator));

        let tokens = vec!["【", "data", "（", "x", "）", "】", "；"];
        let depths = track_nesting_depths_with_prev(&[], &tokens, &u);
        assert_eq!(depths, vec![0, 1, 1, 2, 1, 0, 0]);
    }
}
