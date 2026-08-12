# diffuse — the rewrite plan

The diff-engine, in Rust. The pi review extension's `review.ts` currently
does everything: parse the git diff, mark moved lines, collapse pairs into
mods, merge the word-diff, *and* render the TUI. This project splits that:
**diffuse is the engine** (parse → moves → collapse → merged token runs),
renderer-agnostic — consumed today by a thin TypeScript renderer in the pi
extension, tomorrow by a Rust TUI.

This document is the reference for the rewrite. If we get lost at sea, this
is the map: the boundary, the wire contract, the port map, the phases, the
known shallows.

---

## 1. The boundary

```
git diff + file contents
        │
        ▼
┌─────────────────────────────────────────────────┐
│  diffuse (Rust) — THE ENGINE                     │
│  parseDiff → markMoves → collapseHunkRows →      │
│  merged runs/tokens (semantic colors)            │
└─────────────────────────────────────────────────┘
        │  token-stream JSON (the wire contract, §2)
        ▼
┌─────────────────────────────────────────────────┐
│  renderer (pi extension TS today, Rust TUI       │
│  later) — row assembly, theme application,       │
│  syntax → theme mapping, the component           │
└─────────────────────────────────────────────────┘
```

**In (the engine, ported to Rust):**

| TS source (review.ts) | Role |
|---|---|
| `hunkPath`, `unquoteGitPath`, `parseDiff` (579) | diff → hunks/rows, the fixture shape |
| `addCount` (237), `delCount` (261) | hunk new/old side extents |
| `tokenizeLine` (906), `lcsPairs` (927), `spansFrom` (988), `wordSpans` (1011) | the word-level LCS |
| `lineSimilarity` (1230), `lineSimilarityParts` (1239) | pairing scores |
| `markMoves` (1150) | exact-content relocation + the block-move rule |
| `collapseHunkRows` (1303) | pairing, stretch, reverse-stretch, reflow, the ordered walk — **the big one** |
| `isTailBeforeClosing`, `fnOf` (inside collapse) | the brace-anchor |
| `wholesalePair` (1052) | the split-word/garble gate |
| the component's merge methods | `mergedRuns`, `delPairing`, `delSpansToShow`, `isWholesaleMod`, `isPureDeletionMod`, `displayText` |
| `fileCounts` (1694) | the header counts |
| `langProfile` (1848), `tokenReFor` (1902), `classifyTokens` (1927) | the *semantic* token colors (keyword/string/comment/…) — the renderer maps these to theme colors |

**Out (the renderer, stays in TS for now):**

| TS source | Role |
|---|---|
| `ReviewComponent` + `rowSegments` | the TUI, the theme application |
| `buildVisible`, `allocateContext`, `contextRows`, `hiddenGap` | the flat visible-row assembly (boundary question — see §7) |
| `renderMarkdown`, `markdownTokens` | note / .md rendering |
| `parseReviewArgs`, `editorArgs`, `splitTemplate`, `substitute` | the extension's own CLI |
| the note editor, the state persistence | extension concerns |

---

## 2. The wire contract (frozen early, changes only deliberately)

The bin's JSON output is the integration contract — the TS renderer and the
future Rust TUI both consume it. Freeze the schema at Phase 0; the engine
internals may change in Phase 4, the contract does not.

**Input** (stdin, or `--diff <file>`; contents keyed by path — the exact
shape of `test/fixtures/mirage-2025-08.json`):

```json
{
  "diff": "<the raw git diff>",
  "contents": { "path": "<new file content>", "..." }
}
```

**Output** — per file, the hunks, each hunk's collapsed rows, each row's
merged runs:

```json
{
  "files": [{
    "path": "src/…/cache.rs",
    "status": "modified",
    "newLineCount": 186,
    "oldLineCount": 129,
    "hunks": [{
      "oldStart": 111, "oldCount": 3,
      "newStart": 147, "newCount": 9,
      "rows": [{
        "kind": "mod", "newLine": 155, "oldLine": 113,
        "text": "        Some(result)",
        "delText": "        None",
        "spans": { "del": [[0,12]], "add": [] },
        "runs": [ { "kind": "common", "text": "…", "color": "syntaxKeyword" },
                  { "kind": "del", "text": "…" }, { "kind": "add", "text": "…" } ],
        "tail": null
      }]
    }]
  }],
  "counts": { "adds": 3, "dels": 0, "mods": 1, "moved": 0 }
}
```

