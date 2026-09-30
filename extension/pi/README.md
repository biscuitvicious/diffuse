# pi-review

Interactive git-diff review for [pi](https://github.com/earendil-works/pi-mono).

`/review [base] [--staged] [--] [path...]` opens the working-tree diff in a
full-screen TUI: wrapped hunks with syntax highlighting, word-level change
marks, persistent margin notes, and hunk-level git staging — all from the
terminal (phone-friendly keys, touch/wheel scrolling).

## Install

Bundle the engine and sync the extension into your user-level pi
extensions (`scripts/install.sh` copies `review.ts` + the bundled
`bin/diffuse`, so the installed copy finds its engine with no env):

```sh
./scripts/build-bin.sh      # bundle the release binary into bin/
./scripts/install.sh        # copy review.ts + bin/diffuse to ~/.pi/agent/extensions/
```

Then `/reload` in pi and run `/review` in any git work tree.

## Keys

```
 j/k move (visual lines)   g/G top/bottom (N+G: jump to line N)
 n/p next/previous hunk    tab file        v select range
 m note on selection       d delete note   o open editor at cursor
 e/E expand ctx ▴/▾        c/C collapse    a full-ctx · A all-full
 r/R reset ctx             w word-diff     t syntax
 h/l scroll ±10 cols       ^ to start      $ to end
 s/S stage hunk/file       u/U unstage     x/X discard (safety-stashed)
 ? key-binding popup       q/esc quit
 wheel / touch-drag        scroll (needs terminal mouse forwarding)
```

## What it does

- **Diff browser**: hunks with expandable context (default 4 lines), lines
  wrapped at the viewport (syntax colors continue across the wrap), the
  changed words filled in a stronger shade, and removed words struck through
  inline at their position. Infinite scroll across all files — selection is
  bounded to one file.
- **Notes**: attach a note to any line range (crossing hunks); they persist
  in `~/.pi/review/notes.json` (keyed by repo) and render inline on every
  launch. The cursor resumes at the same file/line when the content still
  matches.
- **Deep git**: stage / unstage / discard the current hunk or whole file
  (`git apply --cached`, reverse apply); discards are safety-stashed
  (`git stash create`). Base selection: worktree vs HEAD (default),
  `--staged` / `--cached`, refs, ranges, path filters; untracked files are reviewed too.
- **Editor**: `o` drops into your editor at the cursor line (range
  pre-selected in vim). Per-binary invocation table (vim family, positional
  editors, `--goto` editors, detached GUI editors).

## Config (env)

| Var | Meaning | Default |
|-----|---------|---------|
| `REVIEW_CONTEXT` | context lines around each hunk | `4` |
| `REVIEW_EDITOR` | editor binary | `$VISUAL`/`$EDITOR` → `vim` |
| `REVIEW_EDITOR_ARGS` | argv template override: `{bin} {file} {line} {start} {end} {count}` | — |
| `REVIEW_EDITOR_RANGE_ARGS` | argv template when a range is selected | — |
| `REVIEW_DIFFUSE_BIN` | the diffuse binary path (the extension spawns the bundled `bin/diffuse` by default) | the bundle next to `review.ts` |

## Development

```sh
npm install     # pulls @earendil-works/pi-tui for the tests
npm test        # unit suite (parser, wrap, syntax, merge, movement, git)
npm run check   # syntax-check the extension
```

The suite runs with `node --experimental-transform-types` (no build step).
