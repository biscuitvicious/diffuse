# diffuse — Architecture & Implementation Guidelines

This document specifies the architectural invariants, engineering principles, and quality standards governing the *diffuse* codebase. All modifications must comply with these guidelines.

---

## 1. Subsystem Boundaries & Decoupling

The codebase is organized into discrete subcrates with strictly unidirectional dependencies:

- **Language Syntax & Profile Independence**: Declaration matching, function signature patterns, comment syntax, block delimiters, and syntactic section boundaries belong exclusively to the language profile subsystem ([`crates/diffuse-syntax/src/profile.rs`](crates/diffuse-syntax/src/profile.rs) and [`crates/diffuse-syntax/src/language/`](crates/diffuse-syntax/src/language/)).
- **Engine Agnosticism**: The diff engine core ([`crates/diffuse-core/`](crates/diffuse-core/)) must never hardcode language-specific keywords, regular expressions, or syntax rules. All syntactic classification is queried dynamically via [`LangProfile`](crates/diffuse-syntax/src/profile.rs).
- **Generic Algorithm Isolation**: Primitives under [`crates/diffuse-core/src/algo/`](crates/diffuse-core/src/algo/) (e.g., Myers O(ND), Hirschberg, Hunt–Szymanski, bit-parallel Myers, affine-gap alignment, bipartite matching) operate over generic sequences and abstractions, decoupled from diff-domain types.

---

## 2. Algorithmic Invariants & Performance Budgets

- **Mathematical Foundations**: Changes in diffing behavior must derive from principled algorithmic formulations, cost matrices, and tunable pipeline parameters. Heuristic special-casing, ad-hoc conditionals, and hardcoded exception branches are strictly prohibited.
- **Latency Budgets**: All algorithms and pipeline operations must execute within sub-second latency budgets on large diffs, utilizing bit-parallel operations, diagonal corridor bounding, and zero-allocation scratch buffers where appropriate.
- **Non-Regression of Architectural Design**: Modifications must preserve or improve the structural coherence and modularity of affected components. Architectural refactoring to prevent entropy is mandatory.

---

## 3. Invariant Enforcement & Fail-Fast Diagnostics

- **Structural Invariants**: Where internal logic guarantees that a condition holds (e.g., resolving a `FlatRow` index into `output.files`, or indexing a hunk coordinate within established bounds), never swallow failure with silent fallback values or empty defaults (`.unwrap_or_default()`).
- **Diagnostic Fail-Fast**: Assert structural invariants using explicit panic macros (`.expect(...)`, `panic!`, or crate DBC macros) enriched with contextual diagnostic parameters. Latent state corruption must be surfaced immediately at the point of origin.
- **Explicit Error Propagation**: Distinguish structural invariants from external inputs, optional user configuration, and empty states. Propagate expected error modes via `Result` or `Option` using `?`.

---

## 4. Test-First Methodology & Exhaustive Branch Coverage

- **Failing Test First**: Every bug fix, functional extension, or behavior adjustment must begin with a targeted automated test (`#[test]`) confirming the initial defect or missing capability prior to code changes.
- **100% Branch Coverage**: All newly introduced code paths must achieve 100% branch coverage. Unit tests must systematically exercise every conditional branch, pattern match arm, and edge case.
- **Performance Budgets**: Individual unit tests must execute in under 0.5s, and the warm cargo test suite must finish in under 1.5s per test binary.

---

## 5. Terminal Styling & Theming Encapsulation

- **Zero Direct ANSI Escapes**: Raw ANSI escape sequences (such as `\x1b[...]` and hardcoded SGR strings) must not appear in application, view, or domain logic.
- **Centralized Palette Authority**: Visual styling is mediated entirely through the theming subsystem ([`crates/diffuse-render/`](crates/diffuse-render/)), using the `yansi` crate as the unified styling abstraction.
- **Renderer Independence**: Core pipeline models emit semantic classifications (e.g., [`FileCounts::segments`](crates/diffuse-core/src/counts.rs)), allowing terminal renderers and external frontends to project colors independently.

## 6. Repository Access & Git Tooling Independence

- **Zero Hard Dependency on Git Binary**: *diffuse* must function correctly across all usages and workflows even when `git` is not installed on the host system.
- **Pure-Rust Repository Access via `gix`**: `gix` must be used for all repository access that would otherwise require `git` to be installed, with no exceptions.
- **Git Binary Permitted as Radically Faster Optimization Only**: Direct execution of the external `git` binary is permitted strictly as an opportunistic optimization if `git` is detected and direct execution would be radically faster. It must never be a hard requirement or single point of failure.