- `runs` are the merged token stream — the semantic colors only
  (`syntaxKeyword`, `syntaxString`, …), never theme values.
- A `del`/`add` run carries no color field (the strike/fill is the
  renderer's business); the semantic color rides the `common` runs and the
  merged tokens' syntax classification.
- `--golden` switches the output to the golden-walk format (the acceptance
  test, §5).

---

## 3. The port map (the one-for-one checklist)

Every TS function has a Rust counterpart with the SAME name, SAME control
flow, SAME comments. The map is the checklist — a phase-1 task is done when
every row is checked:

| review.ts | diffuse/src | notes |
|---|---|---|
| `parseDiff` | `parse.rs` | incl. `hunkPath`, `unquoteGitPath`, the `@@` header parse |
| `addCount`/`delCount` | `parse.rs` | |
| `tokenizeLine` | `lcs.rs` | the `\p{White_Space}\u200b` classes — §6 |
| `lcsPairs` | `lcs.rs` | the tie-break ladder — §6 |
| `spansFrom` | `lcs.rs` | |
| `wordSpans` | `lcs.rs` | the 0.25 whole-line gate |
| `lineSimilarity`/`lineSimilarityParts` | `similarity.rs` | |
| `markMoves` | `moves.rs` | the reflow check, the block-move flush |
| `collapseHunkRows` | `collapse.rs` | the pairing loop, `bestRun`, the stretch gate, the reverse-stretch, the reflow-common, the ordered walk, the sort |
| `isTailBeforeClosing`/`fnOf` | `tail.rs` | |
| `wholesalePair` | `merge.rs` | the split-word + the multi-line exemption |
| `mergedRuns`/`delPairing`/`delSpansToShow`/`displayText` | `merge.rs` | incl. the whitespace-pair minimum, the reverse-stretch shared-prefix merge |
| `isWholesaleMod`/`isPureDeletionMod` | `merge.rs` | |
| `fileCounts` | `counts.rs` | |
| `langProfile`/`tokenReFor`/`classifyTokens` | `syntax.rs` | the semantic colors |

The component's other methods (`rowSegments`, `mergedTokens`, the prefix
assembly) stay in TS — but `mergedTokens`' *classification* output (the
`color: "syntax…"` per token) is engine-side (the `runs` in the contract).

---

## 4. The phases

### Phase 0 — the frame
- [x] Project skeleton (bin + lib, Clap, serde, this file).
- [ ] Freeze the wire contract (§2) — write it as the serde types in
      `parse.rs`/`lib.rs` so the schema IS the code.
- [ ] Port the fixture: copy `mirage-2025-08.json` into `tests/golden/`.

### Phase 1 — the one-for-one port
- Port every function in the map, in dependency order (parse → lcs →
  similarity → moves → tail → collapse → merge → counts → syntax).
- **100% fidelity**: same names, same branches, same thresholds, same
  fallbacks — the TypeScript is the spec. Rust idioms are FORBIDDEN here
  (no iterator chains that change control flow, no enum-shaped rewrites).
  Break only for syntax (no `undefined`, no dynamic typing) and naming
  (`camelCase` → `snake_case`).
- The comments come over too — they carry the *why* of every odd rule
  (the tie-break ladder, the artifact-filter position check, the reflow
  gates).
- The `.map`/`Map<number,number>` → `HashMap`/`BTreeMap` conversions are
  fidelity-neutral as long as iteration order is preserved where it matters
  (the ordered walk, the sort).

### Phase 2 — the tests in Rust
- Port every assertion in `test/review.test.ts` (1789 lines) into
  `#[test]`s, one block per source block, same inputs, same expectations.
- Port the golden walk as a **`tests/` e2e/acceptance test with two
  legs**:
  1. **lib leg** — embed the lib, run the full pipeline on the frozen
     fixture, walk the output and compare against
     `mirage-2025-08.golden.txt`.
  2. **bin leg** — spawn the built `diffuse` binary, feed it the same
     input, decode the token-stream JSON, run the SAME walk over it, and
     assert the same golden — this pins the wire contract (§2) to the
     golden as well as the engine.
  Both legs walk the full scrolldown: the width invariant, every flat row
  reachable, the byte-exact golden rows.
