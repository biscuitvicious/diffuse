# Unicode Word & Alphanumeric Token Generalization

- [·] **Phase 1: Foundational Unicode Character Class Primitives & Word Boundaries (`diffuse-syntax`)**
- [ ] **Phase 2: Semantic Token Classification & Entropy Invariance (`diffuse-core`)**
- [ ] **Phase 3: Cluster Dynamic Programming & Statement Head Generalization (`diffuse-core`)**
- [ ] **Phase 4: Sub-Line Token Alignment & CamelCase Unicode Boundaries (`diffuse-core`)**
- [ ] **Phase 5: Golden Corpus Verification & Full Pipeline Sign-Off**

---

## Executive Summary

*diffuse* reconstructs granular line-level correspondence and sub-line modifications across unified diffs. Historically, tokenization, semantic classification, syntax coloring, and statement-head pairing relied on ASCII-only predicates (`is_ascii_alphanumeric`, `is_ascii_alphabetic`, `first_ascii_word`, and raw byte indexing `b[0]`). While effective for standard English ASCII identifiers, this introduces severe degradation when analyzing codebases containing non-ASCII content:

1. **Token Fragmentation**: Non-ASCII identifiers (such as French `résumé_count`, German `größte_zahl`, Greek `μέγιστο`, Cyrillic `пользователь`, or CJK `ユーザー名`) fail the ASCII identifier scanner and fragment into single-character `RE_OTHER` tokens, destroying phrase landmarks and alignment continuity.
2. **Semantic Misclassification**: Unicode identifiers fall through syntax classification into `TokenColor::Operator` or `TokenClass::ProseWord`, distorting information-theoretic entropy scores ($I(t)$) in the Gotoh DP alignment.
3. **Statement-Head Pairing Blindness**: The clustering DP relies on `first_ascii_word` to detect statement heads (`let`, `fn`, `const`, or comment subjects). Lines opening with non-ASCII terms receive `first_ascii_word = None`, missing syntactic alignment bonuses.

