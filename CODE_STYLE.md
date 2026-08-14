# diffuse — Code Style Guidelines

This document establishes the code style, import conventions, formatting rules, and structural patterns for the `diffuse` codebase.

---

## 1. Toolchain & Formatting

### Nightly Formatter & Linter
The project standardizes on nightly tooling for formatting and linting:

- **Pinned stable toolchain**: Defined in [`rust-toolchain.toml`](file:///home/daniel/lab/diffuse/rust-toolchain.toml).
- **Formatter**: `cargo +nightly fmt` with [`rustfmt.toml`](file:///home/daniel/lab/diffuse/rustfmt.toml) (relies on nightly features: `imports_granularity = "Crate"`, `wrap_comments = true`, `format_code_in_doc_comments = true`).
- **Linter**: `cargo +nightly clippy` (with zero warnings permitted).
- **Line Length**: `max_width = 100`.

To format the workspace:
```bash
cargo +nightly fmt
```

To verify formatting:
```bash
cargo +nightly fmt --all -- --check
```

---

## 2. Import Organization & Rules

### Grouping & Ordering
Imports are organized into distinct groups separated by a single blank line, ordered from outer to inner scope:

1. **Standard Library**: `use std::{...};`
2. **Third-Party Dependencies**: `use regex::...;`, `use serde::...;`, `use clap::...;`
3. **Workspace Crates**: (if multi-crate workspace)
4. **Current Crate**: `use crate::{...};`
5. **Parent / Super Modules**: `use super::*;` (primarily in test submodules)

Within each group, `rustfmt` alphabetizes and merges imports from the same crate into a single `use` block (`imports_granularity = "Crate"`).

### No Inline Qualified References
Avoid inline fully-qualified paths such as `crate::parse::Row`, `crate::lcs::*`, or `std::collections::HashMap` in function signatures, struct fields, or function bodies.

- **Do**: Import the item at the top of the file:
  ```rust
  use crate::parse::{AddLine, DelLine, Row, RowKind};
  use std::collections::{HashMap, HashSet};
  ```
- **Don't**: Write inline qualified paths:
  ```rust
  fn process(row: &crate::parse::Row) -> std::collections::HashMap<usize, usize> { ... }
  ```
- **Exception**: Serde attribute string paths that require absolute paths (e.g. `#[serde(skip_serializing_if = "std::ops::Not::not")]`).

### No Wildcard Imports in Production Code
- Avoid `use some_module::*;` in production code. Explicitly name imported types and functions.
- Test submodules (`#[cfg(test)] mod tests`) should use `use super::*;` to bring outer module items into scope.

---

## 3. Data Modeling & Type Safety

### Strongly-Typed Data Over Primitive Flags
- Prefer expressive enums and dedicated structs over boolean flags or loose tuples.
- Example: The core AST node `Row` is a strongly-typed enum:
  ```rust
  pub enum Row {
      Ctx(CtxLine),
      Del(DelLine),
      Add(AddLine),
      Mod { old_line: DelLine, new_line: AddLine },
      Mov { old_line: DelLine, new_line: AddLine },
      Sep(String),
  }
  ```
- Field names for `Mod` and `Mov` are consistently `old_line: DelLine` and `new_line: AddLine`.

### Consistent Pattern Matching Variable Names
Pattern matching variables should mirror the type names they destructure:
- `Row::Ctx(ctx_line)`
- `Row::Del(del_line)`
- `Row::Add(add_line)`
- `Row::Mod { old_line, new_line }`
- `Row::Mov { old_line, new_line }`

### Safe Coordinate & Line Types
- `LineNum = NonZero<u32>` represents 1-based source file line numbers.

### Struct Constructors & Initialization
- Provide `Type::new(...)` for primary constructors and `Type::with_<feature>(...)` for specialized variants (e.g. `AddLine::with_donor(...)`, `AddLine::with_spans(...)`).
- Constructors are not mandatory when direct struct literal initialization is clearer and more idiomatic.

---

## 4. Documentation & Doc Comments

- **Top-Level Module Comments**: Every `.rs` file under `src/` must maintain a top-level module doc comment (`//!`) summarizing its role in the pipeline.
- **Markdown Identifiers**: Wrap all types, variable names, and code tokens in backticks inside doc comments (enforced by `clippy::doc_markdown`).

---

## 5. Verification & Tooling

### All-in-One Check Script
Run [`scripts/check.sh`](file:///home/daniel/lab/diffuse/scripts/check.sh) to format, auto-fix clippy lints, run tests, and verify extension equivalence:
```bash
./scripts/check.sh
```

### Nightly Clippy
All code must pass strict nightly Clippy linter checks with warnings treated as errors:
```bash
cargo +nightly clippy -- -D warnings
```

To auto-apply clippy fixes across the workspace:
```bash
cargo +nightly clippy --fix --allow-dirty --no-deps --all-targets --workspace -- -D warnings
```

### Test Discipline
- **Unit Tests**: Co-located in `tests` submodules within each module (`mod tests { use super::*; ... }`).
- **Acceptance / Invariant Tests**: Run full golden walks and wire serialization verification in `tests/golden.rs`.
- **Equivalence Gate**: Ensure `npm test` passes in `extension/pi/` whenever engine or wire contract behaviors are touched.
