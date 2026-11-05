# Plan — Unified DiffPalette, Cursor-Tiered Theming and TOML Engine

## Global TODO
- [x] Phase 1 — Foundational Surface, StyledColor & Primitive Palette Modeling (`SurfaceColors`, `StyledColor`, `GutterColors`, `SyntaxPalette`, `PrimitivePalette`)
- [x] Phase 2 — Cursor Axis, DiffKind Indexing & Scoped Container Architecture (`Cursor`, `DiffKind`, `DiffPalette`, `CursorPalette`, `ThemePalette`) *(revised in phase 5: `Scope` renamed `Cursor` with baseline `Rest`, `MovSrc`/`MovTgt` collapsed to `Mov`)*
- [x] Phase 3 — TOML Theme Schema, Serde Deserializer & Cascading Engine with `inherits` & `$variable` Reference (`crates/diffuse-render/src/theme/toml/`)
- [x] Phase 4 — Bundled Canonical Dark and Light TOML Assets with Mode Pairing (`themes/dark.toml`, `themes/light.toml`)
- [x] Phase 5 — Render Pipeline & Row Styler Migration (`crates/diffuse-render/src/`)
- [x] Phase 6 — Interactive TUI Grid, Header, Gutter & Mode Toggle Migration (`crates/diffuse-tui/src/`)
- [·] Phase 7 — Dynamic User Theme Discovery, Configuration & CLI Integration (`crates/diffuse/src/`)
- [ ] Phase 8 — Frozen Golden Corpus Verification, Dual-Leg Sign-Off & Benchmarks

---

## Executive Summary

The diffuse theme palette currently represents diff colors, syntax tokens, and UI chrome across flat, ad-hoc struct fields in [`crates/diffuse-render/src/theme/palette/mod.rs`](../crates/diffuse-render/src/theme/palette/mod.rs). Colors are multiplied across repetitive naming permutations (`add_*`, `del_*`, `mod_*`, `mov_*`, `ctx_*`, `*_in_hunk`, `*_out_hunk`, `*_cursor_bg`, `*_selected`, `*_current`, `*_rest`), leading to over 140 individual fields with hand-coded match statements and zero support for external configuration files.

This plan executes a comprehensive re-architecture inspired by canonical, state-of-the-art theming systems (Helix, Zed, Alacritty):
1. **Structural Cursor Containment Axis**: Introduces `enum Cursor { Rest, File, Hunk, Row }`, formalizing the visual hierarchy of diff review ($\text{Rest (Global)} \supset \text{File} \supset \text{Hunk} \supset \text{Row}$) *(revised in phase 5: originally `Scope { None, ... }`)*.
2. **Unified Surface Abstraction & Typography**: Encapsulates 3-layer visual composite styling into `SurfaceColors` (`bg`, `fill`, `fg`, plus the banner `path` color added in phase 6) and supports optional text modifiers (`bold`, `italic`, `dim`, `underline`) on `StyledColor`.
3. **First-Class DiffKind & Array Indexing**: Defines `enum DiffKind { Ctx, Add, Del, Mod, Mov }`, unifying all diff kinds under an identical `DiffPalette` and enabling $O(1)$ dual-indexed access: `palette[cursor][diff_kind]` *(revised in phase 5: originally `MovSrc`/`MovTgt`)*.
4. **Primitive Swatches with `$variable` Expansion**: Introduces `PrimitivePalette` defining foundational color swatches (`red`, `green`, `bg_dark`, etc.) referenced symbolically in TOML (e.g. `fg = "$green"`).
5. **Declarative TOML Engine with `inherits` & Theme Pairing**: Supports standalone dark/light theme files, base theme extension (`inherits = "dark"`), and twin mode pairing (`light_pair = "catppuccin-latte"`).
6. **Zero-Allocation Runtime Discipline**: All TOML deserialization and cascading inheritance resolve at load time into a flat, `Copy`, array-indexed `ThemePalette` guaranteeing $O(1)$ pointer offsets with zero heap allocations during terminal rendering.

