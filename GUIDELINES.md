# diffuse — Architecture & Implementation Guidelines

## Language Syntax & Profile Independence

- **Declaration & Syntax Rules**: Declaration keyword matching, function signature patterns, comment syntax, block delimiters, and syntactic section boundaries must always be defined in the language profile subsystem ([`crates/diffuse-syntax/src/profile.rs`](crates/diffuse-syntax/src/profile.rs) and [`crates/diffuse-syntax/src/language/`](crates/diffuse-syntax/src/language/)).
- **Engine Agnosticism**: The diff engine core (`crates/diffuse-core/`) must never hardcode language-specific keywords, regexes, or declaration rules. All syntactic awareness must be queried through the active [`LangProfile`](crates/diffuse-syntax/src/profile.rs).

## Algorithm & Core Separation

- **Domain Decoupling**: Algorithms in `crates/diffuse-core/src/algo/` operate on generic types and interfaces (`LcsEngine`, `DiffEngine`, `PrefixMatcher`), decoupled from diffuse domain objects.
- **Latency Budgets**: All algorithms and pipeline operations must remain within sub-second budgets on large diffs, utilizing bit-parallel operations, diagonal corridor bounding, and zero-allocation scratch buffers where appropriate.
