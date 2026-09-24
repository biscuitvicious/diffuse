# diffuse — Architecture and implementation guidelines

This document specifies the architectural invariants, engineering principles, and quality standards that govern the *diffuse* codebase. All modifications must comply with these guidelines.

---

## 1. Subsystem boundaries and decoupling

The codebase is organized into discrete subcrates with strictly unidirectional dependencies:

- **Language syntax and profile independence**: Declaration matching, function signature patterns, comment syntax, block delimiters, and syntactic section boundaries belong exclusively to the language profile subsystem ([`crates/diffuse-syntax/src/profile.rs`](crates/diffuse-syntax/src/profile.rs) and [`crates/diffuse-syntax/src/language/`](crates/diffuse-syntax/src/language/)).
- **Engine agnosticism**: The diff engine core ([`crates/diffuse-core/`](crates/diffuse-core/)) must never hardcode language-specific keywords, regular expressions, or syntax rules. Query all syntactic classification dynamically via [`LangProfile`](crates/diffuse-syntax/src/profile.rs).
- **Generic algorithm isolation**: Primitives under [`crates/diffuse-core/src/algo/`](crates/diffuse-core/src/algo/) (for example, Myers O(ND), Hirschberg, Hunt–Szymanski, bit-parallel Myers, affine-gap alignment, bipartite matching) operate over generic sequences and abstractions, decoupled from diff-domain types.

---

## 2. Algorithmic invariants and performance budgets

- **Mathematical foundations**: Derive all changes in diffing behavior from principled algorithmic formulations, cost matrices, and tunable pipeline parameters. Heuristic special-casing, ad-hoc conditionals, and hardcoded exception branches are strictly prohibited.
- **Latency budgets**: All algorithms and pipeline operations must execute within sub-second latency budgets on large diffs. Use bit-parallel operations, diagonal corridor bounding, and zero-allocation scratch buffers where appropriate.
- **Non-regression of architectural design**: Modifications must preserve or improve the structural coherence and modularity of affected components. Architectural refactoring to prevent entropy is mandatory.

---

## 3. Invariant enforcement and fail-fast diagnostics

- **Structural invariants**: Never swallow failure with silent fallback values or empty defaults (`.unwrap_or_default()`) where internal logic guarantees that a condition holds. Examples include resolving a `FlatRow` index into `output.files`, or indexing a hunk coordinate within established bounds.
- **Diagnostic fail-fast**: Assert structural invariants using explicit panic macros (`.expect(...)`, `panic!`, or crate DBC macros) enriched with contextual diagnostic parameters. Surface latent state corruption immediately at the point of origin.
- **Explicit error propagation**: Distinguish structural invariants from external inputs, optional user configuration, and empty states. Propagate expected error modes via `Result` or `Option` using `?`.

---

## 4. Test-first methodology and exhaustive branch coverage

- **Failing test first**: Every bug fix, functional extension, or behavior adjustment must begin with a targeted automated test (`#[test]`) confirming the initial defect or missing capability before any code changes.
- **100% branch coverage**: All newly introduced code paths must achieve 100% branch coverage. Unit tests must systematically exercise every conditional branch, pattern match arm, and edge case.
- **Performance budgets**: Individual unit tests must execute in under 0.5s, and the warm cargo test suite must finish in under 1.5s per test binary.

---

## 5. Terminal styling and theming encapsulation

- **Zero direct ANSI escapes**: Raw ANSI escape sequences (such as `\x1b[...]` and hardcoded SGR strings) must not appear in application, view, or domain logic.
- **Centralized palette authority**: Visual styling is mediated entirely through the theming subsystem ([`crates/diffuse-render/`](crates/diffuse-render/)), using the `yansi` crate as the unified styling abstraction.
- **Renderer independence**: Core pipeline models emit semantic classifications (for example, [`FileCounts::segments`](crates/diffuse-core/src/counts.rs)), allowing terminal renderers and external frontends to project colors independently.

## 6. Repository access and git tooling independence

- **Zero hard dependency on the git binary**: *diffuse* must function correctly across all usages and workflows even when `git` is not installed on the host system.
- **Pure-Rust repository access via `gix`**: Use `gix` for all repository access that would otherwise require `git` to be installed, with no exceptions.
- **Git binary permitted as a radically faster optimization only**: Direct execution of the external `git` binary is permitted strictly as an opportunistic optimization when `git` is detected and direct execution would be radically faster. It must never be a hard requirement or a single point of failure.