This engineering plan generalizes all word-boundary and alphanumeric matching to the full Unicode character set (conforming to Unicode Standard Annex #31: Identifier and Pattern Syntax and Unicode Standard Annex #29: Text Segmentation). The solution maintains a dual-mode execution model: pure ASCII lines execute over zero-allocation $O(1)$ byte lookup tables, while multi-byte UTF-8 sequences evaluate Unicode character properties (`char::is_alphabetic`, `char::is_alphanumeric`, `char::is_uppercase`), preserving sub-microsecond performance budgets while ensuring complete international script support.

---

## Table of Contents

1. [Phase 1 — Foundational Unicode Character Class Primitives & Word Boundaries (`diffuse-syntax`)](#1-phase-1--foundational-unicode-character-class-primitives--word-boundaries-diffuse-syntax)
2. [Phase 2 — Semantic Token Classification & Entropy Invariance (`diffuse-core`)](#2-phase-2--semantic-token-classification--entropy-invariance-diffuse-core)
3. [Phase 3 — Cluster Dynamic Programming & Statement Head Generalization (`diffuse-core`)](#3-phase-3--cluster-dynamic-programming--statement-head-generalization-diffuse-core)
4. [Phase 4 — Sub-Line Token Alignment & CamelCase Unicode Boundaries (`diffuse-core`)](#4-phase-4--sub-line-token-alignment--camelcase-unicode-boundaries-diffuse-core)
5. [Phase 5 — Golden Corpus Verification & Full Pipeline Sign-Off](#5-phase-5--golden-corpus-verification--full-pipeline-sign-off)

---

## 1. Phase 1 — Foundational Unicode Character Class Primitives & Word Boundaries (`diffuse-syntax`)

### Objective
Establish generic Unicode word character and prefix primitives in `diffuse-syntax`, and update the zero-allocation scanner and semantic classifier to process arbitrary Unicode identifiers and PascalCase types without degrading ASCII performance.

### Architecture & Mechanics
- **Generic Word Primitives**: In [`crates/diffuse-syntax/src/registry.rs`](crates/diffuse-syntax/src/registry.rs):
  - Introduce `is_word_char(c: char) -> bool`:
    ```rust
    pub fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }
    ```
  - Introduce `word_prefix(s: &str) -> &str`, `word_prefix_len(s: &str) -> usize`, and `first_word(s: &str) -> Option<&str>`.
  - Retain `is_ascii_word_char`, `ascii_word_prefix`, and `first_ascii_word` as transparent aliases/forwarding functions to prevent breaking external consumers.
- **Hand-Rolled Scanner Generalization**:
  - In `for_each_syntax_token_span`:
    - Preserve the `text.is_ascii()` branch with byte-level `ASCII_KIND` table lookup for ASCII throughput.
    - In the UTF-8 `chars` loop: replace `c.is_ascii_alphabetic() || c == '_'` with `c.is_alphabetic() || c == '_'` for identifier dispatch.
    - In `scan_ident`: advance while `is_word_char(chars[j].1)`.
    - In `scan_hex` and `scan_num_or_other`: use `is_word_char` for trailing `\b` boundary enforcement.
- **Semantic Syntax Classifier**:
  - In [`crates/diffuse-syntax/src/classifier.rs`](crates/diffuse-syntax/src/classifier.rs):
    - Replace `c.is_ascii_alphabetic() || c == '_'` with `c.is_alphabetic() || c == '_'`.
    - Replace `tok.chars().next().is_some_and(|c| c.is_ascii_uppercase())` with `tok.chars().next().is_some_and(|c| c.is_uppercase())` so non-ASCII PascalCase types highlight as `TokenColor::Type`.

### Verification
- Author targeted tests in [`crates/diffuse-syntax/src/tests.rs`](crates/diffuse-syntax/src/tests.rs):
  - Test `first_word` and `word_prefix` on Latin (`café`), German (`über_funktion`), Greek (`αριθμός`), Cyrillic (`счетчик`), and CJK (`ユーザー`).
  - Test `for_each_syntax_token_span` and `classify_tokens` asserting multi-byte identifiers form single cohesive tokens with correct `TokenColor::Default` or `TokenColor::Type` classifications.
- Gate: `scripts/check.sh` passes.

---

## 2. Phase 2 — Semantic Token Classification & Entropy Invariance (`diffuse-core`)

### Objective
Ensure that `diffuse-core`'s semantic token classifier identifies Unicode identifiers, functions, and words as first-class semantic units rather than fragmenting or misweighting them in entropy calculations.

### Architecture & Mechanics
- **Unicode Identifier Detection**: In [`crates/diffuse-core/src/token.rs`](crates/diffuse-core/src/token.rs):
  - Update `is_identifier_like(t: &str) -> bool`:
    - If `t.is_ascii()`, execute fast byte validation (`b[0].is_ascii_alphabetic() || b[0] == b'_'`).
    - If non-ASCII, validate using `chars()`:
      ```rust
      let mut chars = t.chars();
      let Some(first) = chars.next() else { return false; };
      if !first.is_alphabetic() && first != '_' {
          return false;
      }
      if !chars.all(|c| c.is_alphanumeric() || c == '_') {
          return false;
      }
      t.contains('_') || t.chars().any(char::is_uppercase)
      ```
  - Ensures identifiers with non-ASCII characters receive `TokenClass::Identifier` (weight 2.0) instead of falling through to `TokenClass::ProseWord` (weight 1.5).
- **Whitespace Classification Consistency**:
  - In [`crates/diffuse-core/src/lcs.rs`](crates/diffuse-core/src/lcs.rs): update `token_is_ws` to test `is_ws_char` (`c.is_whitespace() || c == '\u{200b}' || c == '\u{feff}'`) consistently with `tokenize_line`.

### Verification
- Targeted unit tests in [`crates/diffuse-core/src/token/tests.rs`](crates/diffuse-core/src/token/tests.rs) verifying `classify_token` across multilingual identifiers (`first_word`, `café_latte`, `αριθμός_1`, `СчетчикСтрок`).
- Verify `token_self_information` awards appropriate entropy to Unicode tokens.
- Gate: `scripts/check.sh` passes.

---

## 3. Phase 3 — Cluster Dynamic Programming & Statement Head Generalization (`diffuse-core`)

### Objective
Generalize the DP cluster solver's syntactic statement-head alignment heuristic to recognize statement heads and comment subjects across arbitrary language scripts.

### Architecture & Mechanics
- **`DpRowMeta` Generalization**: In [`crates/diffuse-core/src/cluster/dp.rs`](crates/diffuse-core/src/cluster/dp.rs):
  - Rename `first_ascii_word` field in `DpRowMeta` to `first_word`.
  - Populate `first_word` using `diffuse_syntax::registry::first_word(text)`:
    ```rust
    let first_word = if comment {
        let comment_body = trimmed
            .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
            .trim_start();
        first_word(comment_body).map(|w| arena.interner.intern(w))
    } else {
        first_word(text).map(|w| arena.interner.intern(w))
    };
    ```
  - Update all references in `solve_cluster_matrix` to use `first_word`.
- **Token Interner Alphanumeric Integrity**:
  - Confirm `token_is_alnum` in [`crates/diffuse-core/src/lcs.rs`](crates/diffuse-core/src/lcs.rs) correctly flags interned Unicode identifiers in `TokenInterner::alnum`.

### Verification
- Unit tests in [`crates/diffuse-core/src/cluster/tests.rs`](crates/diffuse-core/src/cluster/tests.rs) testing clustering and statement-head agreement on multilingual code changes (e.g. Spanish, German, Japanese function definitions).
- Gate: `scripts/check.sh` passes.

---

## 4. Phase 4 — Sub-Line Token Alignment & CamelCase Unicode Boundaries (`diffuse-core`)

### Objective
Ensure that line tokenization and Gotoh sub-line sequence alignment properly split and match compound Unicode identifiers, CamelCase boundaries, and digit boundaries.

### Architecture & Mechanics
- **Line Tokenizer CamelCase Boundaries**:
  - In [`crates/diffuse-core/src/lcs.rs`](crates/diffuse-core/src/lcs.rs), verify that `for_each_token` and `LineTokenizer`'s non-ASCII branch handles:
    - Lowercase to uppercase transitions: `prev_c.is_lowercase() && next_c.is_uppercase()`.
    - Uppercase acronym to lowercase transitions: `prev_prev_c.is_uppercase() && prev_c.is_uppercase() && next_c.is_lowercase()`.
    - Numeric transitions: `next_c.is_ascii_digit()` / `next_c.is_numeric()`.
- **Information-Theoretic Gotoh Alignment**:
  - Verify that the entropy-weighted Gotoh DP aligns identical Unicode tokens with high confidence and prevents spurious separator alignment.

### Verification
- Author comprehensive tests in [`crates/diffuse-core/src/lcs/tests.rs`](crates/diffuse-core/src/lcs/tests.rs) testing tokenization and alignment of:
  - CamelCase in Cyrillic (`МойКласс` → `МойНовыйКласс`)
  - Accent modifications (`param_créé` → `param_modifié`)
  - East Asian ideographs interspersed with Latin identifiers
- Gate: `scripts/check.sh` passes.

---

## 5. Phase 5 — Golden Corpus Verification & Full Pipeline Sign-Off

### Objective
Verify that all 23 frozen golden corpora remain byte-identical or show verified improvements, with zero regressions across the dual-leg acceptance suite.

### Architecture & Mechanics
- Run [`crates/diffuse/tests/golden.rs`](crates/diffuse/tests/golden.rs) covering both library and binary legs across all 23 acceptance fixtures.
- Assert that pure ASCII fixtures are completely unaffected.
- Execute full verification gate (`./scripts/check.sh`) ensuring formatting, clippy (`-D warnings`), and test budgets (< 0.5s unit, < 3s warm check) are satisfied.

### Sign-off Checklist
- [ ] Phase 1 implemented & reviewed
- [ ] Phase 2 implemented & reviewed
- [ ] Phase 3 implemented & reviewed
- [ ] Phase 4 implemented & reviewed
- [ ] Phase 5 verified across golden corpus and `./scripts/check.sh`
- [ ] Archive plan to `plan/done/Unicode Word & Alphanumeric Token Generalization.md`
