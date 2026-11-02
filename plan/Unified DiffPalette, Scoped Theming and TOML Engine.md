# Plan — Unified DiffPalette, Scoped Theming and TOML Engine

## Global TODO
- [x] Phase 1 — Foundational Surface, StyledColor & Primitive Palette Modeling (`SurfaceColors`, `StyledColor`, `GutterColors`, `SyntaxPalette`, `PrimitivePalette`)
- [ ] Phase 2 — Scope Axis, DiffKind Indexing & Scoped Container Architecture (`Scope`, `DiffKind`, `DiffPalette`, `ScopePalette`, `ThemePalette`)
- [ ] Phase 3 — TOML Theme Schema, Serde Deserializer & Cascading Engine with `inherits` & `$variable` Reference (`crates/diffuse-render/src/theme/toml/`)
- [ ] Phase 4 — Bundled Canonical Dark and Light TOML Assets with Mode Pairing (`themes/dark.toml`, `themes/light.toml`)
- [ ] Phase 5 — Render Pipeline & Row Styler Migration (`crates/diffuse-render/src/`)
- [ ] Phase 6 — Interactive TUI Grid, Header, Gutter & Mode Toggle Migration (`crates/diffuse-tui/src/`)
- [ ] Phase 7 — Dynamic User Theme Discovery, Configuration & CLI Integration (`crates/diffuse/src/`)
- [ ] Phase 8 — Frozen Golden Corpus Verification, Dual-Leg Sign-Off & Benchmarks

---

## Executive Summary

The diffuse theme palette currently represents diff colors, syntax tokens, and UI chrome across flat, ad-hoc struct fields in [`crates/diffuse-render/src/theme/palette/mod.rs`](../crates/diffuse-render/src/theme/palette/mod.rs). Colors are multiplied across repetitive naming permutations (`add_*`, `del_*`, `mod_*`, `mov_*`, `ctx_*`, `*_in_hunk`, `*_out_hunk`, `*_cursor_bg`, `*_selected`, `*_current`, `*_rest`), leading to over 140 individual fields with hand-coded match statements and zero support for external configuration files.

This plan executes a comprehensive re-architecture inspired by canonical, state-of-the-art theming systems (Helix, Zed, Alacritty):
1. **Structural Scope Containment Axis**: Introduces `enum Scope { None, File, Hunk, Row }`, formalizing the visual hierarchy of diff review ($\text{None (Global)} \supset \text{File} \supset \text{Hunk} \supset \text{Row}$).
2. **Unified Surface Abstraction & Typography**: Encapsulates 3-layer visual composite styling into `SurfaceColors` (`bg`, `fill`, `fg`) and supports optional text modifiers (`bold`, `italic`, `dim`, `underline`) on `StyledColor`.
3. **First-Class DiffKind & Array Indexing**: Defines `enum DiffKind { Ctx, Add, Del, Mod, MovSrc, MovTgt }`, unifying all diff kinds under an identical `DiffPalette` and enabling $O(1)$ dual-indexed access: `palette[scope][diff_kind]`.
4. **Primitive Swatches with `$variable` Expansion**: Introduces `PrimitivePalette` defining foundational color swatches (`red`, `green`, `bg_dark`, etc.) referenced symbolically in TOML (e.g. `fg = "$green"`).
5. **Declarative TOML Engine with `inherits` & Theme Pairing**: Supports standalone dark/light theme files, base theme extension (`inherits = "dark"`), and twin mode pairing (`light_pair = "catppuccin-latte"`).
6. **Zero-Allocation Runtime Discipline**: All TOML deserialization and cascading inheritance resolve at load time into a flat, `Copy`, array-indexed `ThemePalette` guaranteeing $O(1)$ pointer offsets with zero heap allocations during terminal rendering.

---

