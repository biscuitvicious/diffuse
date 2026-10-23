# tests/ — integration & acceptance test guidelines

Project-wide conventions live in the root [`AGENTS.md`](../AGENTS.md); this
file covers the integration/acceptance side: the golden corpora, the fixture
file types, regeneration mechanics, and — most importantly — the review
discipline for any change in golden output.

## What lives here

- [`crates/diffuse/tests/golden.rs`](../crates/diffuse/tests/golden.rs) — the Rust golden acceptance: **two legs** over every frozen
  corpus, both asserting the wire-contract invariants and byte-identical
  output:
  1. **lib leg** — embed the engine, run the full pipeline directly.
  2. **bin leg** — spawn the built `diffuse` binary, feed it the same input,
     decode its JSON, run the SAME walk.

  The bin leg pins the wire contract ([`crates/diffuse-core/src/wire.rs`](../crates/diffuse-core/src/wire.rs)) to the engine: a JSON
  schema change breaks it even if engine semantics are unchanged.
- `golden/` — the frozen corpora (below).

## The golden corpora & validation status

> [!IMPORTANT]
> **Validated vs. Provisional Goldens**:
> - **Validated Goldens (`theme-scrollbar-palette`)**: Fully hand-reviewed and verified as a clean rendering baseline. Serves as a strict hard gate against regression.
> - **Unvalidated / Provisional Goldens (all others)**: Captured from earlier engine runs and not yet fully audited for ideal rendering quality. They guard against unintended regressions, but should be taken as **less of a hard gate** when algorithmic improvements produce demonstrably cleaner, strictly better diffs.

