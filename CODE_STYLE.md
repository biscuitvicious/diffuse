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
3. **Workspace & Facade Crates**: When consuming diffuse functionality from integration tests, binaries, or external crates, import via the unified facade (`use diffuse::{core, render, syntax, tui};` or `use diffuse::prelude::*;`). Within internal subcrates, upstream workspace crates are accessed via local crate facades (`use crate::core::...;`, `use crate::syntax::...;`, `use crate::render::...;`), keeping implementation files completely free of raw `diffuse_*` crate prefixes.
4. **Current Crate**: `use crate::{...};`
5. **Parent / Super Modules**: `use super::*;` (primarily in test submodules)

Within each group, `rustfmt` alphabetizes and merges imports from the same crate into a single `use` block (`imports_granularity = "Crate"`).

### Top-Level Imports & Inline Qualification

As a general rule of thumb, always aim to import types, traits, and functions at the top of the file rather than writing long inline qualified paths. However, apply this with common sense in favor of idiomatic, clean Rust style:

- **Never directly reference root `std::` inline**: Always import at least one level down from `std` at the top of the file (e.g. `use std::collections::HashMap;`, `use std::{fs, io};`). Never write `std::collections::HashMap` or `std::io::Result` inline in signatures, structs, or function bodies.
- **Parent Module Imports for Disambiguation & Idiomatic Context**: For common modules or generic leaf names, import the parent module at the top level and qualify by the module name inline. This is preferred for:
  - Disambiguating types: `use std::io;` → `io::Result<T>`, `io::Error` (avoids clashing with the standard `Result` or other error types).
  - Module-namespaced functions: `use std::fs;` → `fs::create_dir_all(path)`, `fs::read_to_string(path)` (much clearer than importing bare `read_to_string`).
  - Formatting / serialization traits: `use std::fmt;` → `fmt::Formatter`, `fmt::Result`.
  - Atomics / memory orderings: `use std::sync::atomic::{AtomicU64, Ordering};` → `Ordering::Relaxed`.
- **Direct Leaf Imports**: For distinctive, domain-specific types, structs, and traits, import the leaf item directly:
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
    fn process_file(path: &std::path::Path) -> std::io::Result<std::collections::HashMap<String, crate::parse::Row>> {
        let content = std::fs::read_to_string(path)?;
        ...
    }
    ```
- **Exceptions**: Serde attribute string paths and macro invocations that require absolute paths (e.g. `#[serde(skip_serializing_if = "std::ops::Not::not")]`).

### No Wildcard Imports in Production Code
- Avoid `use some_module::*;` in production code. Explicitly name imported types, modules, and functions.
- Test submodules (`#[cfg(test)] mod tests`) should use `use super::*;` to bring outer module items into scope.

---

## 3. File Component Layout (Top-Down Organization)

We order components of a file, e.g. structs, impls, traits, its impls, etc and so on, strictly in order of [most externally facing/highest level abstraction] at the *top* of the file and continuing along the spectrum all the way down to the [most internally facing/highest level detail] at the very *bottom* of the file.

### Structural Ordering
1. **Module Doc Comment & Imports**: `//!` module summary, followed by grouped `use` declarations.
2. **Public API Types**: Primary public structs, enums, type aliases, and public constants.
3. **Public Trait Definitions**: Primary trait contracts exposed by the module.
4. **Primary Inherent `impl` Blocks**: Constructors (`new`, `with_*`) and high-level public methods.
5. **Trait Implementation Blocks**: Standard trait impls (`Display`, `From`, `Serialize`, etc.) implementing behavior for the primary types.
6. **Internal / Private Types**: Supporting data structures, internal state structs, and private enums.
7. **Internal / Helper Inherent `impl` Blocks & Private Free Functions**: Low-level helper functions and execution algorithms.
8. **Unit Tests**: `#[cfg(test)] mod tests { use super::*; ... }` at the very bottom of the file.

---

## 4. Data Modeling & Type Safety

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
- Field names for `Mod` are consistently `old_line: DelLine` and `new_line: AddLine`.
- Move variants are `Row::MovSrc(del_line)` and `Row::MovTgt(add_line)`.

### Consistent Pattern Matching Variable Names
Pattern matching variables should mirror the type names they destructure:
- `Row::Ctx(ctx_line)`
- `Row::Del(del_line)`
- `Row::Add(add_line)`
- `Row::Mod { old_line, new_line }`
- `Row::MovSrc(del_line)`
- `Row::MovTgt(add_line)`

### Safe Coordinate & Line Types
- `LineNum = NonZero<u32>` represents 1-based source file line numbers.

### Struct Constructors & Initialization
- Provide `Type::new(...)` for primary constructors and `Type::with_<feature>(...)` for specialized variants (e.g. `AddLine::with_donor(...)`, `AddLine::with_spans(...)`).
- Constructors are not mandatory when direct struct literal initialization is clearer and more idiomatic.

---

## 5. Control Flow & Method Structure

### Guard Statements & Early Returns Over Deep Nesting
- Avoid deeply nested `if`s, `match`es, and checks, preferring instead guard statements, `if let ... else`, and early `break` / `continue` / `return` so that the *main flow* of the method flows cleanly down the function / block with minimum indentation / nested blocks.
- If a condition is genuinely two legitimate main flows, keep as nested blocks, but consider the possibility of extracting the blocks to their own significant functions instead.

---

## 6. Documentation & Doc Comments

- **Top-Level Module Comments**: Every `.rs` file under `src/` must maintain a top-level module doc comment (`//!`) summarizing its role in the pipeline.
- **Markdown Identifiers**: Wrap all types, variable names, and code tokens in backticks inside doc comments (enforced by `clippy::doc_markdown`).

---

## 7. Verification & Tooling

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
