# Language-Agnostic Engine Generalization

- [ ] 1. [Phase 1 — Extended Language Specification (`LangProfile`)](#1-phase-1--extended-language-specification-langprofile)
- [ ] 2. [Phase 2 — Generic Scope Extraction & Boundary Detection (`src/scope/`)](#2-phase-2--generic-scope-extraction--boundary-detection-srcscope)
- [ ] 3. [Phase 3 — Generic Cluster Continuation & Delimiter Matching (`src/collapse/` & `src/cluster/`)](#3-phase-3--generic-cluster-continuation--delimiter-matching-srccollapse--srccluster)
- [ ] 4. [Phase 4 — Universal Verification & Multi-Language Test Corpus](#4-phase-4--universal-verification--multi-language-test-corpus)

---

## Executive Summary

The diffuse diff engine is designed to be renderer-agnostic and language-agnostic. However, historical engine heuristics accumulated language-specific literals (Rust-specific keyword arrays `KEYWORDS`, hardcoded `pub fn` / `struct` / `enum` regexes, and brace assumption `\s{4}\}`).

This plan establishes a canonical architecture where **all language-specific tokens, signatures, declaration prefixes, and block boundary styles live exclusively in [`LangProfile`](../src/syntax/mod.rs)**. The core engine algorithms (DP alignment, scope extraction, block tail proximity, and multi-line expansion continuation) consume these specifications generically without any hardcoded language bias.

---

## Table of Contents

1. [Phase 1 — Extended Language Specification (`LangProfile`)](#1-phase-1--extended-language-specification-langprofile)
2. [Phase 2 — Generic Scope Extraction & Boundary Detection (`src/scope/`)](#2-phase-2--generic-scope-extraction--boundary-detection-srcscope)
3. [Phase 3 — Generic Cluster Continuation & Delimiter Matching (`src/collapse/` & `src/cluster/`)](#3-phase-3--generic-cluster-continuation--delimiter-matching-srccollapse--srccluster)
4. [Phase 4 — Universal Verification & Multi-Language Test Corpus](#4-phase-4--universal-verification--multi-language-test-corpus)

---

## 1. Phase 1 — Extended Language Specification (`LangProfile`)

### Objective
Move all language-specific lexical properties, declaration patterns, and block delimiter semantics into `LangProfile` in [`src/syntax/mod.rs`](../src/syntax/mod.rs).

### Steps
1. **Extend `LangProfile` Definition**:
   ```rust
   pub struct LangProfile {
       pub keywords: HashSet<String>,
       pub hash_comment: bool,
       pub line_comment: bool,
       pub block_comment: bool,
       pub types_by_case: bool,
       pub fence: Option<Regex>,
       pub unlabeled_fence_lang: Option<&'static str>,
       pub fence_strip: Option<Regex>,
       pub markdown: bool,

       // --- Generalized Language Declarations & Delimiters ---
       pub block_style: BlockDelimStyle,
       pub fn_decl: Option<Regex>,
       pub decl_prefixes: &'static [&'static str],
   }
   ```
2. **Define `BlockDelimStyle`**:
   - `Braces`: C/C++, Rust, TypeScript, JavaScript, Go, Java (`{` and `}`).
   - `IndentColon`: Python, YAML (`:` and dedent / unindent).
   - `Keywords`: Shell, Bash (`then`/`do` and `fi`/`done`/`esac`).
3. **Populate Language Profiles**:
   - `py`: `fn_decl = ^\s*(?:async\s+)?def\s+([A-Za-z0-9_]+)`, `decl_prefixes = &["def ", "class ", "async def "]`.
   - `ts`/`js`: `fn_decl = ^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z0-9_]+)`, `decl_prefixes = &["function ", "class ", "interface ", "type ", "enum ", "const ", "let ", "export "]`.
   - `rs`: `fn_decl = ^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)`, `decl_prefixes = &["fn ", "pub fn ", "pub ", "struct ", "enum ", "impl ", "trait ", "type "]`.
   - `go`: `fn_decl = ^\s*func\s+(?:\([^)]*\)\s+)?([A-Za-z0-9_]+)`, `decl_prefixes = &["func ", "type ", "var ", "const "]`.
   - `c`/`cpp`: `fn_decl = ^\s*(?:[A-Za-z0-9_<>:*&]+\s+)+([A-Za-z0-9_]+)\s*\([^;]*\)\s*\{?`, `decl_prefixes = &["class ", "struct ", "enum ", "typedef ", "template "]`.
   - `sh`: `fn_decl = ^\s*(?:function\s+)?([A-Za-z0-9_]+)\s*\(\)`, `decl_prefixes = &["function "]`.

---

## 2. Phase 2 — Generic Scope Extraction & Boundary Detection (`src/scope/`)

### Objective
Eliminate hardcoded Rust keywords and syntax patterns from [`src/scope/mod.rs`](../src/scope/mod.rs).

### Steps
1. **Generic `fn_of`**:
   - Update `fn_of(content, line, profile: Option<&LangProfile>) -> Option<String>`.
   - Match function headers using `profile.and_then(|p| p.fn_decl.as_ref())`.
   - Stop search at declaration boundaries matching `profile.decl_prefixes` or `profile.is_block_close(raw)`.
2. **Generic `is_tail_before_closing`**:
   - Replace the hardcoded `Regex::new(r"^\s{4}\}\s*$")` with generic indentation-aware and profile-aware boundary detection:
     - For brace languages: matches `profile.is_block_close(&content[line])`.
     - For indentation languages: matches when `indent_of(line) > indent_of(next_line)`.

---

## 3. Phase 3 — Generic Cluster Continuation & Delimiter Matching (`src/collapse/` & `src/cluster/`)

### Objective
Remove hardcoded tokens from multi-line expansion loops, statement head checks, and cluster DP solvers.

### Steps
1. **Remove Hardcoded Keyword Lists in [`src/collapse/mod.rs`](../src/collapse/mod.rs)**:
   - Replace `const KEYWORDS = &["if", ...]` in `statement_head_keyword` with universal leading word token extraction (`c.is_alphanumeric() || c == '_'`) or checking `profile.keywords.contains(word)`.
   - Replace `fn_exists` with generic function extraction via `profile.fn_decl`.
2. **Profile-Aware Expansion Continuation**:
   - Replace hardcoded `nxt_t.starts_with("#[") || nxt_t.starts_with("pub ")` with `is_comment_line` and `profile.decl_prefixes`.
   - Generalize multi-line continuation for compound block expressions across all supported languages.
3. **Cluster DP Boundary Solvers in [`src/cluster/dp.rs`](../src/cluster/dp.rs)**:
   - Calculate `open_dist` and `close_dist` using `profile.is_block_open` and `profile.is_block_close` alongside universal indentation drop (dedent) detection.

---

## 4. Phase 4 — Universal Verification & Multi-Language Test Corpus

### Objective
Ensure test suite coverage across Python, TypeScript, Rust, Shell, Go, and C.

### Steps
1. **Multi-Language Scope Tests in [`src/scope/tests.rs`](../src/scope/tests.rs)**:
   - Test `fn_of` across Python (`def foo():`), TypeScript (`export function foo()`), Go (`func (r *T) foo()`), and C (`int foo()`).
2. **Multi-Language Block Tail Tests in [`src/collapse/tests.rs`](../src/collapse/tests.rs)**:
   - Test block close tail proximity (`None` $\leftrightarrow$ `Some(result)`) across Rust, Python (dedent), and Shell (`fi`).
3. **Golden Suite & Equivalence Walk**:
   - Run `./scripts/check.sh` and ensure 100% clean passes across unit tests, golden corpora, and the extension.