| corpus | rust walk | render golden | status | notes |
|---|---|---|---|---|
| `theme-scrollbar-palette` | ✅ | ✅ | **Validated** | Fully hand-reviewed clean rendering baseline; strict hard gate |
| `theme-light-toggle` | ✅ | ✅ | Provisional | Light theme, terminal luminance, and import alignment fixture (pending full review) |
| `row-groups-contrast` | ✅ | ✅ | Provisional | Row groups, dark theme context contrast & goto buffer navigation (pending full review) |
| `rust-lifetime-char` | ✅ | ✅ | Provisional | Rust lifetimes distinction and language-specific char literal parsing (pending full review) |
| `diffuse-2026-08` | ✅ | ✅ | Provisional | The engine diffing its own repo (pending full review) |
| `opt-1-cache-regex-window-slice` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-2-cache-markdown-regex` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-3-cache-lang-profile` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-4-manual-scanner-prefilter` | ✅ | ✅ | **Validated** | Fully hand-reviewed clean rendering baseline (reflow atomicity & delimiter alignment); strict hard gate |
| `diffuse-history-mid` | ✅ | ✅ | Provisional | Pinned 100-commit window, absolute ids (pending full review) |
| `untracked-files` | ✅ | ✅ | Provisional | Untracked additions & status representation (pending full review) |
| `review-notes` | ✅ | ✅ | Provisional | Review notes anchoring, commit scoping isolation, and callouts (pending full review) |
| `ansi-optimizer` | ✅ | ✅ | Provisional | SGR transition collapsing and terminal escape sequence optimization (pending full review) |
| `fn-return-tail` | ✅ | ✅ | Provisional | Return type slot pairing and trailing delimiter boundary alignment (pending full review) |
| `osc11-query-gate` | ✅ | ✅ | Provisional | Terminal mode query gating and interactive reflow expansion (pending full review) |
| `linenum-u32` | ✅ | ✅ | Provisional | Primitive line numbers and non-zero constructor preconditions refactoring (pending full review) |
| `scrollbar-grab-drag` | ✅ | ✅ | Provisional | Scrollbar grab drag, 12% min size, touch velocity scaling, and dispatch extraction (pending full review) |
| `viewport-scroll-boundaries` | ✅ | ✅ | Provisional | Viewport scroll boundaries and reflow expansion (pending full review) |

The terminal render walk in [`crates/diffuse/tests/golden.rs`](../crates/diffuse/tests/golden.rs) (`terminal_render_golden_walk`)
covers the corpora; it locks `.render.dark.txt`, `.render.light.txt`, and
`.render.raw.txt` golden renders. `tests/golden/` is the single canonical copy.

## Fixture file structure per corpus

All fixture files live under dedicated subdirectories `tests/golden/<corpus>/`
as plain, uncompressed files for optimal git visual tracking and diffing.
At the top level of each corpus directory, only the diff, metadata, and snapshot sources are kept:

1. **`diff.patch`** — the frozen unified diff patch.
2. **`meta.json`** — records the harvested `revision` (pinned to absolute full commit ids) and the list of file path to snapshot mappings (`files/*.old` and `files/*.new`).
3. **`files/<stem>.old.<ext>` / `files/<stem>.new.<ext>`** — individual uncompressed snapshots for old and new file contents (with path slashes `/` normalized to dashes `-` and the side marker preceding the original extension for automatic syntax highlighting and editor recognition).

All reference outputs generated by the engine live under the dedicated `outputs/` subdirectory:

4. **`outputs/wire.jsonl`** — the frozen *wire-contract output* (token-stream JSON Lines) of the lib leg; both legs match it byte-for-byte. Changes whenever engine behavior **deliberately** changes.
5. **`outputs/render.dark.txt`** — the frozen *terminal rendered* output under default dark theme. It locks the hand-tuned visual preferences (strike/fill minimums, anchor pairings, move-block shape, reverse-stretch).
6. **`outputs/render.light.txt`** — the frozen *terminal rendered* output under light theme (`DiffuseTheme::LIGHT`).
7. **`outputs/render.raw.txt`** — the frozen *terminal rendered* output under raw diff mode (`--raw`).
8. **`outputs/markdown.md`** — the frozen *GitHub-Flavored Markdown* output locking table metrics and code representation.

## Building a new golden fixture

A new corpus is a **deliberate corpus change** — it requires human
sign-off (root `AGENTS.md`). The fixture lands in `tests/golden/` as the
single canonical copy of the Rust acceptance.

### 1. Pick the source repo and range

- **Snapshot corpora** (`<repo>-<date>`, e.g. `diffuse-2026-08`): a small,
  curated diff exercising the paths under development.
- **History corpora** (`<repo>-history-mid`, e.g. `diffuse-history-mid`):
  a whole-evolution stress corpus — a fixed window of a repo's history
  (e.g. `HEAD~100..HEAD`), never so large the fixture stops being useful
  (multi-MB JSON, minutes-long walks).

Always harvest with **absolute ranges** (`harvest` resolves any
expression to full commit ids, so the recorded revision can never
re-resolve to a different window) and exclude the golden tree itself
(`--exclude tests/golden`) — a corpus must not ingest its own history.

### 2. Harvest the corpus

```sh
harvest --exclude tests/golden -o tests/golden/<name> <base>..<head>
```

The harvester writes `diff.patch`, the old/new side snapshots under
`files/`, and `meta.json` (the pinned revision plus the path→snapshot
mapping). The old-side snapshots are what `apply_oracle` needs to
mechanically prove `apply(diff, old) == new` — a range harvest always
carries them.

### 3. Probe before wiring

Run the pipeline + the cluster DP verifier over the raw fixture first
(e.g., verifying pipeline timing, cluster count, transition histogram,
and absence of projection failures). A corpus that trips invariant
failures or pathological runtime should be fixed or rejected **before**
it joins `corpus_tests!` — a frozen corpus that fails is a blocker, not a
signal.

### 4. Wire it in

- [`crates/diffuse/tests/golden.rs`](../crates/diffuse/tests/golden.rs) → `corpus_tests!`: add
  `(<mod_name>, "<name>", <min_files>, <has_render>, <has_oracle>)` — the
  floor is a sanity assertion that the corpus actually loaded; pick the
  real file count minus a little slack.

### 5. Generate the expected outputs

- **wire/render/markdown** — `GEN_GOLDEN=1 cargo test --test golden`
  (writes every corpus's frozen outputs; unchanged ones rewrite
  byte-identically, so it is safe). The individual flags `UPDATE_WIRE`,
  `UPDATE_RENDER`, `UPDATE_MARKDOWN` scope a single output type.

### 6. Review & land

- The wire diff for a new corpus is pure addition — agent-review it for
  the usual quality signals (sane pairings, no garbage mods), then get
  the human's visual sign-off on the rendered scrolldown before landing.
- Update the corpora table above (and any stale counts in docs).

## Regeneration mechanics

- **wire/render/markdown** — run the rust acceptance with `GEN_GOLDEN=1`
  or the scoped flags (`UPDATE_WIRE=1`, `UPDATE_RENDER=1`,
  `UPDATE_MARKDOWN=1` — e.g. `UPDATE_WIRE=1 cargo test --test golden`).
  Without a flag, a mismatch prints a structured per-row diff and fails.
- **Never regenerate any golden to make a test pass.** A failing golden
  means the engine deviates from the reviewed ideal; find the deviation.

## Review discipline for golden changes — the signoff gate

**Every change in golden output (`*.wire.jsonl`, `*.render.txt`,
`*.markdown.md`) requires BOTH agent review and human review.**

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
  obvious visual defects (mispaired lines, garbled runs, nonsense hunks) that
  mechanical checks cannot detect.
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