---

## Table of Contents
1. [Phase 1 — Foundational Surface, StyledColor & Primitive Palette Modeling](#1-phase-1--foundational-surface-styledcolor--primitive-palette-modeling)
2. [Phase 2 — Cursor Axis, DiffKind Indexing & Scoped Container Architecture](#2-phase-2--cursor-axis-diffkind-indexing--scoped-container-architecture)
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

## 2. Phase 2 — Cursor Axis, DiffKind Indexing & Scoped Container Architecture

### Architectural Principle
Establish two orthogonal axes:
1. **Cursor Axis**: Visual focus depth ($\text{Cursor::Rest} \subset \text{Cursor::File} \subset \text{Cursor::Hunk} \subset \text{Cursor::Row}$).
2. **DiffKind Axis**: Semantic diff variants; both relocation sides share the single `Mov` channel (sides differ in wire line numbers and gutter glyphs, never in theme colors).

> **Revision (Phase 5)**: the axis enum was renamed `Scope` → `Cursor` with the
> baseline variant `None` → `Rest` ("scope" collides with lexical-scope
> reading; the axis is the interactive cursor's focus depth), and the
> `MovSrc`/`MovTgt` variants were collapsed into one `Mov` variant — the TOML
> schema has a single `mov` channel and no consumer can express a side split,
> so two compiled slots were dead complexity.

### Key Types and Modules
- **Location**: `crates/diffuse-render/src/theme/cursor.rs`, `crates/diffuse-render/src/theme/diff.rs`, `crates/diffuse-render/src/theme/palette/mod.rs`
- **`Cursor`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
  #[repr(u8)]
  pub enum Cursor {
      #[default]
      Rest = 0,   // Global baseline tier (outside active file / rest content)
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
      Mov = 4,   // both relocation sides (shared channel)
  }

  impl DiffKind {
      pub const COUNT: usize = 5;

      #[inline]
      pub fn from_row(kind: RowKind) -> Self {
          match kind {
              RowKind::Add => Self::Add,
              RowKind::Del => Self::Del,
              RowKind::Mod => Self::Mod,
              RowKind::Mov => Self::Mov,
              _ => Self::Ctx, // structural rows (File/Sep) style as context
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
- **`CursorPalette`**:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct CursorPalette {
      pub file_header: SurfaceColors,
      pub hunk_divider: SurfaceColors,
      pub diffs: [DiffPalette; DiffKind::COUNT],
  }

  impl std::ops::Index<DiffKind> for CursorPalette {
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
      pub tiers: [CursorPalette; Cursor::COUNT],
      pub ui: UiPalette,
  }

  impl ThemePalette {
      #[inline]
      pub const fn tier(&self, cursor: Cursor) -> &CursorPalette {
          &self.tiers[cursor as usize]
      }

      #[inline]
      pub const fn diff(&self, cursor: Cursor, kind: DiffKind) -> &DiffPalette {
          &self.tiers[cursor as usize].diffs[kind as usize]
      }
  }
  ```

### Acceptance Criteria
- `palette[Cursor::Row][DiffKind::Add].surface.bg` provides direct, zero-allocation, compile-time verified access.
- Submodule decomposition under `crates/diffuse-render/src/theme/` complies with repository code style.

### Coexistence Note
The legacy flat `ThemePalette` in [`crates/diffuse-render/src/theme/palette/mod.rs`](../crates/diffuse-render/src/theme/palette/mod.rs)
remains the canonical theme consumed by existing renderers until phases 5-7
repoint them. The scoped architecture therefore lives in
`theme/palette/scoped.rs` and its `ThemePalette` is not re-exported at the
`theme` root while the legacy name occupies it; the scoped type becomes the
top-level `theme::ThemePalette` when the legacy palette is retired.

---

## 3. Phase 3 — TOML Theme Schema, Serde Deserializer & Cascading Engine

### Architectural Principle
Declarative theme loading with clean inheritance, `$variable` expansion, and `inherits` extension:
1. **`$variable` Expansion**: Token values like `"$green"` expand against `[palette]`.
2. **`inherits` Directive**: Theme files can extend bundled or parent themes (`inherits = "dark"`), overlaying only specific customizations.
3. **Multi-Tier Cascade**:
   - Cursor: `Row` $\to$ `Hunk` $\to$ `File` $\to$ `Rest`.
   - Diff: `add`, `del`, `mod`, `mov` inherit missing syntax tokens from `ctx`.
   - Mov: the `[rest.mov]` channel covers both relocation sides (single channel; no per-side keys).
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

### Implementation Record
- Builtin registry (`"dark"`/`"light"`, plus `"default"` alias) resolves `inherits`; registry entries declare no base, so chains are acyclic and **cyclic-inheritance testing is deferred** to phases 4/7, when non-builtin declarative themes can participate in chains.
- Cascade field-channel order (per `(cursor, kind, field)`): explicit kind channel along the cursor walk (innermost→outermost, chain self-first), then the `ctx` channel, then mode-baseline fallbacks (primitive swatches for surfaces/gutter, the mode's `SyntaxPalette` for tokens). *(Revised in phase 5: the `mov` alias tier collapsed into the explicit `mov` channel.)*
- `parse_theme_str` returns `ParsedTheme { palette (scoped)::ThemePalette, name, mode }`; the scoped `ThemePalette` stays reachable only via `theme::palette::scoped` until phases 5-7 migrate consumers (see the Phase 2 coexistence note).

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

### Implementation Record
- Assets at `crates/diffuse-render/themes/{dark,light}.toml`; `builtin("dark"|"default")` / `builtin("light")` now parse the embedded assets (`include_str!`), so `inherits = "dark" | "light"` inherits the full canonical values.
- Legacy → scoped mapping frozen by `parse.rs::equivalence` tests: surface `bg`=`*_tint_bg`, `fill`=`*_fill_bg`, `fg`=`*_fg`; gutters `num`/`icon` per kind; syntax through legacy `token_color` semantics — including the ctx `Url`→comment quirk (`[rest.ctx.syntax].url` as of the phase 5 `[rest]` rename) which frozen golden renders depend on. Mod/Mov syntax inherits ctx (legacy `_` arm).
- Banner states by cursor axis: REST at `Cursor::Rest`, CURRENT at `Cursor::File`, SELECTED at `Cursor::Row` (file_header & hunk_divider). In-hunk context gutter via `[hunk.ctx]` (`ctx_gutter_in_hunk` vs out-of-hunk at `Rest`/`File`).
- `scoped ui.bg_modal` := legacy `modal_bg` (the modal canvas); legacy `bg_modal` has no scoped slot. `ColorValue` gained a `"primary"` keyword for `Color::Primary` (`hunk_divider_bg_rest`).
- The equivalence freeze exposed (and realigned) `SyntaxPalette::dark()` baseline divergences: `rule` 243→220, `code` 222→81, `table_header` 220→229.
- **Deferred to phases 5-7** (no scoped slot yet): banner path colors (`file_header_path_*`), per-kind cursor backgrounds (legacy `*_cursor_bg`/`*_fill_cursor_bg`), `band_bg`, `note_tint_bg`, legacy `bg_modal`, `note` row styling. The legacy flat palettes remain authoritative for current renderers until those phases migrate consumers; only then are `dark.rs`/`light.rs` removed.

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

### Implementation Record
- **Architecture revisions (user-directed)**: `Scope` renamed `Cursor` with the baseline variant `Rest` (TOML table `[rest]`), `KindSlot` deleted (the schema's `ScopeModel` → `CursorModel::slot(DiffKind)`), and `DiffKind` collapsed to 5 variants (`Mov` absorbs `MovSrc`/`MovTgt`; the schema's per-side `mov_src`/`mov_tgt` keys are rejected by deny-unknown-fields). Cascade tiers simplify to `[explicit kind, ctx]`. See the Phase 2/3 revision notes.
- **`DiffuseTheme::scoped_palette()`**: compiles the bundled asset on first access into `SCOPED_DARK`/`SCOPED_LIGHT` `OnceLock` statics, keyed on `name == "light"`; returns `&'static scoped::ThemePalette` so `DiffuseTheme` stays `Copy` with its flat `palette` field intact.
- **Migrated to scoped dispatch** (all byte-identical; goldens stayed green): `write_token`/`write_token_stacked` via `scoped[Cursor::Rest][DiffKind::from_row(rk)].syntax.token_color(color)`; `diff/header.rs` header bar bg/fg from `scoped.ui` (`file_header_path_current` stays flat — no scoped slot); `diff/hunk.rs` rest divider from `scoped[Cursor::Rest].hunk_divider` (asset `bg = "primary"` reproduces the empty-string bg-code guard); `row/gutter.rs` `styled_prefix` composes per-kind gutters at `Cursor::Rest` with cross-kind glyphs from sibling slots and ctx gutters switching at `Cursor::Hunk` per the `in_hunk` flag; `counts.rs` `count_color`/`count_cells` take `&UiPalette`.
- **Del-gutter italic is palette data**: assets carry `[rest.del] num = { color, italic = true }`; new `StyledColor::fg_code()` fuses attr flags SGR-style (`\x1b[3;38;5;203m`, attr order bold/italic/dim/underline), byte-identical to the legacy `italic_fg_code` (which remains on `ColorExt`, pinned by test).
- **TUI counts call sites migrated** (mechanical): `view/header/counts.rs` formatters, `render_header_bar`/`render_bottom_ribbon`/`bottom_ribbon_lines` now take `&UiPalette` (their reads are all ui fields); `header_bar_line` keeps `&DiffuseTheme` with flat field reads (phase 6) but routes counts through `scoped_palette().ui`; `view/diff_grid/file.rs` and `app/render/ratatui.rs` count/ribbon callers pass `scoped_palette().ui`.
- **Stays flat (deferred)**: `DiffuseTheme::row_style`, `row/raw.rs` and `row/segment.rs` (cursor/fill backgrounds and `band_bg` had no scoped slot — resolved in phase 6), and all markdown emitters (token routing already flows through the migrated `write_token`). The legacy flat palettes remained authoritative for those fields until phase 6.
- **`parse_builtin(name)`** exported (`theme::toml::parse_builtin` → theme root) as the asset-parse entry for the scoped statics; equivalence suite extended to pin the del-gutter italic attribute.

---

## 6. Phase 6 — Interactive TUI Grid, Header, Gutter & Mode Toggle Migration

### Architectural Principle
Update the interactive terminal reviewer (`diffuse-tui`) to dynamically resolve `Scope` based on cursor position, and seamlessly switch themes on `t` key press via `dark_pair`/`light_pair`.

### Key Types and Modules
- **Location**: `crates/diffuse-tui/src/view/diff_grid/`, `crates/diffuse-tui/src/view/header/`, `crates/diffuse-tui/src/app/render/`, `crates/diffuse-tui/src/app/dispatch/`
- **Cursor Resolution Function** *(superseded in phase 6 by `Cursor::from_banner_state` — consumers hold local selected/current booleans, so a row-index ladder would be dead code)*:
  ```rust
  impl AppState {
      pub fn cursor_for_row(&self, row_idx: usize) -> Cursor {
          if row_idx == self.row_idx {
              Cursor::Row
          } else if self.is_row_in_current_hunk(row_idx) {
              Cursor::Hunk
          } else if self.is_row_in_current_file(row_idx) {
              Cursor::File
          } else {
              Cursor::Rest
          }
      }
  }
  ```
- **Banner Styling**:
  - File header banners query `palette.tier(file_cursor).file_header`.
  - Hunk divider banners query `palette.tier(hunk_cursor).hunk_divider`.
- **Mode Toggle**:
  Pressing `T` (or `Ctrl+t` / F2) resolves the current theme's paired twin (`light_pair` or `dark_pair`) with instant $O(1)$ visual update.

### Acceptance Criteria
- TUI interactive navigation and visual rendering pass all existing test suites.
- Cursor row highlights and active hunk indicators maintain visual fidelity.

### Implementation Record
- **Schema/assets**: `SurfaceModel`/`SurfaceColors` gain the banner `path` color (unset paths resolve to the surface foreground at cascade time via `fg_or_path`); `CursorModel`/`CursorPalette` gain the per-tier `band` surface, cascading along the cursor walk. Both assets declare `path` on the three file-header tiers, `[rest.band]` (single legacy `band_bg` cascading to every tier), and the `[row.{ctx,add,del,mod,mov}]` cursor tables (`bg` = legacy `*_cursor_bg`, `fill` = legacy `*_fill_cursor_bg` for add/mod). New `fill = "none"` clear sentinel (`ColorValue::Clear`) unsets an otherwise cascading fill — required for `[row.del]`, whose legacy fill-cursor is dead — rejected outside `fill` slots (`ThemeParseError::InvalidColor`). New `ui.text` slot (legacy `text`; dark primary, light `#24292f`). Equivalence suite pins all of it field-for-field for both modes, plus banner `fg_or_path` parity.
- **Row styler**: `DiffuseTheme::row_style` keeps its `RowStyle` shape but reads the scoped palette — tint/fill/fg at the `Cursor::Rest` tier, cursor/fill-cursor backgrounds at the `Cursor::Row` tier (del/mov carry no fill, matching legacy); parity test against the flat values for every kind in both modes. `row/segment.rs` and `row/raw.rs` resolve `band_bg` at `tier(Row).band`, `ctx_cursor_bg` at `[Cursor::Row][Ctx].surface.bg`, and their remaining gutter/tint reads at `Cursor::Rest`/`Cursor::Hunk`.
- **Banner/divider tiers**: new `Cursor::from_banner_state(is_selected, is_current)` (selected→Row, current→File, else→Rest; in-band file banners map to File, matching the legacy `in_band || is_current` fold). `view/diff_grid/file.rs` banner colors and path colors (`banner.fg_or_path`), `view/diff_grid/hunk.rs` dividers (4 sites), and the finder modal divider all resolve through the tier axis. `AppState::cursor_for_row` from the original spec was **not** implemented: every consumer already holds local selected/current booleans, so a row-index ladder would be dead code; `from_banner_state` is the tier-selection primitive the TUI actually needs.
- **UI chrome**: every remaining flat read migrated to `scoped.ui` — `app/render/ratatui.rs` (bg/status/scrollbar/text), `blank_status_widget` (`&UiPalette`), `view/header/bar.rs::header_bar_line`, `view/header/status.rs`, `view/status_layout.rs` (+ `push_segment`), `editor/highlight.rs`, all modals (`p = &theme.scoped_palette().ui`; legacy `modal_bg` → `ui.bg_modal`), and `modals/path.rs` formatters (`&UiPalette`). `UiPalette` gained `text`; `ThemePalette` (flat) reads are now zero in diffuse-tui non-test code.
- **Mode toggle**: `TuiCommand::ToggleTheme` now calls `DiffuseTheme::paired()` — resolves the self document's `light_pair`/`dark_pair` asset metadata by name (self-pairing guarded as unpaired; falls back to the default theme), so user themes in phase 7 pair by declaration. `ParsedTheme` exposes `light_pair`/`dark_pair`; keybinds unchanged (`T` / `Ctrl+t` / F2).
- **Notes callout**: `style_focused_note_line` takes the theme and resolves its hardcoded `Fixed(237)` cursor background from `[row.ctx]` (pre-existing theme bypass found in the inventory).

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

### Implementation Record
- **Legacy flat palette removal**: the flat `ThemePalette`, `palette/dark.rs`, `palette/light.rs`, and the orphaned accessors (`default_palette`, `light_palette`, `file_header_colors`, `hunk_divider_colors`, flat `token_color`) are deleted; `palette/mod.rs` now re-exports only the scoped types. Before deletion, the flat values were dumped programmatically and frozen into `theme/baseline.rs` (`cfg(test)`): a `Baseline` struct per mode holding the field set the equivalence suite dereferences, plus kind-major token matrices and a `token_color` method replicating the legacy dispatch (Mod/Mov rows resolve the ctx arm). The equivalence suite in `toml/parse.rs` now compiles the bundled assets against these in-source literals instead of the deleted runtime structures — same assertions, frozen reference. `TOKEN_KEYS` visibility widened to `pub(in crate::theme)` for the baseline module.
- **`DiffuseTheme` reshape**: `{ name: &'static str, mode: Mode, palette: &'static scoped::ThemePalette }`. The `DEFAULT`/`LIGHT` constants became OnceLock-backed constructors `DiffuseTheme::dark()`/`light()` (each compiles its bundled asset on first access and caches the scoped palette as a process-wide static); `scoped_palette()` is retained as a thin accessor over `self.palette` so consumer call sites are unchanged. `DiffuseTheme` is `Copy`. The private `scoped_token_color` became public `token_color`, delegating to a new row-kind-aware `scoped::ThemePalette::token_color` (ctx dispatch at the rest tier). `Mode` is re-exported at the theme root. `diff/header.rs` resolves the current-file path white at `[Cursor::File].file_header.fg_or_path()`.
- **Consumer migration**: all ~470 `DiffuseTheme::DEFAULT`/`LIGHT` references (13 non-test sites, the rest tests) migrated via sed plus targeted rewrites; `theme/tests.rs` legacy pins reworked into scoped/baseline expectations (16 tests rewritten or deleted, incl. the WCAG contrast suite re-anchored on scoped channels); `StatusBarContext.theme` became an owned `DiffuseTheme` (Copy) to keep test helper construction temporary-free; `render_ansi` takes `&UiPalette`; the dead legacy `note_tint_bg` slot (no render consumer) was dropped from the schema, with its test literal inlined.
- **Theme loader** (`theme/loader.rs`): 3-tier `load_theme(name_or_path)` — (1) existing file path parses directly, (2) builtin names resolve through `from_name`, (3) XDG discovery over `$XDG_CONFIG_HOME/diffuse/themes/<name>.toml` then `~/.config/diffuse/themes/`. `ThemeLoadError` distinguishes `NotFound`/`Parse`/`Io` with Display messages carrying the offending path. User palettes leak into stable `'static` references on load, honoring the `DiffuseTheme::palette` contract. 10 unit tests (path, builtins, malformed, missing, XDG + `$HOME` fallback, error display).
- **CLI integration**: `resolve_theme` resolves any `--theme` value through `load_theme` (builtin aliases, file paths, and installed theme names all accepted) and degrades gracefully — unresolvable or malformed themes print `diffuse: {error}; falling back to default theme` and continue with the default. `--theme` help documents the path and config-dir forms. Integration tests load a custom TOML from disk (verifying the leaked name, mode, and a cascaded `ui.accent` override) and pin the malformed-file fallback.
- **Deferred to phase 8**: nothing; golden corpora remained byte-identical throughout (verified by the full gate).
- **Naming canonicalization**: the `scoped` qualifier is retired everywhere it named the cursor-tier architecture. `theme/palette/mod.rs` and `theme/palette/scoped.rs` collapse into one `theme/palette.rs` (matching the single-file layout of `cursor.rs`, `diff.rs`, `surface.rs`, `syntax.rs`), so the palette is `theme::palette::ThemePalette`, re-exported as `theme::ThemePalette`; the `ScopedPalette` aliases in `cascade.rs`/`parse.rs`, the `SCOPED_DARK`/`SCOPED_LIGHT` statics (`DARK_PALETTE`/`LIGHT_PALETTE`), and the redundant `DiffuseTheme::scoped_palette()` accessor are gone, with consumers reading the public `palette` field directly. The plan is retitled to match; `Scope` references in the phase 2/5 records remain as history.

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
