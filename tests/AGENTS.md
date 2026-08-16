# tests/ — integration & acceptance test guidelines

Project-wide conventions live in the root [`AGENTS.md`](../AGENTS.md); this
file covers the integration/acceptance side: the golden corpora, the fixture
file types, regeneration mechanics, and — most importantly — the review
discipline for any change in golden output.

## What lives here

- `golden.rs` — the Rust golden acceptance: **two legs** over every frozen
  corpus, both asserting the wire-contract invariants and byte-identical
  output:
  1. **lib leg** — embed the engine, run the full pipeline directly.
  2. **bin leg** — spawn the built `diffuse` binary, feed it the same input,
     decode its JSON, run the SAME walk.

  The bin leg pins the wire contract (PLAN.md §2) to the engine: a JSON
  schema change breaks it even if engine semantics are unchanged.
- `golden/` — the frozen corpora (below).

## The golden corpora

| corpus | rust walk | render golden | notes |
|---|---|---|---|
| `mirage-2025-08` | ✅ | ✅ | the original hand-tuned baseline |
| `diffuse-2025-08` | ✅ | ✅ | the engine diffing its own repo |
| `opt-1-cache-regex-window-slice` | ✅ | ✅ | perf commit captured as fixture |
| `opt-2-cache-markdown-regex` | ✅ | ✅ | perf commit captured as fixture |
| `opt-3-cache-lang-profile` | ✅ | ✅ | perf commit captured as fixture |
| `opt-4-manual-scanner-prefilter` | ✅ | ✅ | perf commit captured as fixture |
| `diffuse-history-mid` | ✅ | ❌ | huge (wire-only; too slow for the TS render walk) |

The rendered walk (`extension/pi/test/golden.test.ts`) covers the same
corpora minus `diffuse-history-mid`; it must never fork its own fixtures —
`tests/golden/` is the single canonical copy.

## Fixture file types per corpus

Each corpus `<name>` has up to three files, each with a **different change
policy**:

1. **`<name>.json`** — the frozen *input* fixture: the captured `git diff`,
   post-patch file contents, and (for `opt-*`) pre-patch `old_contents`.
   **Never regenerated** except when the corpus itself is deliberately
   changed (rare, human sign-off only).
2. **`<name>.wire.json`** — the frozen *wire-contract output* (the
   token-stream JSON) of the lib leg; both legs must match it
   byte-for-byte. Changes whenever engine behavior **deliberately**
   changes — under the review discipline below.
3. **`<name>.golden.txt`** — the frozen *rendered* output (the TUI's
   colored scrolldown). It locks the hand-tuned visual preferences
   (strike/fill minimums, anchor pairings, move-block shape,
   reverse-stretch). It changes only when a rendering preference is
   deliberately changed — under the review discipline below.

## Regeneration mechanics

- **wire.json** — run the rust acceptance with `GEN_GOLDEN=1` or
  `UPDATE_WIRE=1` (e.g. `UPDATE_WIRE=1 cargo test --test golden`). Without
  the flag, a mismatch prints a structured per-row wire diff and fails.
- **golden.txt** — via the extension's walk, and updates are **surgical**:
  1. Run without flags first — it reports the exact changed rows.
  2. `UPDATE_GOLDEN=1 GOLDEN_EXPECT="<rows>" node …/golden.test.ts` — the
     update verifies the changed set **equals** the expected set (rows are
     0-based golden line numbers; ranges like `734-740` work;
     `GOLDEN_CORPUS=<name>` scopes to one corpus).
  3. `GEN_GOLDEN=1` (full rewrite, no safety guard) is **restricted** to
     large-scale structural changes only — never for focused fixes.
- **Never regenerate any golden to make a test pass.** A failing golden
  means the engine deviates from the reviewed ideal; find the deviation.

## Review discipline for golden changes — the signoff gate

**Every change in golden output (`*.wire.json`, `*.golden.txt`) requires
BOTH agent review and human review.**

### Agent review (every time, before presenting the change)

The agent must walk the diff and **assert the new representation is
strictly better — or at least equivalently good — than the previous one**.
Concretely:

- Every changed region must be attributable to the intended behavior
  change; there must be no unexplained changes elsewhere in the corpus.
- Row-kind flips (`add`↔`ctx`, `mod`↔`del`+`add`, …), line renumbering,
  and moved rows must be consistent with the intent (e.g. semantic sliding
  snapping a boundary), not artifacts.
- Token runs must not degrade: common runs shouldn't shrink, struck/fill
  fragmentation shouldn't grow, and mid-word splits shouldn't appear where
  they didn't before.
- The full invariant walk and `apply_oracle` (string & hunk equivalence)
  must pass on every corpus.

**Anything questionable — anything the agent cannot confidently assert is
better-or-equivalent — requires explicit human review and signoff before
the fixture is accepted.** When in doubt, flag it and pause; do not
rationalize a suspicious diff.

### Human review & signoff

- The human does a **visual review** of the rendered before/after to catch
  "duh" problems (mispaired lines, garbled runs, nonsense hunks) that
  mechanical checks can't see.
- Golden fixtures land in the tree only after this signoff — normally just
  before committing (per the root commit policy).
- If the user has instructed the agent to commit automatically, the agent
  may mark a phase complete after its own agent review above — but it must
  still **highlight anything that looks suspicious and pause for review,
  even when explicitly asked to commit.**

### Tie-in with plan phase status

Phase checkboxes in `plan/` follow [`plan/Plan Guidelines.md`](../plan/Plan%20Guidelines.md):
a phase is marked done (`[x]`) only after review — human review in the
normal flow, agent review (per above) when auto-commit was instructed.
While work is underway, the phase carries the in-progress marker (`[·]`).
