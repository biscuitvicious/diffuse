# diffuse — the rewrite plan

The diff-engine, in Rust. The pi review extension's `review.ts` used to do
everything: parse the git diff, mark moved lines, collapse pairs into mods,
merge the word-diff, *and* render the TUI. This project splits that:
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
│  renderer (the pi extension's TS TUI today, a    │
│  Rust TUI later) — row assembly, theme           │
│  application, syntax → theme mapping, the        │
│  component                                       │
└─────────────────────────────────────────────────┘
```

**In (the engine, ported to Rust):**

| TS source (extension/review.ts) | Role |
|---|---|
| `hunkPath`, `unquoteGitPath`, `parseDiff` | diff → hunks/rows, the fixture shape |
| `addCount`/`delCount` | hunk new/old side extents |
| `tokenizeLine`, `lcsPairs`, `spansFrom`, `wordSpans` | the word-level LCS |
| `lineSimilarity`, `lineSimilarityParts` | pairing scores |
| `markMoves` | exact-content relocation + the block-move rule |
| `collapseHunkRows` | pairing, stretch, reverse-stretch, reflow, the ordered walk — **the big one** |
| `isTailBeforeClosing`, `fnOf` (inside collapse) | the brace-anchor |
| `wholesalePair` | the split-word/garble gate |
| the component's merge methods | `mergedRuns`, `delPairing`, `delSpansToShow`, `isWholesaleMod`, `isPureDeletionMod`, `displayText` |
| `fileCounts` | the header counts |
| `langProfile`, `tokenReFor`, `classifyTokens` | the *semantic* token colors (keyword/string/comment/…) — the renderer maps these to theme colors |

**Out (the renderer, stays in TS):**

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
future Rust TUI both consume it. Freeze the schema; the engine internals may
change in Phase 5, the contract does not.

**Input** (stdin, or `--diff <file>`; contents keyed by path — the exact
shape of `tests/golden/mirage-2025-08.json`):

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
        "tail": [ { "newLine": 156, "text": "    }" } ]
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
- `tail` is a mod row's absorbed section lines (the renderer draws them as
  pure context: `newLine` + `text`, no tint) — added at Phase 3/4 when the
  acceptance walk and the renderer began consuming the contract (a mod's
  visual block needs its absorbed lines; the header counts count them).
- The bin's output IS the golden-walk input: the acceptance test (§5)
  decodes the token-stream JSON and walks it — no separate `--golden`
  format. The output is COMPACT JSON (the pretty form tripled the wire).

---

## 3. The port map (the one-for-one checklist)

Every TS function has a Rust counterpart with the SAME name, SAME control
flow, SAME comments. The map is the checklist — a phase-1 task is done when
every row is checked:

| review.ts | diffuse/src | notes |
|---|---|---|
| `parseDiff` | `parse.rs` | ✅ incl. `hunkPath`, `unquoteGitPath`, the `@@` header parse |
| `addCount`/`delCount` | `parse.rs` | ✅ |
| `tokenizeLine` | `lcs.rs` | ✅ the `\p{White_Space}\u200b` classes — §6 |
| `lcsPairs` | `lcs.rs` | ✅ the tie-break ladder — §6 |
| `spansFrom` | `lcs.rs` | ✅ |
| `wordSpans` | `lcs.rs` | ✅ the 0.25 whole-line gate |
| `lineSimilarity`/`lineSimilarityParts` | `similarity.rs` | ✅ |
| `markMoves` | `moves.rs` | ✅ the reflow check, the block-move flush |
| `collapseHunkRows` | `collapse.rs` | ✅ the pairing loop, `bestRun`, the stretch gate, the reverse-stretch, the reflow-common, the ordered walk, the sort |
| `isTailBeforeClosing`/`fnOf` | `tail.rs` | ✅ |
| `wholesalePair` | `merge.rs` | ✅ the split-word + the multi-line exemption |
| `mergedRuns`/`delPairing`/`delSpansToShow`/`displayText` | `merge.rs` | ✅ incl. the whitespace-pair minimum, the reverse-stretch shared-prefix merge |
| `isWholesaleMod`/`isPureDeletionMod` | `merge.rs` | ✅ |
| `fileCounts` | `counts.rs` | ✅ |
| `langProfile`/`tokenReFor`/`classifyTokens` | `syntax.rs` | ✅ the semantic colors |
| the engine pipeline + the wire contract | `bin_pipeline.rs` + the bin | ✅ parse → old-content reconstruction → moves → collapse → colored runs → counts; the JSON is §2's contract |
| `oldContentFromDiff` | `old_content.rs` | ✅ (from the golden test) — the old side for the brace-anchor |

The component's other methods (`rowSegments`, `mergedTokens`, the prefix
assembly) stay in TS — but `mergedTokens`' *classification* output (the
`color: "syntax…"` per token) is engine-side (the `runs` in the contract).

---

## 4. The phases

### Phase 0 — the frame ✅
- [x] Project skeleton (bin + lib, Clap, serde, this file).
- [x] Freeze the wire contract (§2) as the serde types — the schema IS the
      code (the `tail` field lands at Phase 4 when the renderer consumes it).
- [x] The frozen corpus in `tests/golden/`.

### Phase 1 — the one-for-one port ✅
- [x] Port every function in the map, in dependency order (parse → lcs →
      similarity → moves → tail → collapse → merge → counts → syntax).
- [x] 100% fidelity: same names, same branches, same thresholds, same
      fallbacks — the TypeScript is the spec. Rust idioms are FORBIDDEN
      (no iterator chains that change control flow, no enum-shaped
      rewrites). Break only for syntax (no `undefined`, no dynamic typing)
      and naming (`camelCase` → `snake_case`).
- [x] The comments came over — they carry the *why* of every odd rule.
- [x] The `.map`/`Map<number,number>` → `HashMap`/`BTreeMap` conversions are
      fidelity-neutral, iteration order preserved where it matters.

### Phase 2 — the extension moves into the workspace ✅ ✅
The pi-review extension stops being a separate repo and becomes a
sub-project of diffuse — **co-located before the tests and the integration**,
so the engine and its consumer change together:

```
diffuse/                    (the engine crate at the root; the extension
├── src/                    is a sub-project, not a Cargo member — it is
│   └── …                   TypeScript, loaded by pi as-is)
├── tests/golden/           the SHARED frozen corpus (fixture + golden) —
│                           the single canonical copy, read by both the
│                           Rust acceptance and the extension's golden walk
├── extension/              the extensions home — one subdir per agent
│   └── pi/                 the Pi review extension — the TS renderer + TUI
│       ├── review.ts       the whole extension (single file, pi loads it)
│       ├── bin/diffuse     the BUNDLED diffuse binary (build-bin.sh — the
│       │                   extension ships with the engine it was tested
│       │                   against; review.ts resolves it next to itself)
│       ├── package.json    builds the extension bundle
│       ├── scripts/        check/test/golden/build-bin/install
│       └── test/           the unit + golden suites — running against the
│                           diffuse backend from Phase 4 on
└── Cargo.toml              the engine (lib + bin)
```

- [x] The extension's files moved in (review.ts, package.json, scripts,
      tests, docs) — `~/lab/pi-review` remains as the pristine upstream
      reference until the workspace copy is proven identical.
- [x] The extension's golden test reads the shared corpus at
      `tests/golden/` — no forked fixture/golden pair.
- [x] The extension's own test suite still passes in place (`npm test`).
- [x] The diffuse binary is BUNDLED with the extension (`bin/diffuse`),
      and `scripts/install.sh` syncs review.ts + the binary into Pi's
      extensions dir — the installed copy needs no env to find the engine.
- The reference paths in this doc and AGENTS.md now point at
  `extension/pi/review.ts`.

### Phase 3 — the tests in Rust ✅
Port the engine-side assertions in `test/review.test.ts` (1789 lines) into
`#[test]`s, one block per source block, same inputs, same expectations —
**the renderer-side blocks stay in TS** (they test `ReviewComponent`,
`buildVisible`, notes, markdown, movement — the renderer, which stays in
TS). "As applicable" is the rule: a block that drives an engine function
(`parseDiff`, `wordSpans`, `markMoves`, `collapseHunkRows`, the merge
methods, `fileCounts`, the syntax classifier) ports; a block that drives the
component stays.

