# pi-review

> **Home**: this extension lives inside the diffuse workspace
> (`extension/pi/` at the diffuse repo root — the extensions home, one
> subdir per agent) — the engine and its extension change together.
>
> **Backend**: the extension is a **thin client** for the native Rust binary.
> `/review` stops the pi TUI, resolves the git workspace and arguments, and
> spawns the bundled `diffuse` binary (`bin/diffuse`) in interactive TUI mode
> with `stdio: "inherit"`. On exit, it restarts the pi TUI and notifies note counts.

Interactive git-diff review for [pi](https://github.com/earendil-works/pi-mono).
`/review` opens the working-tree diff in the native diffuse TUI: syntax-highlighted
word-level diffs, persistent notes, hunk/file staging, discard checkpoints, editor
jump, and interactive fuzzy file finder.

## Layout

```
review.ts              # thin client command registration & diffuse binary launcher
index.ts               # extension entrypoint
src/backend.ts         # binary path resolver & arg parser
src/persistence.ts     # notes database reader for summary notification
test/smoke.test.ts     # smoke test verifying binary execution & args
scripts/build-bin.sh   # builds & bundles bin/diffuse
scripts/install.sh     # installs the extension into ~/.pi/agent/extensions/
```

## Architecture

1. **Thin client wrapper (`review.ts`)**:
   - Validates TUI mode.
   - Runs `git rev-parse --show-toplevel` and `--show-prefix`.
   - Parses flags and file arguments via `parseReviewArgs`.
   - Stops pi TUI (`ctx.tui.stop()`).
   - Spawns `bin/diffuse` (`stdio: "inherit"`, `cwd: repoRoot`).
   - Awaits exit, restarts pi TUI (`ctx.tui.start()`, `ctx.tui.requestRender(true)`).
   - Reads `~/.pi/review/notes.json` to display summary notification.

2. **Native Rust Binary (`diffuse`)**:
   - Implements the complete engine, renderer, event loop, notes persistence, and git actions natively in Rust.