- **The gate: everything passes, 100%, before Phase 3.** Any failure is a
  port bug — fix the port, do not change the expectation.

### Phase 3 — the TS integration
- The extension spawns the bin (or loads the WASM build) with the review
  input, receives the token-stream JSON, and renders it with the existing
  component (the `runs` replace `mergedRuns`, `spans` replace the baked
  spans).
- `REVIEW_BACKEND=rust|ts` (or a `/review` flag) keeps the TS engine as a
  fallback during the transition — A/B the two backends on the same input.
- **The equivalence proof**: the TS renderer fed the Rust backend's JSON
  must produce the byte-identical golden. That is the integration gate.

### Phase 4 — the Rust-y redesign
- Only after the engine is fully ported, tested, and integrated. Now the
  type system earns its keep:
  - `RowKind` as a proper enum; spans as typed ranges; the pairing
    decisions as an explicit state machine.
  - The LCS as a real algorithm (the `Uint32Array` → proper storage, maybe
    Hirschberg for the long lines).
  - `Result`/`Option` instead of the TS fallbacks; the `?`-heavy parse.
  - The stretch/reverse-stretch/reflow machinery as typed structures
    instead of `Map`s of rows.
  - Zero-copy where the content is borrowed, not cloned.
- **The golden is the regression gate**: any visual change needs the
  user's sign-off — the golden encodes their exact preferences (the
  strike/fill minimums, the anchor pairings, the reverse-stretch shape).

### Phase 5 — the engine as application-preserving refactorings

After every stage of the rewrite is done and green. The engine stops being
one global pipeline and becomes a sequence of **composable passes over a
diff tree**, each pass a 1:1 *refactoring* of the diff:

> A pass transforms the diff structure (rows become mods, runs stretch,
> dels get reflowed, spans attach) while the **applied result is
> byte-identical**: `apply(original_diff, old_file)` ==
> `apply(transformed_diff, old_file)` for every transformation, at every
> step.

The diff is a *program* (a tree of applicable rows — each row knows how
it edits the file: a del removes `oldLine`, an add inserts `newLine`, a
mod removes its delText's lines and inserts its text, a stretch mod's
multi-line text is several inserted lines). The engine is a pipeline of
refactorings: `pair` (the `-`/`+` → mod merge), `moves` (inert
annotations), `stretch`/`reverse-stretch` (line-group merges — the
concatenated text applies identically), `reflow` (inert absorption),
`subword` (span/run annotations).

**Why this is better:**
- **The apply-oracle is a complete invariant.** Every pass — and every
  composition of passes — is validated by the mechanical check: apply the
  original diff and the transformed diff to the same old file, compare
  byte-for-byte. No pass can silently change meaning.
- **Each pass is testable in isolation.** A pass is a pure `diff → diff`
  function with a precise name and a small local change. The hairy
  global state of the current collapse (the ordered walk, the anchors,
  the sort) turns out to be a RENDER concern — the diff tree only carries
  semantics; the renderer assembles the display order.
- **The golden corpus becomes the oracle corpus.** The fully-transformed
  golden walk already asserts the render; the apply-oracle adds the
  semantic half: every fixture's old file + original diff, and old file +
  transformed diff, must produce the same new file — which is also the
  actual checked-in file.

