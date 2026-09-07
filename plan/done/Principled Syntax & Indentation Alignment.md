# Principled Syntax & Indentation Alignment

- [x] 1. [Phase 1 — Demoting Indentation from Hard Assertion to Soft Hint](#1-phase-1--demoting-indentation-from-hard-assertion-to-soft-hint)
- [x] 2. [Phase 2 — Eliminating Hardcoded Delimiters & Attribute Bypasses](#2-phase-2--eliminating-hardcoded-delimiters--attribute-bypasses)
- [x] 3. [Phase 3 — Replacing Blanket Deletion Suppression with Tunable DP Weights](#3-phase-3--replacing-blanket-deletion-suppression-with-tunable-dp-weights)
- [x] 4. [Phase 4 — Multi-Language Verification & Golden Invariants](#4-phase-4--multi-language-verification--golden-invariants)

---

## Executive Summary

Diff engines must not assume clean or canonical indentation when determining code semantics. In the vast majority of programming languages (C, Rust, Go, JavaScript/TypeScript, Shell, Ruby, Java), indentation is purely stylistic or optional — an unindented shell script or irregularly formatted C function carries the exact same AST and execution semantics as a beautifully indented one. Only languages with syntactically significant indentation (Python, YAML) imbue leading whitespace with block structure meaning.

Currently, several locations in `diffuse-core` make rigid, binary assertions based on indentation:
1. **Direct tail pairing bypass** ([`cluster/dp.rs:1272-1275`](../crates/diffuse-core/src/cluster/dp.rs#L1272-L1275)): unconditionally awards the full 800-point tail bonus when `indent_diff == 0.0` and both lines sit before a closing delimiter, completely bypassing token similarity and delimiter type matching.
2. **Hardcoded indentation in scope detection** ([`scope.rs:36`](../crates/diffuse-core/src/scope.rs#L36)): hardcodes `next.starts_with("    }")`, failing on 2-space, tabbed, or irregularly formatted code.
3. **Hardcoded syntax bypasses and blanket rules**: ad-hoc string comparisons (`"}"`, `#[`, `///`, `[section]`) and global file-wide suppression (`in_new`) that override DP alignment without tunability.

### Core Architectural Principles

1. **Indentation is a Soft Hint, Never a Hard Assertion**: In all languages where `significant_indent == false`, indentation difference is strictly an objective-function cost term in DP (`w.indent * indent_diff`), never a binary gate or automatic match generator.
2. **Syntactic Delimiters Drive Structure**: Block nesting, tails, and boundaries are inferred from balanced delimiters (`{ ... }`, `do ... done`, `if ... fi`) and language grammar tokens provided by [`LangProfile`](../crates/diffuse-syntax/src/profile.rs), not whitespace indentation.
3. **Zero Magic Strings in Core**: Core algorithms (`shift.rs`, `collapse.rs`, `moves.rs`, `dp.rs`) consume language capabilities exclusively through [`LangProfile`](../crates/diffuse-syntax/src/profile.rs).
4. **Soft DP Penalties over Blanket Rejections**: Blanket exclusions (like `in_new` in `guarded_dels`) that drop rows before DP runs are replaced with tunable DP penalties or scope-restricted checks.

---

## Table of Contents

1. [Phase 1 — Demoting Indentation from Hard Assertion to Soft Hint](#1-phase-1--demoting-indentation-from-hard-assertion-to-soft-hint)
2. [Phase 2 — Eliminating Hardcoded Delimiters & Attribute Bypasses](#2-phase-2--eliminating-hardcoded-delimiters--attribute-bypasses)
3. [Phase 3 — Replacing Blanket Deletion Suppression with Tunable DP Weights](#3-phase-3--replacing-blanket-deletion-suppression-with-tunable-dp-weights)
4. [Phase 4 — Multi-Language Verification & Golden Invariants](#4-phase-4--multi-language-verification--golden-invariants)

---

## 1. Phase 1 — Demoting Indentation from Hard Assertion to Soft Hint

### Objective
Eliminate binary bypasses that use indentation to assert semantic equivalence or block structure in non-significant indent languages.

### Steps
1. **Extend `LangProfile` with `significant_indent` & `implicit_return`**:
   - In [`crates/diffuse-syntax/src/profile.rs`](../crates/diffuse-syntax/src/profile.rs):
     - Add `pub significant_indent: bool`. Set to `true` for Python and YAML, `false` for Rust, C, Go, JavaScript, Shell, etc.
     - Add `pub implicit_return: bool`. Set to `true` for Rust, `false` for C, C++, Go, Java, Python, TypeScript, etc.
     - Add `pub fn is_terminal_return_expr(&self, line: &str) -> bool`: distinguishes true return expressions (`None`, `return 17;`) from non-return statements (`None;`, `let x = 1;`).
2. **Scope-Aware Terminal Return Pairing in DP (`cluster/dp.rs`)**:
   - In [`crates/diffuse-core/src/cluster/dp.rs`](../crates/diffuse-core/src/cluster/dp.rs):
     - Replace the blind `indent_diff == 0.0` bypass with **Scope-Aware Terminal Return Affinity**:
       Zero-token-overlap tail pairing is restricted to matching terminal return expressions (`is_terminal_return_expr`) of the enclosing function scope (`is_fn_tail` / `fn_of`), or inferred outermost block tails before a shared closing delimiter.
     - Statements ending with semicolons in Rust (`alpha();`, `None;`) do NOT qualify as return expressions and cannot artificially pair without token similarity.
     - Turn indentation difference into a strong continuous penalty (`w.gamma * (indent_diff)^2` or steep multiplier), so jumping nesting levels into an inner guard clause (`if ... { return None; }`) strongly penalizes candidate matches compared to the outermost function return (`Some(result)`).
3. **Generalize Tail Detection in `scope.rs`**:
   - In [`crates/diffuse-core/src/scope.rs:35-40`](../crates/diffuse-core/src/scope.rs#L35-L40):
     Remove the hardcoded `"    }"` 4-space check.
   - For brace and keyword languages: identify the next line as a closing boundary via `profile.classify_exact_close(next) != CloseDelimiter::None` or `profile.is_block_close(next)`.
   - Distinguish block headers from tails (`!profile.is_block_open(curr)`).
4. **Generalize Scope Boundary Detection in `fn_of` & Surrounding Signals**:
   - In [`crates/diffuse-core/src/scope.rs:74-76`](../crates/diffuse-core/src/scope.rs#L74-L76):
     Replace `l == "}" || raw.starts_with('}')` with `profile.is_block_close(l)`.
   - In piped diffs where `fn_of` has no preceding signature, infer function scope depth from the cluster's outermost closing delimiter in `following_ctx`.

### Verification Gate
- Write targeted unit tests in `scope/tests.rs` and `cluster/tests.rs`:
  - 2-space indented code, tab-indented code, and unindented Shell scripts (`if ... then ... fi`) pairing correctly.
  - Zero-similarity lines before `}` receiving no artificial tail pairing (`alpha();` vs `zzzzz = 42;`).
  - Semicolon-terminated statement (`None;`) NOT qualifying as terminal expression in Rust.
  - Guard clause inversion (`None` -> `Some(result)` before `}`) pairing correctly over inner `if { return None; }`.
- Confirm initial failure, then implement, and verify clean test passes.

---

## 2. Phase 2 — Eliminating Hardcoded Delimiters & Attribute Bypasses

### Objective
Migrate all remaining hardcoded string comparisons and syntax bypasses out of `diffuse-core` into generalized properties on `LangProfile`.

### Steps
1. **Enrich `LangProfile` with Missing Syntax Attributes**:
   - In [`crates/diffuse-syntax/src/profile.rs`](../crates/diffuse-syntax/src/profile.rs):
     - `pub attribute_prefixes: &'static [&'static str]` (e.g., `&["#["]` for Rust, `&["@"]` for Python/JS).
     - `pub continuation_prefixes: &'static [&'static str]` (e.g., `&["&&", "||", "."]` for C/Rust, `&["and ", "or ", "\\"]` for Python).
     - `pub import_prefixes: &'static [&'static str]` (e.g., `&["use "]` for Rust, `&["import "]` for Python/Go).
     - `pub section_headers: bool` (true for INI, TOML, GitConfig, false for standard programming languages).
     - `pub fn is_attribute_line(&self, line: &str) -> bool`.
     - `pub fn is_expression_open(&self, line: &str) -> bool`.
2. **Clean up `profile.rs` Methods**:
   - In [`profile.rs:362-380`](../crates/diffuse-syntax/src/profile.rs#L362-L380):
     Refactor `is_decl_break` to use `self.is_comment_line`, `self.is_attribute_line`, and profile declaration prefixes instead of hardcoded Rust doc comments (`///`, `//!`) and keywords.
   - Refactor `is_continuation_line` to use `self.continuation_prefixes`.
   - Refactor `is_import_statement` to use `self.import_prefixes` without hardcoded semicolon assertions for languages that do not use them.
3. **Clean up `diffuse-core` Call Sites**:
   - In [`shift.rs:324-348`](../crates/diffuse-core/src/shift.rs#L324-L348):
     Replace hardcoded `is_closing_delim` and `is_boundary` with `profile.classify_exact_close` and `profile.is_attribute_line`.
   - In [`collapse.rs:226-235`](../crates/diffuse-core/src/collapse.rs#L226-L235):
     Replace hardcoded `nxt_trim == "};"` and `nxt_trim == "{"` with `profile.is_expression_close` and `profile.is_expression_open`.
   - In [`moves.rs:172-184`](../crates/diffuse-core/src/moves.rs#L172-L184):
     Replace hardcoded trivial delimiters in `is_trivial_line` with `profile.classify_exact_close` and `profile.is_expression_open`.
   - In [`cluster/dp.rs:528-530`](../crates/diffuse-core/src/cluster/dp.rs#L528-L530):
     Guard section header checking (`[section]`) behind `profile.is_some_and(|p| p.section_headers)`.

### Verification Gate
- Unit tests in `shift/tests.rs`, `collapse/tests.rs`, `moves/tests.rs` exercising Python decorators (`@dataclass`), Rust attributes (`#[derive(...)]`), and non-semicolon imports.
- Confirm branch coverage across all modified methods.

---

## 3. Phase 3 — Replacing Blanket Deletion Suppression with Tunable DP Weights

### Objective
Eliminate the blunt whole-file boolean filter `in_new` in `guarded_dels` that unilaterally excludes deleted lines from DP pairing, converting it into a scope-aware or DP-weighted mechanism.

### Steps
1. **Analyze `in_new` Scope Boundaries**:
   - In [`crates/diffuse-core/src/cluster/pairing.rs:128-144`](../crates/diffuse-core/src/cluster/pairing.rs#L128-L144):
     Currently, if a deleted line's exact text exists *anywhere* in the new file, it is completely dropped from pairing consideration in the current hunk.
   - In a refactoring, moving code or modifying similar blocks often results in identical common idioms (e.g., `let x = 1;` or `return Ok(());`) that still should pair within their hunk.
2. **Scope-Restrict or Soft-Weight Suppression**:
   - If the enclosing function or scope is available (`fn_of`), only suppress if the line exists in the *same function/scope* in `new_content`, not unrelated parts of a 10,000-line file.
   - Alternatively, pass a flag `survives_in_new: bool` into [`RowMeta`](../crates/diffuse-core/src/cluster/dp.rs) and add a tunable penalty `w.survives_elsewhere_penalty` in DP scoring rather than a hard boolean exclusion.

### Verification Gate
- Test suite verifying that refactored blocks with common statements within a hunk pair properly without false-positive suppression, while preserving move suppression for genuine relocations.

---

## 4. Phase 4 — Multi-Language Verification & Golden Invariants

### Objective
Verify that the generalized, indentation-agnostic engine handles all supported languages with clean diffs and passes all performance and golden invariants.

### Steps
1. **Test Corpus Expansion**:
   - Add unit tests for unindented Shell code, irregularly indented Rust/C code, 2-space indented JavaScript, and significant-indent Python code.
2. **Performance & Latency Invariant Checks**:
   - Ensure all individual tests execute in `< 0.5s`.
   - Ensure full `./scripts/check.sh` executes in `< 3s` warm.
3. **Golden Acceptance Walk**:
   - Walk any changes in `tests/golden/`.
   - Assert strictly-better-or-equivalent output per [`tests/AGENTS.md`](../tests/AGENTS.md).
   - Review and obtain user approval before finalizing.
