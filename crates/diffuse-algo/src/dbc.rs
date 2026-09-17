//! Design by Contract (DbC) assertions and invariant enforcement.
//!
//! Provides `require!`, `ensure!`, and `invariant!` macros that validate
//! mathematical and structural invariants during development and testing
//! (`cfg!(debug_assertions)`), while compiling to zero-cost no-ops in optimized
//! release builds.

/// Format a standardized diagnostic message for contract violations.
pub fn format_contract_failure(
    kind: &str,
    condition: &str,
    file: &str,
    line: u32,
    details: Option<&str>,
) -> String {
    if let Some(msg) = details {
        format!(
            "\n[DBC: {kind} FAILED] at {file}:{line}\n  Condition: {condition}\n  Details:   {msg}\n"
        )
    } else {
        format!("\n[DBC: {kind} FAILED] at {file}:{line}\n  Condition: {condition}\n")
    }
}

/// Core failure handler for contract violations.
#[cold]
#[inline(never)]
pub fn fail_contract(
    kind: &str,
    condition: &str,
    file: &str,
    line: u32,
    details: Option<&str>,
) -> ! {
    let msg = format_contract_failure(kind, condition, file, line, details);
    tracing::error!(target: "diffuse_algo::dbc", %kind, %condition, %file, %line, ?details, "Contract failure");
    panic!("{msg}");
}

/// Asserts a function or method precondition $\{P\}$.
///
/// Evaluated in debug and test builds (`cfg!(debug_assertions)`). In release
/// builds, compiles to a zero-cost no-op.
#[macro_export]
macro_rules! require {
    ($cond:expr $(,)?) => {
        if cfg!(debug_assertions) && !$cond {
            $crate::dbc::fail_contract("PRECONDITION", stringify!($cond), file!(), line!(), None);
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        if cfg!(debug_assertions) && !$cond {
            let details = format!($($arg)+);
            $crate::dbc::fail_contract("PRECONDITION", stringify!($cond), file!(), line!(), Some(&details));
        }
    };
}

/// Asserts a function or method postcondition $\{Q\}$.
///
/// Evaluated in debug and test builds (`cfg!(debug_assertions)`). In release
/// builds, compiles to a zero-cost no-op.
#[macro_export]
macro_rules! ensure {
    ($cond:expr $(,)?) => {
        if cfg!(debug_assertions) && !$cond {
            $crate::dbc::fail_contract("POSTCONDITION", stringify!($cond), file!(), line!(), None);
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        if cfg!(debug_assertions) && !$cond {
            let details = format!($($arg)+);
            $crate::dbc::fail_contract("POSTCONDITION", stringify!($cond), file!(), line!(), Some(&details));
        }
    };
}

/// Asserts an internal state, data structure, or loop invariant.
///
/// Evaluated in debug and test builds (`cfg!(debug_assertions)`). In release
/// builds, compiles to a zero-cost no-op.
#[macro_export]
macro_rules! invariant {
    ($cond:expr $(,)?) => {
        if cfg!(debug_assertions) && !$cond {
            $crate::dbc::fail_contract("INVARIANT", stringify!($cond), file!(), line!(), None);
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        if cfg!(debug_assertions) && !$cond {
            let details = format!($($arg)+);
            $crate::dbc::fail_contract("INVARIANT", stringify!($cond), file!(), line!(), Some(&details));
        }
    };
}
