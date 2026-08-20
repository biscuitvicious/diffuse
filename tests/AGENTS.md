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

## Building a new golden fixture

A new corpus is a **deliberate corpus change** — it requires human
sign-off (root `AGENTS.md`). The fixture lands in `tests/golden/` as the
single canonical copy shared by the Rust acceptance and the extension's
walk.

### 1. Pick the source repo and range

- **Snapshot corpora** (`<repo>-<date>`, e.g. `mirage-2025-08`): a small,
  curated diff exercising the paths under development. Rendered goldens
  (`.golden.txt`) are generated for these.
- **History corpora** (`<repo>-history-mid`, e.g. `diffuse-history-mid`):
  a whole-evolution stress corpus — `HEAD~<midpoint>..HEAD` of a repo, with
  the midpoint capped at **250 commits** (past that the fixture grows past
  usefulness: multi-MB JSON, minutes-long walks). These are **wire-only**:
  they get `.wire.json` but no `.golden.txt` (the TS render walk is too
  slow) and are excluded from the extension's `CORPORA` list.

### 2. Build the fixture JSON

Schema — `tests/golden/<name>.json`:

```json
{
  "diff": "<full unified git diff for the range>",
  "contents":    { "<path>": "<full file content at HEAD>", ... },
  "old_contents": { "<path>": "<full file content at range base>", ... }
}
```

- `contents` (post-patch) feeds the pipeline's context expansion and the
  on-disk coverage walk; `old_contents` (pre-patch) is what
  `apply_oracle` needs to mechanically prove `apply(diff, old) == new` —
  **always include it** (corpora without it silently skip the oracle per
  file).
- Extract both sides via `git show <rev>:<path>` for every file named in
  the diff's `diff --git` headers.
- **Drop binary files**: the engine is line-oriented — remove their diff
  sections and their contents (look for `Binary files … differ`); decode
  text with `errors='replace'` so no fixture build dies on odd bytes.

### 3. Probe before wiring

Run the pipeline + the cluster DP verifier over the raw fixture first
(e.g., verifying pipeline timing, cluster count, transition histogram,
and absence of projection failures). A corpus that trips invariant
failures or pathological runtime should be fixed or rejected **before**
it joins `CORPORA` — a frozen corpus that fails is a blocker, not a
signal.

### 4. Wire it in

- `tests/golden.rs` → `CORPORA`: add `("<name>", <min_files>)` — the
  floor is a sanity assertion that the corpus actually loaded; pick the
  real file count minus a little slack.
- `extension/pi/test/golden.test.ts` → `CORPORA`: **only** for snapshot
  corpora that get a rendered golden. History corpora stay out (the
  render walk is too slow).

### 5. Generate the expected outputs

- **wire.json** — `GEN_GOLDEN=1 cargo test --test golden` (rewrites every
  corpus's wire file; unchanged ones rewrite byte-identically, so it is
  safe). Or scope by temporarily commenting `CORPORA` down to the new
  name.
- **golden.txt** (snapshot corpora only) — `GEN_GOLDEN=1 GOLDEN_CORPUS=<name>
  node --experimental-transform-types extension/pi/test/golden.test.ts`
  (full-regen flag is fine here: a NEW golden has no prior expectation to
  surgically diff against).

### 6. Review & land

- The wire diff for a new corpus is pure addition — agent-review it for
  the usual quality signals (sane pairings, no garbage mods), then get
  the human's visual sign-off on the rendered scrolldown before landing.
- Update the corpora table above (and any stale counts in docs).

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
