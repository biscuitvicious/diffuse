# diffuse — Code style guidelines

This document specifies the code style, import conventions, formatting rules, and structural patterns for the `diffuse` codebase.

---

## 1. Toolchain and formatting

### Nightly formatter and linter

Formatting and linting use nightly tooling:

- **Pinned stable toolchain**: Defined in [`rust-toolchain.toml`](rust-toolchain.toml).
- **Formatter**: `cargo +nightly fmt` with [`rustfmt.toml`](rustfmt.toml). Relies on the nightly features `imports_granularity = "Crate"`, `wrap_comments = true`, and `format_code_in_doc_comments = true`.
- **Linter**: `cargo clippy`, with zero warnings permitted.
- **Line length**: `max_width = 100`.

To format the workspace:

```bash
cargo +nightly fmt
```

To verify formatting:

```bash
cargo +nightly fmt --all -- --check
```

---

## 2. Import organization and rules

### Grouping and ordering

Imports are organized into distinct groups, separated by a single blank line, ordered from outer to inner scope:

1. **Standard library**: `use std::{...};`
2. **Third-party dependencies**: `use regex::...;`, `use serde::...;`, `use clap::...;`
3. **Workspace and facade crates**: When consuming diffuse functionality from integration tests, binaries, or external crates, import via the unified facade (`use diffuse::{core, render, syntax, tui};` or `use diffuse::prelude::*;`). Within internal subcrates, access upstream workspace crates via local crate facades (`use crate::core::...;`, `use crate::syntax::...;`, `use crate::render::...;`), keeping implementation files completely free of raw `diffuse_*` crate prefixes.
4. **Current crate**: `use crate::{...};`
5. **Parent / super modules**: `use super::*;` (primarily in test submodules)

Within each group, `rustfmt` alphabetizes and merges imports from the same crate into a single `use` block (`imports_granularity = "Crate"`).

### Top-level imports and inline qualification

Import types, traits, and functions at the top of the file rather than writing long inline qualified paths. Apply this with common sense in favor of idiomatic, clean Rust style:

- **Never reference the root `std::` inline**: Always import at least one level down from `std` at the top of the file (for example, `use std::collections::HashMap;` or `use std::{fs, io};`). Never write `std::collections::HashMap` or `std::io::Result` inline in signatures, structs, or function bodies.
- **Import parent modules for disambiguation and idiomatic context**: For common modules or generic leaf names, import the parent module at the top level and qualify by the module name inline. Prefer this for:
  - Disambiguating types: `use std::io;` → `io::Result<T>`, `io::Error` (avoids clashing with the standard `Result` or other error types).
  - Module-namespaced functions and constants: `use std::fs;` → `fs::create_dir_all(path)`, `fs::read_to_string(path)`; `use std::env;` → `env::consts::OS`, `env::var(...)`.
  - Formatting / serialization traits: `use std::fmt;` → `fmt::Formatter`, `fmt::Result`.
  - Atomics / memory orderings: `use std::sync::atomic::{AtomicU64, Ordering};` → `Ordering::Relaxed`.
- **Import distinctive leaves directly**: For distinctive, domain-specific types, structs, and traits, import the leaf item directly:
  - **Do**:
    ```rust
    use crate::parse::{AddLine, DelLine, Row, RowKind};
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::{fs, io};

    fn process_file(path: &Path) -> io::Result<HashMap<String, Row>> {
        let content = fs::read_to_string(path)?;
        ...
    }
    ```
  - **Don't**:
    ```rust
    // Root std:: and full inline paths in signatures/bodies
    fn process_file(path: &std::path::Path) -> std::io::Result<HashMap<String, crate::parse::Row>> {
        let content = std::fs::read_to_string(path)?;
        ...
    }
    ```
- **Exceptions**: Serde attribute string paths and macro invocations that require absolute paths (for example, `#[serde(skip_serializing_if = "std::ops::Not::not")]`).

### No wildcard imports in production code

Avoid `use some_module::*;` in production code. Explicitly name imported types, modules, and functions.

