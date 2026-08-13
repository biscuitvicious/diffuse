# diffuse — TODO & Future Roadmap

## Golden Farm Expansion (Paused for Phase 6)
- [ ] **Widen golden corpus by 1–2 orders of magnitude** (target: 50–100+ captured diff fixtures).
- [ ] **Multi-language diversity**: Ensure fixtures cover a broad spectrum of programming languages and formats:
  - Systems & compiled languages: Rust, C, C++, Go, Zig
  - Dynamic / Web languages: TypeScript, JavaScript, Python, Ruby, PHP
  - Structured data & markup: JSON, YAML, HTML, CSS, Markdown, SQL, Shell scripts
  - New / novel syntax profiles & edge cases (e.g. nested closures, macro expansions, complex diff boundaries)
- [ ] **Automated fixture capture tool**: A script to harvest diffs + old/new file contents directly from repository git histories.

## Arbitrary Commit & Golden Viewer (Phase 4.1)
- [x] Git commit range support (`/review A..B`, `/review A...B`, `/review <ref>`) in `pi-review`.
- [ ] Add direct golden file loading CLI option (e.g. `pi-review --golden <path/to/fixture.json>`).

## Phase 6 — Diff-Tree Refactoring Pipeline (Next Up)
- [ ] Transform `collapse` into a sequence of composable passes over a diff tree.
- [ ] Ensure every pass satisfies the **apply-oracle invariant**: `apply(diff_step, old_content) == new_content`.
