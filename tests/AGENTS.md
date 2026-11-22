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
> - **Validated Goldens (`theme-scrollbar-palette`, `opt-4-manual-scanner-prefilter`, `container-compatibility`, `scrollbar-grab-drag`)**: Fully hand-reviewed and verified as a clean rendering baseline. Serves as a strong signal against unintended changes and regressions, while allowing room for deliberate refinements, visual preferences, and demonstrable improvements.
> - **Unvalidated / Provisional Goldens (all others)**: Captured from earlier engine runs and not yet fully audited for ideal rendering quality. They guard against unintended regressions, but carry lower inertia when algorithmic improvements produce demonstrably cleaner, strictly better diffs.

| corpus | rust walk | render golden | status | notes |
|---|---|---|---|---|
| `theme-scrollbar-palette` | ✅ | ✅ | **Validated** | Fully hand-reviewed clean rendering baseline; strong signal against changes |
| `theme-light-toggle` | ✅ | ✅ | Provisional | Light theme, terminal luminance, and import alignment fixture (pending full review) |
| `row-groups-contrast` | ✅ | ✅ | Provisional | Row groups, dark theme context contrast & goto buffer navigation (pending full review) |
| `rust-lifetime-char` | ✅ | ✅ | Provisional | Rust lifetimes distinction and language-specific char literal parsing (pending full review) |
| `diffuse-2026-08` | ✅ | ✅ | Provisional | The engine diffing its own repo (pending full review) |
| `opt-1-cache-regex-window-slice` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-2-cache-markdown-regex` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-3-cache-lang-profile` | ✅ | ✅ | Provisional | Perf commit captured as fixture (pending full review) |
| `opt-4-manual-scanner-prefilter` | ✅ | ✅ | **Validated** | Fully hand-reviewed clean rendering baseline (reflow atomicity & delimiter alignment); strong signal against changes |
| `diffuse-history-mid` | ✅ | ✅ | Provisional | Pinned 100-commit window, absolute ids (pending full review) |
| `untracked-files` | ✅ | ✅ | Provisional | Untracked additions & status representation (pending full review) |
| `review-notes` | ✅ | ✅ | Provisional | Review notes anchoring, commit scoping isolation, and callouts (pending full review) |
| `ansi-optimizer` | ✅ | ✅ | Provisional | SGR transition collapsing and terminal escape sequence optimization (pending full review) |
| `fn-return-tail` | ✅ | ✅ | Provisional | Return type slot pairing and trailing delimiter boundary alignment (pending full review) |
| `osc11-query-gate` | ✅ | ✅ | Provisional | Terminal mode query gating and interactive reflow expansion (pending full review) |
| `linenum-u32` | ✅ | ✅ | Provisional | Primitive line numbers and non-zero constructor preconditions refactoring (pending full review) |
| `scrollbar-grab-drag` | ✅ | ✅ | **Validated** | Scrollbar grab drag, 12% min size, touch velocity scaling, and dispatch extraction |
| `viewport-scroll-boundaries` | ✅ | ✅ | Provisional | Viewport scroll boundaries and reflow expansion (pending full review) |
| `container-compatibility` | ✅ | ✅ | **Validated** | Fully hand-reviewed clean rendering baseline (container compatibility in Gotoh alignment and submodule commit pairing); strong signal against changes |
| `diffcoord-anchoring` | ✅ | ✅ | Provisional | Two-sided coordinate anchoring, multi-line ranges, rich markdown formatting, and unanchored source line preservation (pending full review) |

The terminal render walk in [`crates/diffuse/tests/golden.rs`](../crates/diffuse/tests/golden.rs) (`terminal_render_golden_walk`)
covers the corpora; it locks `.pager.render.dark.txt`, `.pager.render.light.txt`, and
`.pager.render.raw.txt` pager renders, alongside the `.tui.{init,top,bot}.{48x24,120x36,200x60}.render.{dark,light}.txt`
interactive reviewer frames. `tests/golden/` is the single canonical copy.

## Fixture file structure per corpus

All fixture files live under dedicated subdirectories `tests/golden/<corpus>/`
as plain, uncompressed files for optimal git visual tracking and diffing.
Frozen inputs live under `fixture/` as a sparse workspace-relative tree so the
harness can pass them straight to `--old-dir`/`--new-dir`; expected outputs
live under `expect/`:

