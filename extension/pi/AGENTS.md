# pi-review

> **Home**: this extension lives inside the diffuse workspace
> (`extension/pi/` at the diffuse repo root — the extensions home, one
> subdir per agent) — the engine and its renderer change together. The
> frozen corpus is shared: the golden walk reads `../../../tests/golden/`
> (never fork a second fixture/golden pair).
>
> **Backend**: the extension runs the **rust backend by default** — it
> spawns the diffuse binary BUNDLED with it (`bin/diffuse`, next to
> review.ts) and renders its token-stream JSON; the byte-identical golden
> gate passed. `REVIEW_BACKEND=ts` keeps the built-in TS engine as the
> fallback until the manual review signs off.

Interactive git-diff review for [pi](https://github.com/earendil-works/pi-mono).
`/review` opens the working-tree diff in a full-screen TUI: wrapped hunks with
syntax highlighting, word-level change marks, persistent margin notes, and
hunk-level git staging.

## Layout

```
review.ts              # the entire extension (single file — pi loads it as-is)
test/review.test.ts    # unit suite (node --experimental-transform-types, no build)
scripts/check.sh       # syntax-check the extension (parse only)
scripts/test.sh        # run the unit suite
```

The extension is deliberately a **single file**: pi auto-discovers
`~/.pi/agent/extensions/*.ts`, and keeping the whole tool in one file makes
installation a symlink. New code should not split the file without a strong
reason.

## Architecture (bottom-up)

- **Pure functions** (module scope, exported via `reviewInternals` for tests):
  diff parsing (`parseDiff`), context allocation (`allocateContext` — the
  cooperative two-pass so expanded hunks never overlap), move detection
  (`markMoves` — exact-content del/add pairs), modified-pair collapse
  (`collapseHunkRows`), the word-diff LCS (`wordSpans`/`tokenizeLine`), the
  syntax tokenizer (`classifyTokens`), note persistence, editor invocation
  (the per-binary pattern table), resume-position resolution.
- **`ReviewComponent`** — the pi TUI component: input handling, the merged
  flat-row model across files, the visual-space viewport, rendering.
- **The command handler** — git plumbing, base selection, the full-screen
  overlay hosting, the `?` popup.

## Hard-won invariants (do not break)

1. **Visual-space movement.** `rowIdx` is the flat row (notes/editor/hunk
   nav/`NG` operate on it); `visLine` is the wrapped sub-line. `j`/`k`/wheel/
   drag step by *visual* lines; the cursor highlight follows the single
   visual line.
2. **Geometry/width lockstep.** `segCountOf` (used for the scroll window and
   movement) and `rowSegments` (the render) must compute segment counts with
   the **same width** — historically `process.stdout.columns` in one and the
   render `width` in the other caused an out-of-bounds segment crash. The
   render passes its `width` to `visualGeometry`; movement uses the terminal
   width.
3. **Token-stream slicing, not re-tokenization.** A line is tokenized once;
   wrap segments slice the token stream. Re-tokenizing per segment resets
   syntax colors at the wrap boundary.
4. **`displayText` mirrors `mergedTokens` exactly.** The merged inline view
   (common words + struck removed + filled added) must insert the same del
   spans at the same offsets in both the text the segments slice and the
   token stream — otherwise the stream positions drift and content is sliced
   off.
5. **`delSpansToShow` filters alignment artifacts.** A del span whose text
   exists verbatim in the add (e.g. a trailing `..]` absorbed into `..].`) is
   an LCS artifact, not a removal — skipping it prevents duplicated struck
   text. Both `displayText` and `mergedTokens` use it.
6. **Unified collapse order = NEW-side positions.** `collapseHunkRows`
   sorts the mods and adds by their new line number (a replacement block
   that precedes the paired line — new lines 134-139 inserted above the
   modified signature at 140 — leads with the additions), and each
   genuinely removed line anchors right after the mod whose old position
   preceded it (old 109 renders after the mod that replaced old 108).
   Sections separated by empty lines stay together; old-line-numbered
   removed rows never interrupt the new-file sequence.
7. **Tail absorption is position-guarded and blank-closing.** A mod's tail
   absorbs only the pure-add lines whose new position FOLLOWS the mod's own
   new line — and a trailing blank joins the section too (the block reads as
   a unit: the closing brace plus the air after it), after which the section
   ends and the following add is a separate `+` row. When the paired add is
   the last of a replacement block (old `None` ↔ new `return None;`), the
   block's earlier lines are `+` rows before the mod — never tail lines
   after it. The tail rows render with their own line numbers (context-style
   prefix, no marker) on the mod's shade.
8. **Diff shifts are content-neutral (both directions).** A hunk's
   coverage may shift DOWN — absorbing the closing `}` + blank of an added
   block that the diff left as below-context — only when the lines released
   ABOVE (the mod's absorbed section: the previous function's closing +
   blank) EXACTLY match the absorbed lines. It may shift UP — absorbing the
   comment lines above the hunk (the block reads comment + change as one
   unit) — only when the released trailing rows EXACTLY match and the
   absorbed leads with a comment. A shift must never alter the change; a
   blank-only or brace-led trade is not a flow win.
15. **Reflow-absorption corpus is display-aware.** An add whose content
   exists verbatim in a del is a "pure reflow" (dropped, invisible) only
   when the del it matches is otherwise invisible. Moved adds check the
   full raw-del corpus (a local same-position reshuffle is noise); unmoved
   adds check only the dels that are actually invisible — unmoved,
   unpaired, AND not meaningful (a `-` row): an add matching a shown del
   is real content (the seven lines a one-line import was split into all
   survive).
9. **Fragment pairs are rejected.** A del↔add pair whose add's content is
   ≥80% matched inside the del with overall similarity < 0.5 is a
   fragment of a split line (the one-line `use` statement became a block)
   — pairing it renders the whole old line struck against one arbitrary
   new fragment. The direction matters: a growth pair (`None` →
   `return None;`) matches only part of the add and pairs normally.
10. **Wholesale pairs render as the `-`/`+` baseline.** A pair whose
   spans are garble-shaped — the mismatched fragment counts or a standalone
   removal landing mid-word (the LCS aligned around it: `fro~~upper~~zen`,
   `b~~ layer's markers —~~ottom`) — reads badly inline: a `~` row must
   never carry an embedded "newline" (the whole-old-struck + whole-new-
   filled blocks look like one). The collapse un-pairs such pairs into the
   plain `-` line + `+` line. The decision is the shared `wholesalePair`
   (artifact-filtered spans + the del↔add pairing), used by both the
   collapse and the component's `isWholesaleMod` — they never drift apart.
   Long lines still wrap freely — only the embedded-block render is banned.
11. **Per-token background re-application.** The tokenizer's `theme.fg`
   resets (`\x1b[0m`) kill any background applied once before it; backgrounds
   (line tint, word fill, selection lift) are re-applied after every token.
12. **Cursor lift = one shade.** The selected visual line is one 256-color
   step lighter than its row (28/88 for diff rows, 237 for context), applied
   to the prefix too; the themed `toolPendingBg` stripe is gone.
13. **Row types are one theme table.** The four diff row types —
   `ROW_STYLES` at the top of the file: `add`/`del`/`mod`/`mov` — each
   with its `icon` (the prefix character; the moved "double wave" `≈` is
   swappable for terminals that cannot render it), `tint` (the neutral
   background index), `cursor` (the selected lift), `fill`/`fillCursor`
   (the mod rows' word fill), and `fg` (the pi theme key for the row's own
   text — `toolDiffRemoved`, `toolDiffContext`, …). The renderer resolves
   every tint and icon through the table — no hardcoded `\x1b[48;5;N` or
   literal `+`/`-`/`~`/`≈` in the row logic. Defaults (GitHub-dark style):
   `add` 22/28, `del` 52/88, `mod` 236/237, `mov` 236/237; word fill
   28/34. The green is reserved for the pure `+` rows; every `~` mod is
   the grey — the struck and filled words are the only color. Keep it
   subtle — the syntax colors carry the reading load.
14. **Persist everything through `~/.pi/review/`.** Notes in `notes.json`
    (keyed by repo), resume position in `state.json`; restore only when the
    file is still in the diff and the line content matches (else fall back to
    the same file's first row).

## Testing

- `npm test` / `./scripts/test.sh` — the suite (`node
  --experimental-transform-types`, no build step): the unit tests AND the
  golden compare. The unit tests import `../review.ts` (the REPO's copy, not
  `~/.pi/agent/extensions/review.ts`) — a per-commit verification needs no
  extension sync, and the committed state is what gets tested. Construct fixtures via the pure functions (`parseDiff` +
  content + `markMoves` + `collapseHunkRows`) and exercise
  `ReviewComponent` with stub `tui`/`theme`/`done` callbacks. Stub themes
  must return the *text* from `bg` (an empty-string bg erases the cursor
  row's content).
- `npm run check` / `./scripts/check.sh` — parse-only syntax check.
- The suite runs under `node --experimental-transform-types`, which strips
  types but does **not** type-check — a nonexistent field assignment compiles
  silently (this bit us: `f.aboveCtx` set on the file instead of the hunks).
  Watch for that class of bug.

## Golden file discipline

The golden (`test/golden/mirage-2025-08.golden.txt`) is the locked "ideal"
render of the frozen fixture corpus. It is **ANSI-stripped** — text and
geometry only — so a color/shade change never touches it; a golden mismatch
means the merge/collapse/order/context logic changed, which is exactly what
must be reviewed.

Updates are **surgical, never a blanket regeneration**:

1. Run the golden test (plain, no env): on a mismatch it reports the exact
   failing rows (the golden file's 0-based line numbers).
2. Review the report: the changed rows must be ONLY the region the change
   was supposed to touch.
3. Re-run with the explicit expectation naming exactly those rows (ranges
   work: `"734-740, 812"`):
   ```
   UPDATE_GOLDEN=1 GOLDEN_EXPECT="734,735,736" node --experimental-transform-types test/golden.test.ts
   ```
   (or `GOLDEN_EXPECT="…" ./scripts/golden.sh`). The update verifies the
   changed set EQUALS the expected set:
   - any changed row **outside** it → `GOLDEN_EXPECT VIOLATION`, exit 1,
     nothing written — a regression elsewhere in the corpus: fix the code,
     do not bake it in;
   - any expected row that did **not** change → stale expectation, exit 1.
4. `git diff test/golden/` must show exactly the expected rows — a small,
   reviewed hunk against the baseline commit.

## Commit policy

Do not commit without explicit instruction. Present the change summary and
ask; the user commits themselves.

## Style

- Follow the repo's API conventions: no `String` for paths/args where the
  OS matters (use `OsString`/`PathBuf`), owned forms unmarked, `_ref` for
  borrowed conveniences — see the mirage repo's AGENTS.md conventions.
- Keep the pure logic testable; new behavior gets a unit test before it
  ships.

## TypeScript conventions

Target the **most recent stable TypeScript**, with the erasable-syntax-only
rule: `node --experimental-transform-types` strips types but cannot strip
`enum` / `namespace` / parameter properties / legacy decorators — none of
those. The basic idiomatic conventions to use explicitly:

1. **`const` by default; `let` only when the binding is reassigned; never
   `var`.** New code defaults to `const`; a reassigned accumulator or loop
   counter is the only `let`.
2. **Types at the boundaries, inference inside.** Annotate the exported
   function signatures and the public API surface; let the local variables
   and the small helpers infer. No `any` in new code — the `as any` in the
   tests is confined to the `reviewInternals` seams.
3. **Strict null handling.** `?.` / `??` / explicit narrowing over the
   `&&`/`||` chains and the non-null assertions; a `!` carries a comment
   explaining why the value cannot be null there.
4. **Template literals over concatenation.** `` `${n} ~` `` — never
   `n + " ~"`. (Already the house style; keep it.)
5. **Arrow functions for callbacks; named `function` declarations at the
   module scope.** The module-level pure helpers are `function` (hoisted,
   greppable); the inline callbacks (`map`, `filter`, event handlers) are
   arrows.
6. **`for...of` and array methods over index loops** — unless the loop
   body needs the index for arithmetic (the LCS, the token walks: the
   exception, not the rule).
7. **Destructuring and default parameters.** `const { delPos, pair } = …`,
   `function f(ctx = defaultCtx())` — named extraction over repeated
   `obj.field`, defaults over positional-arg sprawl.
8. **Modern ESM; `import type` for type-only imports.** A symbol used only
   as a type comes through `import type` so the transform strips it
   cleanly.
