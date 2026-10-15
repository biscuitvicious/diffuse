# diffuse — known bugs

Confirmed defects awaiting a fix, grouped by subsystem. Checked items are resolved.

## 1. Execution context and CLI

- [ ] **Silent empty output when git fails to resolve the input range**: Running `diffuse -p <range>` (or any mode that shells out to `git diff`) with unknown revisions prints nothing — 4 bytes (`\x1b[0m`) on stdout, nothing on stderr — and exits 0. Repro: `diffuse -p f7fa5c3..9b06db9` outside the repository those commits belong to (e.g. submodule SHAs from the main repo); compare with `git diff f7fa5c3..9b06db9`, which correctly reports `fatal: ambiguous argument ... unknown revision or path not in the working tree`. Expected: surface git's stderr and exit non-zero instead of rendering an empty diff. (See **Use ariadne for CLI error reporting** in `TODO.md` §7.)