1. **`fixture/diff.patch`** — the frozen unified diff patch.
2. **`meta.json`** (corpus root) — provenance only: the harvested `revision` (pinned to short commit ids or `<ref>^!` for single-commit ranges), plus `repo_key`/`rev_key` when the corpus carries review notes. It lists no file paths — the file set is derived from `fixture/diff.patch` itself (new side keyed by `path`, old side by the rename source), and side presence from the trees below: a path with `fixture/old/<path>` has old-side content, and one with `fixture/new/<path>` has new-side content.
3. **`fixture/old/<path>` / `fixture/new/<path>`** — individual uncompressed snapshots for old and new file contents, stored at their workspace-relative paths (added files appear only under `new/`, deleted files only under `old/`).
4. **`fixture/notes.json`** — harvested review notes, when the corpus carries them.

All reference outputs generated by the engine live under the dedicated `expect/` subdirectory:

5. **`expect/wire.jsonl`** — the frozen *wire-contract output* (token-stream JSON Lines) of the lib leg; both legs match it byte-for-byte. Changes whenever engine behavior **deliberately** changes.
6. **`expect/pager.render.dark.txt`** — the frozen *terminal rendered* pager output under default dark theme. It locks the hand-tuned visual preferences (strike/fill minimums, anchor pairings, move-block shape, reverse-stretch).
7. **`expect/pager.render.light.txt`** — the frozen *terminal rendered* pager output under light theme (`DiffuseTheme::light()`).
8. **`expect/pager.render.raw.txt`** — the frozen *terminal rendered* pager output under raw diff mode (`--raw`).
9. **`expect/tui.{init,top,bot}.{48x24,120x36,200x60}.render.{dark,light}.txt`** — the frozen *interactive TUI* screen renders across three states (`init` pre-stream, `top` post-stream initial view, and `bot` after navigating to bottom via `G`) and three viewport dimensions: phone (`48x24`), 720p desktop (`120x36`), and 1200p+ desktop (`200x60`).
10. **`expect/markdown.md`** — the frozen *GitHub-Flavored Markdown* output locking table metrics and code representation.

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
expression to short commit ids or `<ref>^!`, so the recorded revision can never
re-resolve to a different window).
When harvesting from the `diffuse` repository itself, `harvest` automatically
excludes `tests/golden` from the diff and creates lightweight git tags (`<name>`
for single commits, or `<name>-beg` and `<name>-end` for multi-commit ranges)
pointing to the harvested revisions. This provides direct reference parity and
enables rendering the golden changeset via `diffuse <name>^!`.

### 2. Harvest the corpus

```sh
harvest -o tests/golden/<name> <base>..<head>
```

The harvester writes `fixture/diff.patch`, the old/new side snapshots as
sparse trees under `fixture/old/` and `fixture/new/`, `meta.json` (the pinned
revision plus note keys) at the corpus root, and `fixture/notes.json`
when review notes are present.
which creates a lightweight git tag at the harvested commit in the source
repository (named after the corpus directory, or a custom name if specified
via `--tag <name>`). This ensures immediate visual verification and review
via `diffuse <name>^!`. The old-side snapshots are what `apply_oracle`
needs to mechanically prove `apply(diff, old) == new` — a range harvest always
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
  `(<mod_name>, "<name>", <num_files>)` — asserts that the corpus parses into
  the exact expected number of files.

### 5. Generate the expected outputs

- **wire/render/markdown** — `GEN_GOLDEN=1 cargo test --test golden`
  (writes every corpus's frozen outputs; unchanged ones rewrite
  byte-identically, so it is safe). The individual flags `UPDATE_WIRE`,
  `UPDATE_RENDER` (or `UPDATE_PAGER` / `UPDATE_TUI`), and `UPDATE_MARKDOWN`
  scope a single output type.

### 6. Review & land

- The wire diff for a new corpus is pure addition — agent-review it for
  the usual quality signals (sane pairings, no garbage mods), then get
  the human's visual sign-off on the rendered scrolldown before landing.
- Update the corpora table above (and any stale counts in docs).

## Regeneration mechanics

- **wire/render/markdown** — run the rust acceptance with `GEN_GOLDEN=1`
  or the scoped flags (`UPDATE_WIRE=1`, `UPDATE_RENDER=1`, `UPDATE_PAGER=1`,
  `UPDATE_TUI=1`, `UPDATE_MARKDOWN=1` — e.g. `UPDATE_RENDER=1 cargo test --test golden`).
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
