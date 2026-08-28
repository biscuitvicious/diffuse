use super::*;

#[test]
fn test_require_passing() {
    let x = 10;
    require!(x > 0);
    require!(x == 10, "x should be 10, got {x}");
}

#[test]
fn test_ensure_passing() {
    let mut vec = vec![1, 2, 3];
    vec.push(4);
    ensure!(vec.len() == 4);
    ensure!(!vec.is_empty(), "vector must not be empty");
}

#[test]
fn test_invariant_passing() {
    let count = 5;
    invariant!(count > 0);
    invariant!(count < 10, "count invariant satisfied");
}

#[test]
#[should_panic(expected = "[DBC: PRECONDITION FAILED]")]
fn test_require_failing_panics() {
    let x = 0;
    require!(x > 0, "x must be strictly positive");
}

#[test]
#[should_panic(expected = "[DBC: POSTCONDITION FAILED]")]
fn test_ensure_failing_panics() {
    let vec: Vec<i32> = Vec::new();
    ensure!(!vec.is_empty(), "must not return empty");
}

#[test]
#[should_panic(expected = "[DBC: INVARIANT FAILED]")]
fn test_invariant_failing_panics() {
    let balance = -5;
    invariant!(balance >= 0, "balance must stay non-negative");
}

#[test]
#[should_panic(expected = "[DBC: PRECONDITION FAILED]")]
fn test_require_failing_no_details() {
    let x = 0;
    require!(x > 0);
}

#[test]
#[should_panic(expected = "[DBC: POSTCONDITION FAILED]")]
fn test_ensure_failing_no_details() {
    let x = 0;
    ensure!(x > 0);
}

#[test]
#[should_panic(expected = "[DBC: INVARIANT FAILED]")]
fn test_invariant_failing_no_details() {
    let x = 0;
    invariant!(x > 0);
}

#[test]
fn test_format_contract_message() {
    let formatted = format_contract_failure(
        "PRECONDITION",
        "divisor != 0",
        "crates/diffuse-core/src/math.rs",
        42,
        Some("divisor cannot be zero"),
    );

    assert!(formatted.contains("[DBC: PRECONDITION FAILED] at crates/diffuse-core/src/math.rs:42"));
    assert!(formatted.contains("Condition: divisor != 0"));
    assert!(formatted.contains("Details:   divisor cannot be zero"));

    let formatted_no_details = format_contract_failure(
        "POSTCONDITION",
        "count > 0",
        "crates/diffuse-core/src/lib.rs",
        10,
        None,
    );
    assert!(
        formatted_no_details
            .contains("[DBC: POSTCONDITION FAILED] at crates/diffuse-core/src/lib.rs:10")
    );
    assert!(formatted_no_details.contains("Condition: count > 0"));
    assert!(!formatted_no_details.contains("Details:"));
}