---

## 7. Build, installation, and golden acceptance invariants

- **Deterministic builds**: Local binary installations must enforce `--locked` (for example, `cargo install --locked --path crates/diffuse`) to ensure strict adherence to pinned dependency versions in `Cargo.lock`.
- **Golden corpus immutability**: Acceptance corpora ([`tests/golden/`](tests/golden/)) capture frozen diffs and uncompressed snapshots pinned to absolute commit hashes. They must exclude the golden tree itself and remain byte-for-byte reproducible via `GEN_GOLDEN=1`.
- **Dual acceptance legs**: Changes to engine semantics or wire formats must pass both the library pipeline leg and the spawned CLI binary leg byte-for-byte (see [`tests/AGENTS.md`](tests/AGENTS.md)).
- **Tool output filtering via negative exclusion (`grep -v`)**: When post-processing tool, linter, or compiler output in repository scripts, never pipe into positive whitelists (such as `grep -E '^(Fixed|error)'`), which involuntarily swallow unexpected output, panic traces, or warnings. Always use negative blacklists (`grep -v` / `grep -vE`) targeting solely the specific progress strings to suppress (for example, `grep -vE '^[[:space:]]*(Checking|Compiling|Finished)'`), so that any unhandled or unexpected diagnostic output remains visible.

---

## 8. Documentation and content creation standards (GOV.UK standard)

All project documentation, architectural guidelines, docstrings, user manuals, and written communications must follow the guidelines below. They are adapted from the **[GOV.UK Writing Guidelines](https://guidance.publishing.service.gov.uk/writing-to-gov-uk-standards/writing-guidelines/)**, which remain the original reference.

- **Front-load content and use the inverted pyramid**: Place the most critical information, conclusion, or action at the top. Structure content in descending order of importance, so readers who scan absorb the essential points immediately.
- **Keep content scannable**: Readers scan in an F-shaped pattern, typically reading only 20% to 28% of the words on a page. Keep paragraphs to 1–3 sentences, group related concepts under front-loaded descriptive headings, and use bulleted lists for multi-item sequences.
- **Use plain, direct language**: Use plain English and active voice. Prefer simple, concrete terms over pretentious synonyms ("use", not "utilize"). Eliminate jargon, idioms, and unexpanded acronyms. Keep sentences under 25 words on average.
- **Keep a factual tone**: Be specific, informative, clear, and concise. Be brisk but not terse, incisive and precise, serious but not pompous, and objective. Avoid promotional hype, marketing rhetoric, padding, and subjective adjectives.
- **Open with actionable titles and clear summaries**: Every document, major section, and change note opens with a concise summary stating the problem, solution, and outcome. Make headings and links descriptive, front-loaded, and action-oriented.
- **Name Markdown documentation files in Title Caps**: Use Title-Caps filenames in subdirectories (for example, `docs/Algorithms.md`, `plan/Plan Guidelines.md`). Exceptions: Markdown files in the project root (`README.md`, `CODE_STYLE.md`, `GUIDELINES.md`), and all-caps idiomatic names (`LICENSE`, `tests/AGENTS.md`, `extension/pi/AGENTS.md`).
- **Use American spelling**: Use American spelling throughout ("color", "behavior", "serialize"). This is a deliberate exception to the GOV.UK default of British spelling: the codebase, Rust APIs, and tooling use American spelling. Follow the semantic meaning of the GOV.UK guidelines rather than the literal British spelling rule.

---

## 9. Method and type naming standards

- **Method naming**: Describe clearly, succinctly, and unambiguously either what behavior the method fulfills or what value it returns. Describe the most abstract concept the method represents, and avoid naming implementation details. Follow Rust language best practices for idiomatic method naming (`snake_case`, standard conversion and query prefixes like `as_`, `to_`, `into_`, `is_`, `has_`, and front-loaded active verbs for mutating operations).
- **Type naming**: Name types (structs, enums, traits, and aliases) clearly, succinctly, and unambiguously based on the core domain concept, entity, or role they represent. Describe the most abstract concept and the semantic responsibility, and avoid naming algorithm-specific implementation details or underlying computational machinery (for example, prefer `DiffOp` or `MatchOp` over `DpOp`). Follow Rust language best practices for idiomatic type naming (`UpperCamelCase`, noun phrases for types, capability adjectives or verbs for traits).