- [x] Every engine-side block in the corpus has a Rust counterpart (the
      checklist in `tests/` and the per-module `tests.rs` files).
- [x] The golden acceptance (`tests/golden.rs`) — TWO legs over the frozen
      corpus, both must match byte-for-byte:
  1. **lib leg** — embed the engine, run the full pipeline, walk the output.
  2. **bin leg** — spawn the built `diffuse` binary, feed it the same
     input, decode the token-stream JSON, run the SAME walk.
  The walk asserts the wire contract is self-consistent over the real
  corpus: the row-kind/line-number rules, the spans in bounds, the runs
  concatenating to the display text, the header counts matching a walk of
  the rows, and the two legs byte-identical.
- [x] The wire contract gained the mods' `tail` (the absorbed section
      lines) — the renderer needs them, and the counts walk needs them.
- **The rendered golden walk lives in the extension** (it needs the
  renderer — the golden.txt is the TUI's colored scrolldown). The Rust
  side proves the engine + the contract; the extension proves the render.
  The two meet at Phase 4's equivalence gate.
- **The gate: everything passes, 100%, before Phase 4.** Any failure is a
  port bug — fix the port, do not change the expectation.

### Phase 4 — the TS integration ✅
- [x] The extension spawns the bin with the review input (`REVIEW_BACKEND=rust`,
      `REVIEW_DIFFUSE_BIN` override), receives the token-stream JSON, and
      renders it with the existing component: `filesFromWire` converts the
      wire output into the `DiffFile`/`Row` shapes (the content arrays stay
      the extension's own — buildVisible reads them; the rows carry the
      engine's `runs`).
- [x] The renderer seam: `rustTokens` supplies the token stream for the rows
      the TS would merge (non-wholesale mods + moved-diff rows), from the
      wire runs. **The golden encodes the renderer's OWN classification**
      (the cursor file's syntax profile — every row classifies with the
      cursor's file), so this renderer re-derives the semantic colors with
      its own profile over the runs' merge structure (adjacent same-kind
      runs are merged back to the coarse runs, then classified) — the
      engine's per-file colors ride the contract for other consumers, and
      are pinned to the reference by the golden's rust-leg classification
      check. The .md fence/markdown paths and the plain add/ctx rows stay
      TS-side.
- [x] `REVIEW_BACKEND` is GONE — the extension is rust-only: it spawns the
      bundled diffuse binary (the TS engine was removed with the reference
      repo; the engine is tested in Rust).
- [x] The diffuse binary is BUNDLED with the extension (`bin/diffuse`),
      and `scripts/install.sh` syncs review.ts + the binary into pi's
      extensions dir — the installed copy finds its engine with no env.
- [x] **The equivalence proof passed**: the extension's golden walk with
      the rust backend produces the byte-identical golden — same
      scrolldown, same width invariant, every flat row reachable. The
      golden's rust leg ALSO walks every wire run and asserts its per-file
      colors equal the TS tokenizer's — the engine's classification is
      pinned to the reference, not just the render.
- [x] The renderer derives its token stream from the WIRE'S SPANS via the
      TS-side merge machinery (`mergedTokens`) — byte-identical to the TS
      backend by construction. (The runs were first consumed via a
      fine→coarse reconstruction, which proved LOSSY — adjacent same-kind
      coarse runs, e.g. four adjacent del runs, merged into one — the
      diffuse corpus caught it.) The wire runs stay in the contract,
      validated by the acceptance walk + the golden's classification check,
      for other consumers.
- [x] The frozen corpus gained a SECOND pair: `diffuse-2025-08` — the
      diffuse repo's own port diff (26 files, 2592 diff lines), locked as
      both a fixture + a golden. Both corpora run through the acceptance
      walk and the extension's golden walk (rust leg).
- [x] **The TS backend is dropped** — the built-in engine is removed from
      the extension (the user's manual review is done; the golden's rust
      leg is routine). The unit suite is reworked to build its files via
      the wire; the engine-side blocks live in Rust. The reference repo
      (~/lab/pi-review) is deleted.

**Follow-up (deferred per the user's request): the runs' JSON shape.** The
wire runs are `[{kind, color?, text}, …]` objects. The user suggested a
more compact structured form — `[{add: "xyz"}, {del: "abc"}]` — one key
per run, less bandwidth, the kind as the key. Do it after the rust backend
is 100% validated; it touches the contract (both the serde types + the
consumers), so the golden.rs bin leg and the extension's wire decode must
move together. JSONL (one file-object per line) was also evaluated: its
benefits (lower transient parse memory, crash robustness — the frontend
keeps completed files if the bin dies mid-output) are real but marginal
while the component renders all-at-once; it becomes worth it when a future
consumer (the Rust TUI) renders file-by-file incrementally — the contract
is already per-file, so it's a natural later step.

### Phase 5 — the Rust-y redesign
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

### Phase 6 — the engine as application-preserving refactorings

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

Phase 6 is deliberately last: it presupposes the port is faithful (Phase
3's acceptance), integrated (Phase 4), and co-located (Phase 2) — then the
refactoring structure is a rewrite of the *architecture*, not the behavior.

---

## 5. Acceptance gates

| Gate | What proves it |
|---|---|
| Phase 1 | every port-map row is checked; `cargo build` clean |
| Phase 2 | the extension lives in the workspace, its suite passes in place, and it reads the shared corpus |
| Phase 3 | all unit tests pass; the golden acceptance — **lib leg and bin leg** — walks the frozen corpus byte-identically with the wire-contract invariants (§2 contract pinned to the engine) |
| Phase 4 | the extension renders from the rust backend (the golden's rust leg — ✅ passed); the TS backend and the reference repo are removed (✅) |
| Phase 5 | the redesign passes the golden unchanged; the extension builds from the workspace and passes its tests against the diffuse backend; clippy `-D warnings`; the docs updated |
| Phase 6 | the pass pipeline passes the golden AND the apply-oracle: `apply(original) == apply(each transformed step) == the checked-in new file`, for every corpus fixture |

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
13. **The stretch-consumption splice quirk**: the TS consumes a stretch's
    adds with `pendingAdds.splice(indexOf(a), 1)` — an add ALREADY
    consumed by an earlier pair has `indexOf` -1, and `splice(-1, 1)`
    removes the LAST pending add (the trailing adds drop out of the
    pairing and surface as the stretch mod's absorbed section). The port's
    `retain` must replicate the pop (the diffuse-repo PLAN.md case).
14. **The old-content dash-content del**: a del whose CONTENT starts with a
    dash (`-- item` — a markdown list bullet `- item`) is a real del; only
    the three-dash `--- a/file` header is skipped. A `rest.starts_with('-')`
    check swallows it and shifts every later old line by one.
15. **`char_slice` is total**: the JS `String.slice` clamps and never
    panics; empty del/add lines yield empty spans `(0,0)` that the
    wholesale check slices. The port must clamp, not panic.
16. **The new-file `@@ -0,0` underflow**: a new file's hunk has old start
    0; `oi - 1` underflows usize. Debug PANICS (which surfaced it);
    RELEASE WRAPS and grows the vec to OOM — the stale bundled release
    binary did exactly this. The `@@` grow must saturate; a del at `oi == 0`
    must not index. Lesson: rebuild the bundled bin after engine fixes —
    the `diffuse-2025-08` corpus (with its new files) now guards it.
17. **The stretch-window blowup is O(n²)**: the consecutive-add windows
    (`for w in 2..=len` × `for k in 0..=len-w`) are built eagerly — n³/6
    elements for a run of n adds. A git diff with a huge new file (a
    4000-line whole-file add) OOMs BOTH engines. Real usage dodges it
    (untracked files bypass collapse); the golden corpora must too — the
    diffuse corpus excludes the >1000-line new files. A lazy-window
    redesign is Phase 5 material.

---

## 7. Open questions

- **`buildVisible`/context allocation**: engine or renderer? The context
  rows are presentation, but the `absorbLeading/ClosingContext` shifts
  read the hunk rows. Default: keep in TS for Phase 1-4; revisit at
  Phase 5.
- **Syntax highlighting**: `classifyTokens`' semantic output is
  engine-side; the fence-state tracking for `.md` rows is a renderer
  concern (it spans visual rows). Keep the fence state in TS.
- **WASM vs spawn** for the TS integration: spawn is simpler (the bin IS
  the contract); WASM is one build away if spawn latency matters. Start
  with spawn.
- **The golden walk's home**: it needs the renderer, so it lives in the
  extension's TS suite; the Rust acceptance proves the engine + contract.
  If a Rust TUI materializes (Phase 5+), the walk moves into it.

---

## 8. Definition of done

The project is done when:

- `diffuse` (lib) is the complete engine, `diffuse` (bin) speaks the
  frozen contract.
- Every engine-side test from `test/review.test.ts` passes in Rust, and
  the golden acceptance (lib leg + bin leg) walks the frozen corpus
  byte-identically.
- The pi extension (in-workspace) renders from the Rust backend with a
  byte-identical golden, and `REVIEW_BACKEND` is a relic (defaulted to
  rust) after the user's manual sign-off.
- Phase 5's redesign is done, the golden still passes, and the docs
  (this file, README, AGENTS) describe the Rust-y shape.
- Phase 6's refactoring pipeline is done: each pass is a pure diff → diff
  transformation, the apply-oracle passes on every fixture, and the render
  concerns (the ordered walk) have moved out of the engine.
- A Rust TUI consumes the same contract (§2) — the near-term follow-up.