---

## 7. Build, Installation, & Golden Acceptance Invariants

- **Deterministic Builds**: Local binary installations must enforce `--locked` (e.g., `cargo install --locked --path crates/diffuse`) to ensure strict adherence to pinned dependency versions in `Cargo.lock`.
- **Golden Corpus Immutability**: Acceptance corpora ([`tests/golden/`](tests/golden/)) capture frozen diffs and uncompressed snapshots pinned to absolute commit hashes. They must exclude the golden tree itself and remain byte-for-byte reproducible via `GEN_GOLDEN=1`.
- **Dual Acceptance Legs**: Changes to engine semantics or wire formats must pass both the library pipeline leg and the spawned CLI binary leg byte-for-byte (see [`tests/AGENTS.md`](tests/AGENTS.md)).
- **Tool Output Filtering via Negative Exclusion (`grep -v`)**: When post-processing tool, linter, or compiler output in repository scripts, never pipe into positive whitelists (such as `grep -E '^(Fixed|error)'`) which involuntarily swallow unexpected output, panic traces, or warnings. Always use negative blacklists (`grep -v` / `grep -vE`) targeting solely the specific progress strings to suppress (e.g., `grep -vE '^[[:space:]]*(Checking|Compiling|Finished)'`), guaranteeing that any unhandled or unexpected diagnostic output remains visible.

---

## 8. Documentation & Content Creation Standards (GOV.UK Standard)

All project documentation, architectural guidelines, docstrings, user manuals, and written communications must adhere to the principles specified in the **[GOV.UK Writing Guidelines](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/writing-guidelines/)**. This standard serves as the primary benchmark for clarity, economy of language, and informational utility.

### Executive Summary of Key Guidelines

- **Front-Loading & Inverted Pyramid**: Place the most critical information, conclusion, or action at the very top. Structure content in descending order of importance so that readers who skim or scan absorb the essential points immediately.
- **Clear Structure & Scannability**: Readers scan in an F-shaped pattern, typically consuming only 20% to 28% of words on a page. Keep paragraphs short (1–3 sentences), group related concepts under front-loaded descriptive headings, and use bulleted lists for multi-item sequences.
- **Plain, Direct Language**: Use plain English and active voice. Favor simple, concrete terms over pretentious synonyms (e.g., write "use" rather than "utilize"). Eliminate jargon, idioms, buzzwords, and acronyms without expansion. Aim for sentences averaging under 25 words.
- **Tone of Voice**:
  - **Specific, informative, clear, and concise**: Every phrase must convey substance without padding.
  - **Brisk, but not terse**: Move swiftly through explanation without omitting necessary mechanics.
  - **Incisive and precise**: Avoid excessive conversational preamble or conversational filler that obscures technical accuracy.
  - **Serious, not pompous**: Maintain technical authority without administrative stiffness or self-importance.
  - **Objective and emotionless**: Avoid promotional hype, marketing rhetoric, and subjective adjectives. Document capabilities factually.
- **Actionable Titles & Clear Summaries**: Every document, major section, and change note must open with a concise summary stating the exact problem, solution, and outcome upfront. Headings and links must be descriptive, front-loaded, and action-oriented.
- **Markdown Documentation File Naming**: Use plain Title-Caps filenames for Markdown documentation in subdirectories (e.g., `docs/Algorithms.md`, `plan/Plan Guidelines.md`). Exceptions apply only to Markdown files located directly in the project root (e.g., `README.md`, `CODE_STYLE.md`, `GUIDELINES.md`, `LICENSE.md`) or files that adhere to all-caps per idiomatic conventions (e.g., `tests/AGENTS.md`, `extension/pi/AGENTS.md`).

---

## 9. Method & Type Naming Standards

- **Method Naming**: Describe clearly, succinctly, and unambiguously either what behaviour the method fulfills, or what the return value of the method is. Describe the most abstract concept the method represents, and avoid naming implementation details. Follow Rust language best practices for idiomatic method naming (`snake_case`, standard conversion and query prefixes like `as_`, `to_`, `into_`, `is_`, `has_`, and front-loaded active verbs for mutating operations).
- **Type Naming**: Name types (structs, enums, traits, and aliases) clearly, succinctly, and unambiguously based on the core domain concept, entity, or role they represent. Describe the most abstract concept and semantic responsibility, and avoid naming algorithm-specific implementation details or underlying computational machinery (for example, prefer `DiffOp` or `MatchOp` over `DpOp`). Follow Rust language best practices for idiomatic type naming (`UpperCamelCase`, noun phrases for types, capability adjectives or verbs for traits).
