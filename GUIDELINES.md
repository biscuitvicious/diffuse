# diffuse — Architecture & Implementation Guidelines

## Language Syntax & Profile Independence

- **Declaration & Syntax Rules**: Declaration keyword matching, function signature patterns, comment syntax, block delimiters, and syntactic section boundaries must always be defined in the language profile subsystem ([`crates/diffuse-syntax/src/profile.rs`](crates/diffuse-syntax/src/profile.rs) and [`crates/diffuse-syntax/src/language/`](crates/diffuse-syntax/src/language/)).
- **Engine Agnosticism**: The diff engine core (`crates/diffuse-core/`) must never hardcode language-specific keywords, regexes, or declaration rules. All syntactic awareness must be queried through the active [`LangProfile`](crates/diffuse-syntax/src/profile.rs).

## Algorithm & Core Separation

- **Domain Decoupling**: Algorithms in `crates/diffuse-core/src/algo/` operate on generic types and interfaces (`LcsEngine`, `DiffEngine`, `PrefixMatcher`), decoupled from diffuse domain objects.
- **Latency Budgets**: All algorithms and pipeline operations must remain within sub-second budgets on large diffs, utilizing bit-parallel operations, diagonal corridor bounding, and zero-allocation scratch buffers where appropriate.

## Fail-Fast Error Handling & Invariant Enforcement

- **Enforce Structural Invariants**: At any point where the normal, correct functioning of the codebase guarantees that an indexing, map lookup, or state query cannot fail (e.g., resolving a `FlatRow` into `output.files`, accessing a hunk by a valid `HunkStart` or selection coordinate), never silently swallow lookups with `.unwrap_or(...)`, empty fallbacks, or silent `None` returns that allow corrupt or desynchronized state to propagate.
- **Fail Fast with Diagnostics**: Use `.expect("...")` / panics with diagnostic context (such as index vs. slice bounds) on broken invariants so latent bugs and data structure mismatches are caught immediately at the origin.
- **Explicit Early Return on Preconditions**: When handling functions that legitimately return `Option` or `Result`, propagate precondition failures or unexpected states early and explicitly with `?` rather than continuing execution with invalid default values.
- **Judicious Application**: Distinguish structural invariants (which should fail fast) from external inputs, optional user state, or legitimate missing elements (e.g., empty repos or non-existent notes), which should remain handled gracefully.

## Build & Installation Conventions

- **`cargo install --locked`**: Always use `--locked` when installing binaries locally (e.g., `cargo install --locked --path crates/diffuse-cli --force`) to ensure deterministic builds strictly bound to the pinned versions in `Cargo.lock`.
