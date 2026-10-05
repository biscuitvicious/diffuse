# TODO

## Markdown rendering (`.md` files and notes)

Inline Markdown renders in the terminal — to whatever extent is physically
possible (no font-size games for headers, just color + SGR prominence).
The SGR styles are pure visual aids — the control characters (`**`,
backticks, `~~`, …) stay in the content, because this is a diffing utility
and the control chars are part of the diffable text.

**Done**
- `#`-headings: marker muted, title in the accent + SGR bold
- `` `code` `` spans: dedicated code color (`cde`/`code` palette key,
  no SGR — the code color is the distinction)
- `**bold**`: accent + SGR bold · `*italic*`: muted + SGR italic
- `~~strikethrough~~`: muted + SGR strike
- `[label](url)` links: label + url underlined (SGR 4)
- Fenced code blocks (```` ```lang ````): the fence line muted, the code
  syntax-highlighted with the fence language's profile
  (rust/ts/js/py/sh/c/go/…); fence state tracks across rows, reset on
  file switch

**Backlog**
- `> ` blockquotes (muted, maybe with a leading bar)
- Lists — `- ` / `* ` bullets and `1. ` ordered items (marker + indent
  alignment)
- Tables (pipe-aligned columns)
- Images `![alt](url)` (alt text styled, url muted)
- Horizontal rules `---` / `___` / `***` (muted)
- Nested / multi-line emphasis in notes (the inline parser is top-level
  spans only)
- The `.md` mod rows (word-diff) could use the fence-aware token stream
  too — currently only the context/pure rows do

## Rendering polish

**Done**
- `~` rows: added subwords in the added green, the line in the grey mod
  tint (the `!` in `if !fingerprint.is_valid()` reads as added content)
- `return` pairs with `return` — `"return " (- expr) (+ expr) ";"` (the
  old `return self.results…` matches the new `return None;`, not a pop)
- The artifact filter drops a del span only when it is a fragment among
  other del content — a whole-line del nested in a growth (`None` →
  `return None;`) keeps its strike
- Moved rows subword their indentation change (8 → 12 spaces struck/
  filled at the line start — a semantic move, not an invisible ≈)
- Reflow pairing: when several old lines merge onto one new line
  (`self.resolved_layers` / `.iter()` / `.any(…)` → the one-liner
  `self.layers.iter().any(…)`), the FIRST del carries the meaningful
  change (`resolved_`); the later dels' content survives verbatim inside
  the add — pure reflows, dropped (a non-first del whose text is a
  substring of the add never steals the pair)
- The reflow-common merge: the reflowed dels' content reads as common in
  the mod, not as added — `self.~~resolved_~~layers.iter().any(…)`,
  and the `let up` one-liner's `?;` (the reflowed `.find(…)` tail) reads
  common instead of added
- The LCS drops a del token with no counterpart later in the add
  (`resolved` in `resolved_layers` → `layers`) and a del punctuation
  against a word — the meaningful `self.` + `layers` run survives, the
  coincidental `_`-of-`sub_path` tie-break does not steal it
- The trace wobble: `"Found in lower layer"` → `"Found in layer below
  the top upper"` renders `layer"layer` no more — the `"` is a
  tokenizer delimiter (the del's closing quote must not glue onto
  `layer`), the artifact filter drops a multi-span del fragment only
  when its text sits in the add AT THE SAME POSITION (`..]` → `..].`;
  `lower` vs `?next_lower,` is a real replacement, not absorption),
  and a del space yields to the del's NEXT token pairing with the
  add's word (the old `layer` matches the new `layer`)
- Line numbers pad to 4 columns, right-aligned
- The in-component note editor (m → status-line prompt, plain enter =
  line break, ctrl+enter saves, esc cancels) — no invisible pi editor
  under the overlay

**Backlog**
- (empty — the golden fixture's old-content reconstruction is fixed below)

**Done**
- The golden fixture's diff is now captured with `-U3` context (was
  `-U0` — zero context, so the reconstruction could never see the
  fn's closing `}` after a del run). The reconstruction now recovers
  the brace-anchor's old-side line, and the golden exercises the
  `None` ↔ `Some(result)` tail pairing (walk 191) exactly like the
  real review — no longer pinning the word-similarity `None` ↔
  `return None;` fallback.
