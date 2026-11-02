//! Delimiter pairing and boundary trimming algorithms.

use super::{
    classifier::{DelimiterCategory, DelimiterClassifier},
    stack::SmallStack,
};

/// Computes matching delimiter partner indices into `out`.
///
/// For an opening delimiter at index `i`, sets `out[i] = Some(j)` where `j` is
/// its matching closing delimiter. For a closing delimiter at index `j`, sets
/// `out[j] = Some(i)` where `i` is its matching opening delimiter. For
/// non-delimiters or unmatched delimiters, sets `None`.
pub fn compute_delimiter_partners_into<T, C>(items: &[T], classifier: &C, out: &mut [Option<usize>])
where
    C: DelimiterClassifier<T>,
{
    debug_assert_eq!(items.len(), out.len());
    out.fill(None);
    let mut stack: SmallStack<(usize, u8), 32> = SmallStack::new();

    for (idx, item) in items.iter().enumerate() {
        match classifier.classify(item) {
            Some(DelimiterCategory::Open(id)) => {
                stack.push((idx, id));
            }
            Some(DelimiterCategory::Close(id)) => {
                if let Some(&(open_idx, open_id)) = stack.last()
                    && open_id == id
                {
                    stack.pop();
                    out[open_idx] = Some(idx);
                    out[idx] = Some(open_idx);
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                if let Some(&(open_idx, open_id)) = stack.last()
                    && open_id == id
                {
                    stack.pop();
                    out[open_idx] = Some(idx);
                    out[idx] = Some(open_idx);
                } else {
                    stack.push((idx, id));
                }
            }
            _ => {}
        }
    }
}

/// Computes the matching delimiter partner index for each delimiter in `items`.
///
/// For an opening delimiter at index `i`, returns `Some(j)` where `j` is its
/// matching closing delimiter. For a closing delimiter at index `j`, returns
/// `Some(i)` where `i` is its matching opening delimiter. For non-delimiters or
/// unmatched delimiters, returns `None`.
pub fn compute_delimiter_partners<T, C>(items: &[T], classifier: &C) -> Vec<Option<usize>>
where
    C: DelimiterClassifier<T>,
{
    let mut partners = vec![None; items.len()];
    compute_delimiter_partners_into(items, classifier, &mut partners);
    partners
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