Test submodules (`#[cfg(test)] mod tests`) should use `use super::*;` to bring outer module items into scope.

---

## 3. File component layout (top-down organization)

Structure components within a source file in strict top-down order of abstraction. The most externally facing contracts (public types and trait definitions) appear at the top. Internal implementation details and private helpers follow, with unit tests at the bottom.

### Structural ordering

1. **Module doc comment and imports**: `//!` module summary, followed by grouped `use` declarations.
2. **Public API types**: Primary public structs, enums, type aliases, and public constants.
3. **Public trait definitions**: Primary trait contracts exposed by the module.
4. **Primary inherent `impl` blocks**: Constructors (`new`, `with_*`) and high-level public methods.
5. **Trait implementation blocks**: Standard trait impls (`Display`, `From`, `Serialize`, etc.) implementing behavior for the primary types.
6. **Internal / private types**: Supporting data structures, internal state structs, and private enums.
7. **Internal / helper inherent `impl` blocks and private free functions**: Low-level helper functions and execution algorithms.
8. **Unit tests**: `#[cfg(test)] mod tests { use super::*; ... }` at the very bottom of the file.

---

## 4. Data modeling and type safety

### Strongly typed data over primitive flags

Prefer expressive enums and dedicated structs over boolean flags or loose tuples.

The core AST node `Row` is a strongly typed enum:

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

- Field names for `Mod` are consistently `old_line: DelLine` and `new_line: AddLine`.
- Move variants are `Row::MovSrc(del_line)` and `Row::MovTgt(add_line)`.

### Consistent pattern matching variable names

Pattern matching variables should mirror the type names they destructure:

- `Row::Ctx(ctx_line)`
- `Row::Del(del_line)`
- `Row::Add(add_line)`
- `Row::Mod { old_line, new_line }`
- `Row::MovSrc(del_line)`
- `Row::MovTgt(add_line)`

### Safe coordinate and line types

`LineNum = NonZero<u32>` represents 1-based source file line numbers.

### Struct constructors and initialization

- Provide `Type::new(...)` for primary constructors and `Type::with_<feature>(...)` for specialized variants (for example, `AddLine::with_donor(...)`, `AddLine::with_spans(...)`).
- Constructors are not mandatory when direct struct literal initialization is clearer and more idiomatic.

---

## 5. Control flow and method structure

### Guard statements and early returns over deep nesting

Avoid deeply nested `if`s, `match`es, and checks. Prefer guard statements, `if let ... else`, and early `break` / `continue` / `return`, so that the main flow of the method runs cleanly down the function or block with minimal indentation.

If a condition represents two legitimate main flows, keep it as nested blocks. Also consider extracting the blocks into their own significant functions.

---

## 6. Documentation and doc comments

- **Top-level module comments**: Every `.rs` file under `src/` must maintain a top-level module doc comment (`//!`) summarizing its role in the pipeline.
- **Markdown identifiers**: Wrap all types, variable names, and code tokens in backticks inside doc comments (enforced by `clippy::doc_markdown`).

---

## 7. Verification and tooling

### All-in-one check script

Run [`scripts/check.sh`](scripts/check.sh) to format, auto-fix clippy lints, run tests, and verify extension equivalence:

```bash
scripts/check.sh
```

### Clippy

All code must pass strict Clippy checks, with warnings treated as errors, using the pinned toolchain:

```bash
cargo clippy -- -D warnings
```

To auto-apply clippy fixes across the workspace:

```bash
cargo clippy --fix --allow-dirty --no-deps --all-targets --workspace -- -D warnings
```

### Test discipline

- **Unit tests**: Co-located in `tests` submodules within each module (`mod tests { use super::*; ... }`).
- **Acceptance / invariant tests**: Run full golden walks and wire serialization verification in [`crates/diffuse/tests/golden.rs`](crates/diffuse/tests/golden.rs).
- **Equivalence gate**: Ensure `npm test` passes in `extension/pi/` whenever engine or wire contract behaviors are touched.