## Table of Contents
1. [Phase 1 — Foundational Surface, StyledColor & Primitive Palette Modeling](#1-phase-1--foundational-surface-styledcolor--primitive-palette-modeling)
2. [Phase 2 — Scope Axis, DiffKind Indexing & Scoped Container Architecture](#2-phase-2--scope-axis-diffkind-indexing--scoped-container-architecture)
3. [Phase 3 — TOML Theme Schema, Serde Deserializer & Cascading Engine](#3-phase-3--toml-theme-schema-serde-deserializer--cascading-engine)
4. [Phase 4 — Bundled Canonical Dark and Light TOML Assets with Mode Pairing](#4-phase-4--bundled-canonical-dark-and-light-toml-assets-with-mode-pairing)
5. [Phase 5 — Render Pipeline & Row Styler Migration](#5-phase-5--render-pipeline--row-styler-migration)
6. [Phase 6 — Interactive TUI Grid, Header, Gutter & Mode Toggle Migration](#6-phase-6--interactive-tui-grid-header-gutter--mode-toggle-migration)
7. [Phase 7 — Dynamic User Theme Discovery, Configuration & CLI Integration](#7-phase-7--dynamic-user-theme-discovery-configuration--cli-integration)
8. [Phase 8 — Frozen Golden Corpus Verification, Dual-Leg Sign-Off & Benchmarks](#8-phase-8--frozen-golden-corpus-verification-dual-leg-sign-off--benchmarks)

---

## 1. Phase 1 — Foundational Surface, StyledColor & Primitive Palette Modeling

### Architectural Principle
Separate concrete color swatches from semantic roles, unify 3-layer visual representations (`bg`, optional `fill`, `fg`) into `SurfaceColors`, and support text modifiers (`bold`, `italic`, `dim`) directly on styled tokens.

### Key Types and Modules
- **Location**: `crates/diffuse-render/src/theme/surface.rs`, `crates/diffuse-render/src/theme/primitive.rs`, `crates/diffuse-render/src/theme/syntax.rs`
- **`StyledColor`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct StyledColor {
      pub color: Color,
      pub bold: bool,
      pub italic: bool,
      pub dim: bool,
      pub underline: bool,
  }
  ```
- **`SurfaceColors`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct SurfaceColors {
      /// Layer 1: Base surface canvas background.
      pub bg: Color,
      /// Layer 2: Optional interior emphasis fill (e.g. word diff highlight,
      /// pill/badge background, or selection highlight).
      pub fill: Option<Color>,
      /// Layer 3: Foreground text or primary content.
      pub fg: Color,
  }
  ```
  Provides helper constructors `new(bg, fg)`, `with_fill(bg, fill, fg)`, and style accessors:
  - `bg_style()`: `Style::new().bg(self.bg)`
  - `fill_or_bg_style()`: `Style::new().bg(self.fill.unwrap_or(self.bg))`
  - `text_style()`: `Style::new().bg(self.bg).fg(self.fg)`
- **`GutterColors`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct GutterColors {
      pub num: StyledColor,
      pub icon: StyledColor,
  }
  ```
- **`SyntaxPalette`**:
  Consolidates programming language and markdown token classifications (`keyword`, `typ`, `function`, `number`, `literal`, `string`, `char_literal`, `comment`, `operator`, `text`, `bold`, `italic`, `strike`, `link`, `url`, `delim`, `rule`, `code`, `table_border`, `table_header`, `link_border`, `quote_border`, `fence_border`).
- **`PrimitivePalette`**:
  Foundation swatches (`base_bg`, `base_fg`, `red`, `green`, `yellow`, `orange`, `blue`, `purple`, `cyan`, `gray`, `dark_red`, `dark_green`) used for baseline fallbacks and `$variable` resolution.

### Acceptance Criteria
- Unit tests validating `SurfaceColors` and `StyledColor` style generator methods against `yansi::Style`.
- 100% branch coverage on color conversion and helper methods.

---

## 2. Phase 2 — Scope Axis, DiffKind Indexing & Scoped Container Architecture

### Architectural Principle
Establish two orthogonal axes:
1. **Scope Axis**: Visual containment ($\text{Scope::None} \subset \text{Scope::File} \subset \text{Scope::Hunk} \subset \text{Scope::Row}$).
2. **DiffKind Axis**: Semantic diff variants including relocation side asymmetry (`MovSrc` vs `MovTgt`).

### Key Types and Modules
- **Location**: `crates/diffuse-render/src/theme/scope.rs`, `crates/diffuse-render/src/theme/diff.rs`, `crates/diffuse-render/src/theme/palette/mod.rs`
- **`Scope`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
  #[repr(u8)]
  pub enum Scope {
      #[default]
      None = 0,   // Global baseline scope (outside active file / rest)
      File = 1,   // Inside active file, but outside active hunk
      Hunk = 2,   // Inside active hunk, but not the active cursor row
      Row = 3,    // Active cursor row or focused header/divider
  }
  ```
- **`DiffKind`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
  #[repr(u8)]
  pub enum DiffKind {
      #[default]
      Ctx = 0,
      Add = 1,
      Del = 2,
      Mod = 3,
      MovSrc = 4,
      MovTgt = 5,
  }

  impl DiffKind {
      pub const COUNT: usize = 6;

      #[inline]
      pub fn from_row(kind: RowKind, mov_side: Option<MovSide>) -> Self {
          match (kind, mov_side) {
              (RowKind::Add, _) => Self::Add,
              (RowKind::Del, _) => Self::Del,
              (RowKind::Mod, _) => Self::Mod,
              (RowKind::Mov, Some(MovSide::Src)) => Self::MovSrc,
              (RowKind::Mov, _) => Self::MovTgt,
              _ => Self::Ctx,
          }
      }
  }
  ```
- **`DiffPalette`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct DiffPalette {
      pub surface: SurfaceColors,
      pub gutter: GutterColors,
      pub syntax: SyntaxPalette,
  }
  ```
- **`ScopePalette`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct ScopePalette {
      pub file_header: SurfaceColors,
      pub hunk_divider: SurfaceColors,
      pub diffs: [DiffPalette; DiffKind::COUNT],
  }

  impl std::ops::Index<DiffKind> for ScopePalette {
      type Output = DiffPalette;
      #[inline]
      fn index(&self, kind: DiffKind) -> &Self::Output {
          &self.diffs[kind as usize]
      }
  }
  ```
- **`ThemePalette`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct ThemePalette {
      pub scopes: [ScopePalette; 4],
      pub ui: UiPalette,
  }

  impl ThemePalette {
      #[inline]
      pub fn scope(&self, scope: Scope) -> &ScopePalette {
          &self.scopes[scope as usize]
      }

      #[inline]
      pub fn diff(&self, scope: Scope, kind: DiffKind) -> &DiffPalette {
          &self.scopes[scope as usize][kind]
      }
  }
  ```

### Acceptance Criteria
- `palette[Scope::Row][DiffKind::Add].surface.bg` provides direct, zero-allocation, compile-time verified access.
- Submodule decomposition under `crates/diffuse-render/src/theme/` complies with repository code style.

---

## 3. Phase 3 — TOML Theme Schema, Serde Deserializer & Cascading Engine

### Architectural Principle
Declarative theme loading with clean inheritance, `$variable` expansion, and `inherits` extension:
1. **`$variable` Expansion**: Token values like `"$green"` expand against `[palette]`.
2. **`inherits` Directive**: Theme files can extend bundled or parent themes (`inherits = "dark"`), overlaying only specific customizations.
3. **Multi-Tier Cascade**:
   - Scope: `Row` $\to$ `Hunk` $\to$ `File` $\to$ `None`.
   - Diff: `add`, `del`, `mod`, `mov_src`, `mov_tgt` inherit missing syntax tokens from `ctx`.
   - Mov: `[none.mov]` propagates to both `MovSrc` and `MovTgt` unless explicitly overridden.
   - Primitive: Missing semantic roles fall back to foundational swatches.

### Key Types and Modules
- **Location**: `crates/diffuse-render/src/theme/toml/`
  - `model.rs`: Intermediate Serde deserialization structs supporting hex strings (`"#RRGGBB"`), ANSI 256 numbers (`42`), and `$name` references.
  - `cascade.rs`: Expansion algorithm compiling sparse TOML definitions into pre-resolved `ThemePalette`.
  - `parse.rs`: High-level parser `parse_theme_str(toml: &str) -> Result<ThemePalette, ThemeParseError>`.
- **Dependencies**: Add `toml = { workspace = true }` to workspace dependencies and `crates/diffuse-render/Cargo.toml`.

### Acceptance Criteria
- Unit tests verifying:
  - `$variable` substitution against `[palette]` entries.
  - Inherited theme overlays (`inherits = "..."`).
  - Partial theme parsing where missing tokens successfully cascade across scope and diff dimensions.
  - Error reporting on unresolved `$variables`, cyclic inheritance, or invalid hex formats.

---

## 4. Phase 4 — Bundled Canonical Dark and Light TOML Assets with Mode Pairing

### Architectural Principle
Replace hardcoded static Rust arrays with embedded TOML assets parsed at compile-time or static initialization, guaranteeing byte-for-byte visual identity with current default palettes and clean dark/light pairing.

### Key Types and Modules
- **Location**: `crates/diffuse-render/themes/dark.toml`, `crates/diffuse-render/themes/light.toml`
- **Metadata & Mode Pairing**:
  - `dark.toml`: `name = "default"`, `mode = "dark"`, `light_pair = "light"`.
  - `light.toml`: `name = "default"`, `mode = "light"`, `dark_pair = "dark"`.
- **Embedding**:
  - `const DARK_THEME_TOML: &str = include_str!("../themes/dark.toml");`
  - `const LIGHT_THEME_TOML: &str = include_str!("../themes/light.toml");`
- **Verification**:
  - Author equivalence unit tests asserting that `ThemePalette::default_palette()` and `ThemePalette::light_palette()` loaded from TOML match the exact RGB/ANSI color values previously specified in `dark.rs` and `light.rs`.

### Acceptance Criteria
- Equivalence assertions pass with zero deviations.
- `dark.rs` and `light.rs` are replaced or re-implemented via bundled TOML parsing.

---

## 5. Phase 5 — Render Pipeline & Row Styler Migration

### Architectural Principle
Migrate diff row renderers, gutters, and markdown formatters to pass and consume `Scope` and `DiffPalette`.

### Key Types and Modules
- **Location**: `crates/diffuse-render/src/diff/`, `crates/diffuse-render/src/row/`, `crates/diffuse-render/src/markdown/`
- **`RowStyle` Refactoring**:
  Update `RowStyle` constructors to accept `&DiffPalette` and `Scope`.
- **Gutter Rendering**:
  Update `render_gutter` to query `diff_palette.gutter.num` and `diff_palette.gutter.icon`.
- **Token Color Lookup**:
  Re-implement `palette.token_color(color, row_kind, mov_side, scope)` using direct dispatch:
  ```rust
  pub fn token_color(
      &self,
      color: TokenColor,
      row_kind: RowKind,
      mov_side: Option<MovSide>,
      scope: Scope,
  ) -> Option<Color> {
      let kind = DiffKind::from_row(row_kind, mov_side);
      self.scope(scope)[kind].syntax.token_color(color)
  }
  ```

### Acceptance Criteria
- Unit tests in `diffuse-render` updated and passing.
- Terminal markdown table and diff row rendering pass with zero regressions.

---

## 6. Phase 6 — Interactive TUI Grid, Header, Gutter & Mode Toggle Migration

### Architectural Principle
Update the interactive terminal reviewer (`diffuse-tui`) to dynamically resolve `Scope` based on cursor position, and seamlessly switch themes on `t` key press via `dark_pair`/`light_pair`.

### Key Types and Modules
- **Location**: `crates/diffuse-tui/src/view/diff_grid/`, `crates/diffuse-tui/src/view/header/`, `crates/diffuse-tui/src/app/render/`, `crates/diffuse-tui/src/app/dispatch/`
- **Scope Resolution Function**:
  ```rust
  impl AppState {
      pub fn scope_for_row(&self, row_idx: usize) -> Scope {
          if row_idx == self.row_idx {
              Scope::Row
          } else if self.is_row_in_current_hunk(row_idx) {
              Scope::Hunk
          } else if self.is_row_in_current_file(row_idx) {
              Scope::File
          } else {
              Scope::None
          }
      }
  }
  ```
- **Banner Styling**:
  - File header banners query `palette.scope(file_scope).file_header`.
  - Hunk divider banners query `palette.scope(hunk_scope).hunk_divider`.
- **Mode Toggle**:
  Pressing `t` resolves the current theme's paired twin (`light_pair` or `dark_pair`) with instant $O(1)$ visual update.

### Acceptance Criteria
- TUI interactive navigation and visual rendering pass all existing test suites.
- Cursor row highlights and active hunk indicators maintain visual fidelity.

---

## 7. Phase 7 — Dynamic User Theme Discovery, Configuration & CLI Integration

### Architectural Principle
Support user customization through explicit CLI flag (`--theme <name-or-path>`) and standard discovery paths (`$XDG_CONFIG_HOME/diffuse/themes/` / `~/.config/diffuse/themes/`).

### Key Types and Modules
- **Location**: `crates/diffuse/src/main.rs`, `crates/diffuse/src/args.rs`, `crates/diffuse-render/src/theme/loader.rs`
- **CLI Arguments**:
  Add `--theme <THEME>` option to CLI parser.
- **Theme Loader**:
  1. If `--theme` specifies an existing file path, load it.
  2. If `--theme` specifies a known name (`dark`, `light`), use the bundled asset.
  3. Otherwise, search config directories for `<THEME>.toml`.

### Acceptance Criteria
- Integration test loading a custom TOML theme file from disk.
- Graceful fallbacks with informative errors for malformed or missing theme files.

---

## 8. Phase 8 — Frozen Golden Corpus Verification, Dual-Leg Sign-Off & Benchmarks

### Architectural Principle
Enforce zero deviation on all regression baselines and performance budgets per [`tests/AGENTS.md`](../tests/AGENTS.md).

### Verification Steps
1. Execute full verification gate: `./scripts/check.sh`.
2. Verify all 41 frozen golden corpora in `tests/golden/` across both the library leg and CLI binary leg.
3. Assert that unit tests execute in $< 0.5s$ and `./scripts/check.sh` completes in $< 3s$ on warm builds.

### Acceptance Criteria
- 100% golden corpora match byte-for-byte.
- Zero compiler warnings under nightly `rustfmt` and `clippy -- -D warnings`.