Phase 5 is deliberately last: it presupposes the port is faithful (Phase
2's golden), integrated (Phase 3), and idiomatic (Phase 4) — then the
refactoring structure is a rewrite of the *architecture*, not the
behavior.

---

## 5. Acceptance gates

| Gate | What proves it |
|---|---|
| Phase 1 | every port-map row is checked; `cargo build` clean |
| Phase 2 | all unit tests pass; the golden walk — **lib leg and bin leg** — matches `mirage-2025-08.golden.txt` on the frozen fixture (§2 contract pinned to the golden) |
| Phase 3 | `REVIEW_BACKEND=rust` renders the identical golden; A/B on the real repo |
| Phase 4 | the redesign passes the golden unchanged; clippy `-D warnings`; the docs updated |
| Phase 5 | the pass pipeline passes the golden AND the apply-oracle: `apply(original) == apply(each transformed step) == the checked-in new file`, for every corpus fixture |

---

## 6. The shallows — behaviors that WILL bite if misported

These are the rules the user has tuned by hand. Each is a port-trap:

1. **The LCS tie-break ladder** (`lcsPairs`): the exact order of the
   `else if` chain — a del token with no counterpart later in the add is
   dropped; a del punctuation against a word yields; a space yields to its
   next token's match. Getting the order wrong silently changes every
   span. Port the ladder as a ladder.
2. **The `\p{White_Space}` + U+200B classes** (`tokenizeLine`): the
   split is by Unicode character classes, not `\s`. The operator glue
   class (`[<>!=+\-*\/\\|&^%~?:._]`) and the delimiter class
   (`[()\[\]{};,@#\`"]`) — note the quote became a delimiter (the trace
   wobble fix).
3. **The 0.25 whole-line gate** (`wordSpans`): below the ratio, the whole
   line is one span — but the *matched-chars ratio is against the new
   text's length*.
4. **The artifact filter's position check** (`delSpansToShow`): a
   multi-span del fragment is dropped only when its text sits in the add
   AT THE SAME POSITION (`..]` → `..].`) — `lower` vs `?next_lower,` is
   a real replacement, not absorption.
5. **The block-move rule** (`markMoves`): a del run whose FIRST del did
   not move is a deletion — the interior moves drop with their paired
   adds.
6. **The brace-anchor** (`collapseHunkRows`): the del is a tail (the old
   line after it is the fn's closing brace) AND some pending add is a
   tail in the SAME fn. The verbatim-reflow skip must not preempt it.
7. **The reflow gates**: a non-first del of a run whose text is a
   substring of the candidate add doesn't steal the pair; a del whose
   text is verbatim in the new file is skipped unless it's a
   brace-anchor candidate; the reflow-common merge drops the mod's add
   spans that contain a reflow line's content.
8. **The stretch gate**: one del over a run of adds, sim ≥ 0.25, the
   del's single best must be CROSSED (≥ 3 del runs) and not wholesale.
9. **The reverse-stretch**: a del run folds into one mod — shared prefix
   common, fill first, the struck block across the visual lines (NOT the
   del spans' own positions, which interleave the fill mid-chain). A
   multi-line del span is exempt from the split-word wholesale check.
10. **The ordered walk's sort**: mods at their del's position, the dels'
    anchor-based keys, the comment-chain absorption, `[last, 2.5]` vs
    `[newLine, 1|2]` — the flat row order is load-bearing.
11. **The whitespace-pair minimum**: the shared prefix is the ACTUAL
    matching chars — a type change (tab vs spaces) shares nothing and
    both subword; pairs apply regardless of the position gap.
12. **The counts**: moves are neither added nor removed; mods are one row.

---

## 7. Open questions

- **`buildVisible`/context allocation**: engine or renderer? The context
  rows are presentation, but the `absorbLeading/ClosingContext` shifts
  read the hunk rows. Default: keep in TS for Phase 1-3; revisit at
  Phase 4.
- **Syntax highlighting**: `classifyTokens`' semantic output is
  engine-side; the fence-state tracking for `.md` rows is a renderer
  concern (it spans visual rows). Keep the fence state in TS.
- **WASM vs spawn** for the TS integration: spawn is simpler (the bin IS
  the contract); WASM is one build away if spawn latency matters. Start
  with spawn.
- **The golden corpus in the repo**: the fixture is ~300KB; keep it under
  `tests/golden/` as the frozen corpus, generated from the same mirage
  snapshot.

---

## 8. Definition of done

The project is done when:

- `diffuse` (lib) is the complete engine, `diffuse` (bin) speaks the
  frozen contract.
- Every test from `test/review.test.ts` + the golden walk passes in Rust.
- The pi extension renders from the Rust backend with a byte-identical
  golden, and `REVIEW_BACKEND` is a relic (defaulted to rust).
- Phase 4's redesign is done, the golden still passes, and the docs
  (this file, README, AGENTS) describe the Rust-y shape.
- Phase 5's refactoring pipeline is done: each pass is a pure diff → diff
  transformation, the apply-oracle passes on every fixture, and the render
  concerns (the ordered walk) have moved out of the engine.
- A Rust TUI consumes the same contract (§2) — the near-term follow-up.
