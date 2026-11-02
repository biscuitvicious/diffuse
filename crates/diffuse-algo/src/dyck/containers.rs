//! Surrounding and enclosing container tracking and nesting depth algorithms.

use super::{
    classifier::{DelimiterCategory, DelimiterClassifier},
    stack::SmallStack,
};

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

/// Computes the surrounding container ID (the active enclosing container
/// outside the delimiter boundary) for each item in `items`, continuing from
/// the delimiter state established by `prev_items`. Writes into `out`.
///
/// For standard tokens, this equals the active enclosing container. For opening
/// and closing delimiter tokens, this returns the container that surrounds the
/// delimiter pair, rather than the container opened/closed by the delimiter
/// itself.
pub fn track_surrounding_containers_into<T, C>(
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
                *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
            } else {
                *slot = top_id;
            }
            continue;
        }

        match cat {
            Some(DelimiterCategory::Open(id)) => {
                *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
                stack.push((id, false));
            }
            Some(DelimiterCategory::Close(id)) => {
                if stack.last() == Some(&(id, false)) {
                    stack.pop();
                    *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
                } else {
                    *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
                }
            }
            Some(DelimiterCategory::Symmetric(id)) => {
                *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
                stack.push((id, true));
            }
            _ => {
                *slot = stack.last().map(|&(id, _)| id).unwrap_or(0);
            }
        }
    }
}

/// Computes the surrounding container ID for each item in a sequence.
pub fn track_surrounding_containers<T, C>(items: &[T], classifier: &C) -> Vec<Option<u8>>
where
    C: DelimiterClassifier<T>,
{
    let mut out = vec![0u8; items.len()];
    track_surrounding_containers_into(&[], items, classifier, &mut out);
    out.into_iter()
        .map(|id| if id == 0 { None } else { Some(id) })
        .collect()
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
