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
fn test_universal_delimiter_classifier_ascii() {
    let c = UniversalDelimiterClassifier;
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
    let c = UniversalDelimiterClassifier;
    let empty: Vec<&str> = vec![];
    assert_eq!(trim_balanced_boundaries(&empty, &c), (0, 0));

    let ws = vec!["  ", "\t", "\n"];
    assert_eq!(trim_balanced_boundaries(&ws, &c), (0, 0));
}

#[test]
fn test_trim_balanced_boundaries_simple_and_nested() {
    let c = UniversalDelimiterClassifier;
    let tokens = vec!["(", "foo", ")"];
    assert_eq!(trim_balanced_boundaries(&tokens, &c), (0, 3));

    let nested = vec!["(", "[", "foo", "]", ")"];
    assert_eq!(trim_balanced_boundaries(&nested, &c), (0, 5));

    let symmetric = vec!["\"", "hello", "\""];
    assert_eq!(trim_balanced_boundaries(&symmetric, &c), (0, 3));
}

#[test]
fn test_trim_balanced_boundaries_with_whitespace_and_separators() {
    let c = UniversalDelimiterClassifier;
    // Leading whitespace, leading separator and closing delimiter
    let tokens = vec![" ", ".", ")", "(", "foo", ")", ";", " "];
    // start should be index 3 ("("), best_end should be index 6 (after ")")
    assert_eq!(trim_balanced_boundaries(&tokens, &c), (3, 6));
}

#[test]
fn test_trim_balanced_boundaries_mismatch_stops() {
    let c = UniversalDelimiterClassifier;
    // Mismatched closing bracket stops matching
    let tokens = vec!["(", "foo", ")", "]", "bar"];
    assert_eq!(trim_balanced_boundaries(&tokens, &c), (0, 3));
}

#[test]
fn test_track_enclosing_containers() {
    let c = UniversalDelimiterClassifier;
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
fn test_track_enclosing_containers_nested_delimiters() {
    let c = UniversalDelimiterClassifier;
    // [ foo ( bar ) ]
    let tokens = vec!["[", "foo", "(", "bar", ")", "]"];
    let enclosing = track_enclosing_containers(&tokens, &c);
    assert_eq!(
        enclosing,
        vec![Some(2), Some(2), Some(1), Some(1), Some(1), Some(2)]
    );

    let surrounding = track_surrounding_containers(&tokens, &c);
    // [ and ] have surrounding None (0)
    // foo has surrounding Some(2) (Bracket)
    // ( has surrounding Some(2) (Bracket)
    // bar has surrounding Some(1) (Paren)
    // ) has surrounding Some(2) (Bracket)
    assert_eq!(
        surrounding,
        vec![None, Some(2), Some(2), Some(1), Some(2), None]
    );
}

#[test]
fn test_compute_delimiter_partners() {
    let c = UniversalDelimiterClassifier;
    // [ foo ( bar ) ]
    let tokens = vec!["[", "foo", "(", "bar", ")", "]"];
    let partners = compute_delimiter_partners(&tokens, &c);
    assert_eq!(
        partners,
        vec![Some(5), None, Some(4), None, Some(2), Some(0)]
    );

    // Unmatched opener: ( a ( b )
    let unclosed = vec!["(", "a", "(", "b", ")"];
    let p_unclosed = compute_delimiter_partners(&unclosed, &c);
    assert_eq!(p_unclosed, vec![None, None, Some(4), None, Some(2)]);
}

#[test]
fn test_dyck_balanced_iterator() {
    let c = UniversalDelimiterClassifier;
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

    let default_classifier = UniversalDelimiterClassifier;
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
    let c = UniversalDelimiterClassifier;
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

#[test]
fn test_terminal_dyck_stack_and_small_stack_equality() {
    let u = UniversalDelimiterClassifier;

    // SmallStack equality and count_matches
    let mut s1: SmallStack<u8, 4> = SmallStack::new();
    let mut s2: SmallStack<u8, 4> = SmallStack::new();
    assert_eq!(s1, s2);
    s1.push(1);
    s1.push(2);
    s2.push(1);
    assert_ne!(s1, s2);
    s2.push(2);
    assert_eq!(s1, s2);
    assert_eq!(s1.count_matches(|&x| x == 1), 1);
    assert_eq!(s1.count_matches(|&x| x == 3), 0);

    // Terminal Dyck stack over characters
    // Case 1: balanced inner delimiters, leaves '{' open
    let code_add = "TuiCommand::MouseScrollUp { n, row, col } => {";
    let stack_add = terminal_dyck_stack(code_add.chars(), &u);
    assert_eq!(stack_add.len(), 1);
    assert_eq!(stack_add.last(), Some(&(3, false)));

    // Case 2: del with parens and trailing brace
    let code_del = "TuiCommand::MouseScrollUp(n) => {";
    let stack_del = terminal_dyck_stack(code_del.chars(), &u);
    assert_eq!(stack_del.len(), 1);
    assert_eq!(stack_del.last(), Some(&(3, false)));
    assert_eq!(stack_add, stack_del);

    // Case 3: Strings with escapes do not unbalance delimiters
    let str_code = r#"let s = "brace: { escaped: \"{\""; ("#;
    let stack_str = terminal_dyck_stack(str_code.chars(), &u);
    assert_eq!(stack_str.len(), 1);
    assert_eq!(stack_str.last(), Some(&(1, false))); // only '(' is active

    // SmallStack::iter over inline and spillover
    let mut s3: SmallStack<u8, 2> = SmallStack::new();
    s3.push(10);
    s3.push(20);
    s3.push(30); // spillover to Vec
    let items: Vec<u8> = s3.iter().copied().collect();
    assert_eq!(items, vec![10, 20, 30]);
}

#[test]
fn test_has_trailing_delimiter_expansion() {
    let u = UniversalDelimiterClassifier;

    // Both lines end with '{', same net count -> false (scrollbar-grab-drag case)
    let del1 = "TuiCommand::MouseScrollUp(n) => {";
    let add1 = "TuiCommand::MouseScrollUp { n, row, col } => {";
    assert!(!super::containers::has_trailing_delimiter_expansion(
        add1, del1, &u
    ));

    // add opens a new '{' where del had none -> true
    let del2 = "fn do_something();";
    let add2 = "fn do_something() {";
    assert!(super::containers::has_trailing_delimiter_expansion(
        add2, del2, &u
    ));

    // add has '{' inside closed block on same line -> false
    let del3 = "let x = 1;";
    let add3 = "let x = { 1 };";
    assert!(!super::containers::has_trailing_delimiter_expansion(
        add3, del3, &u
    ));

    // add has '{' inside string literal -> false
    let del4 = "let s = \"foo\";";
    let add4 = "let s = \"{\";";
    assert!(!super::containers::has_trailing_delimiter_expansion(
        add4, del4, &u
    ));

    // add opens '(' across multiple lines -> true
    let del5 = "let (a, b) = pair;";
    let add5 = "let (a, b) = call(";
    assert!(super::containers::has_trailing_delimiter_expansion(
        add5, del5, &u
    ));
}
