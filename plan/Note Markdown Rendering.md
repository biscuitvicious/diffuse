# Note Markdown Rendering

- [x] 1. [Phase 1 — Block and Inline Markdown Classification (`crates/diffuse-syntax/`)](#1-phase-1--block-and-inline-markdown-classification-cratesdiffuse-syntax)
- [x] 2. [Phase 2 — Markdown Line Assembly and Reflow (`crates/diffuse-render/`)](#2-phase-2--markdown-line-assembly-and-reflow-cratesdiffuse-render)
- [x] 3. [Phase 3 — TUI Note Callout Integration (`crates/diffuse-tui/`)](#3-phase-3--tui-note-callout-integration-cratesdiffuse-tui)
- [ ] 4. [Phase 4 — Task List Checkbox Glyphs](#4-phase-4--task-list-checkbox-glyphs)
- [ ] 5. [Phase 5 — Unicode Table Rendering with Cell Squashing](#5-phase-5--unicode-table-rendering-with-cell-squashing)
- [ ] 6. [Phase 6 — Verification and Acceptance Gate](#6-phase-6--verification-and-acceptance-gate)

---

## Executive Summary

Review notes currently render as raw Markdown source: the TUI callout path
(`AppState::note_callouts`) runs note text through
[`markdown_file_rows`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/pipeline.rs)
and prints each source line separately, so emphasis markers (`**`, `*`, `~~`,
backticks), list dashes, and `---` rules are displayed literally instead of
being rendered.

This plan introduces a **separate, reusable terminal Markdown rendering
mode** that renders the *styling* of the Markdown rather than its source:
emphasis is applied as SGR attributes, bullets draw as `•`, rules draw as a
solid line across the full note width, headings stay bold with their `###`
prefixes, paragraphs and blockquotes join across source lines and reflow to
the content width, and tables draw as column-aligned unicode grids.

Two architectural invariants govern the solution:

- **Subsystem boundaries** ([`GUIDELINES.md` §1](../GUIDELINES.md)): all
  pulldown-cmark parsing and marker-free classification belongs in
  `diffuse-syntax` (which already depends on pulldown-cmark); width reflow,
  glyph substitution, and ANSI stylization belong in `diffuse-render`. No
  new dependencies are required — the parsing *is* split out and reusable
  from `diffuse-syntax`.
- **Wire-contract immutability** ([`tests/AGENTS.md`](../tests/AGENTS.md)):
  `note_runs`, `markdown_file_rows`, `OutputNote.runs`, and the golden
  corpora (`.wire.jsonl`, `.render.txt`, `.markdown.md`) remain
  byte-identical. The new mode is an additional rendering path consumed by
  the TUI; the existing Markdown classification (doc comments, GFM export,
  `--markdown` output) is untouched.

## Requirements Inventory

1. **Styling, not source**: bold, italic, underline, and strikethrough
   modifier characters must not display; the SGR styling applies instead.
2. **Code spans**: no backticks — only the styling (same classification the
   current Markdown mode applies).
3. **Fenced code blocks**: no ``` markers and no language tag; only the
   syntax-highlighted content per fence language.
4. **Bullet lists**: `•` instead of `-`/`*`; ordered lists keep their
   numbers, right-aligned to the widest marker among the list's items
   (` 1. One` … `10. Ten`).
5. **Horizontal rules**: draw as a solid unicode line stretching the full
   content width; rules imply a blank line before and after.
6. **Headings**: keep the `###`-prefixes; render the heading bold; imply a
   blank line before and after.
7. **Paragraph joining**: soft-wrapped paragraphs (and headings) join
   across source lines with whitespace normalized into single-paragraph
   text, parsed into in-paragraph markup `Span`s at parse time, and
   reflowed to extend the full note width instead of breaking on
   newlines.
8. **Blockquotes**: join together like a paragraph; render with a vertical
   bar at the first column; nested blockquotes stack bars recursively
   (`▎quote`, `▎▎nested`). Blank lines whose `>` markers join them to the
   quote are inside it (paragraph separators); blank lines without a
   joining `>` line separate two distinct quote blocks.
9. **Links**: display as `label (url)` — label and url underlined and
   link-colored, parens muted.
10. **Checkboxes**: `[ ]`, `[·]`, `[x]` draw as unicode glyphs for empty,
    in-progress, and checked — decided: `☐` (U+2610), `◐` (U+25D0),
    `☑` (U+2611).
11. **Tables**: render as column-aligned unicode tables; when the natural
    table width exceeds the content width, cells squash and wrap internally
    (separate phase).
12. **Reusability**: the Markdown rendering module is separate and
    note-agnostic — no note-referencing identifiers — so it applies to
    any Markdown (notes are one consumer).
13. **Cow discipline**: pass references straight through (`Cow::Borrowed`)
    whenever rebuilding a `String` is unnecessary.

## Architecture and Styling Decisions

| Aspect | Decision |
|---|---|
| Crate split | Parsing + marker-free classification in `diffuse-syntax`; width reflow + glyphs + ANSI in `diffuse-render`; TUI consumes both. `diffuse-render` gains no new dependency. |
| Block model | Recursive, note-agnostic `markdown::Block` tree: `Blank`, `Para`, `Head`, `List`, `Quote`, `Code`, `Rule`; `ListItem` is a struct, not a `Block` variant — `List { items: Vec<ListItem> }` locks the items-only contract at the type level. Shared container newtypes: `Inline<'a>(Vec<Span>)` unifies inline content (paragraphs, headings, table cells, link labels, emphasis children) and `Flow<'a>(Vec<Block>)` unifies block children (items, quotes, top level). Consecutive all-whitespace lines condense into one `Blank`. |
| Inline model | Parse-time `markdown::Span` trees (`Text`/`Bold`/`Italic`/`Strike`/`Code`/`Link`/`Task`); marker-only event gaps are skipped, prose gaps kept. The renderer maps span variants to theme colors and `MD_SGR_*` attributes. |
| Module naming | Syntax side: `crates/diffuse-syntax/src/markdown.rs` (module `markdown`: `Block`, `Span`, `ListItem`, `TaskState`, `ListMarker`, `blocks`, `spans`). Render side: `crates/diffuse-render/src/markdown/terminal.rs` (re-exported from the existing `markdown` module: `MarkdownLine`, `markdown_lines`, `render_markdown_line`). No note-referencing identifiers. |
| Headings | `###` prefix in `accent` color, then bold title (`MD_SGR_BOLD`); exactly one blank line before and after (collapsed, never doubled). Wrapped continuation lines indent `level + 1` columns to align after the `### ` marker. |
| Rules | One line of `─` (U+2500) repeated to the full content width, `mdRule` color, bold; exactly one blank line before and after. |
| Blockquotes | `▎` (U+258E) bars in muted `border` color at the first column, then the joined, reflowed quote content. Nested blockquotes are a `Quote` inside a `Quote` and stack one bar per depth level (`▎quote`, `▎▎nested`), recursively; content starts at column `depth` and wraps at `width − depth`; continuation lines repeat the stacked bars; `Blank` children render bars-only. Chosen over `│` (U+2502) on aesthetics alone — it reads better as a quote indent bar; revisit during Phase 6 visual review. |
| Bullets | `•` (U+2022) in `accent` color; ordered markers keep their source numbers. |
| Code spans | `syntaxString` classification, surrounding SGR stacked, backticks dropped. |
| Code fences | Content lines as-is (no joining), classified via `for_each_line_classified_token` with the fence-language profile; hard-wrapped at width; fence markers and info string dropped. |
| Links | `label (url)`: label `mdLink` + `MD_SGR_UNDERLINE`, url `mdLink` + `MD_SGR_UNDERLINE`, parens `border`. |
| Blank lines | Explicit `Blank` blocks (condensed runs of all-whitespace source lines) render as one empty visual line — bars-only inside quotes. Headings and rules force exactly one `Blank` before and after (inserted at render when absent, doubles collapsed); top-level leading and trailing `Blank`s are trimmed. |
| Lists | One `List` holds all its items, tight or loose (CommonMark keeps blank-separated items in a single loose list). Blank gaps between items set the next `ListItem.blank_before` and render as a blank line between those items; blank lines followed by indented content belong to the item's children; blank lines followed by outer-level content end the list. Numbered markers right-align to the widest marker across all items; bullets draw `• `. |
| Wrapping | Hard wrap at the content width (consistent with the existing segment slicing), preserving run styling across splits via `Cow::Owned` only at split boundaries. Blocks with a leading marker (headings, list items, checkboxes, blockquotes) wrap at the indent column *after* the marker: continuation lines align with the content's starting column — e.g. `• List item which` wraps to 2 columns, `### Really long` wraps to `level + 1` columns. |
| Wire contract | No changes to `note_runs`, `markdown_file_rows`, `OutputNote`, or golden fixtures. |

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Requirements Inventory](#requirements-inventory)
3. [Architecture and Styling Decisions](#architecture-and-styling-decisions)
4. [Phase 1 — Block and Inline Markdown Classification (`crates/diffuse-syntax/`)](#1-phase-1--block-and-inline-markdown-classification-cratesdiffuse-syntax)
5. [Phase 2 — Markdown Line Assembly and Reflow (`crates/diffuse-render/`)](#2-phase-2--markdown-line-assembly-and-reflow-cratesdiffuse-render)
6. [Phase 3 — TUI Note Callout Integration (`crates/diffuse-tui/`)](#3-phase-3--tui-note-callout-integration-cratesdiffuse-tui)
7. [Phase 4 — Task List Checkbox Glyphs](#4-phase-4--task-list-checkbox-glyphs)
8. [Phase 5 — Unicode Table Rendering with Cell Squashing](#5-phase-5--unicode-table-rendering-with-cell-squashing)
9. [Phase 6 — Verification and Acceptance Gate](#6-phase-6--verification-and-acceptance-gate)
10. [Implementation Notes (review findings)](#implementation-notes-review-findings)
11. [Open Questions](#open-questions)

---

## 1. Phase 1 — Block and Inline Markdown Classification (`crates/diffuse-syntax/`)

### Objective

Parse note Markdown into a block model and classify inline content without
emitting source marker characters. Pure classification — no width, no ANSI.

### Steps

1. **Create `crates/diffuse-syntax/src/markdown.rs`** (module `markdown`,
   re-exported from `lib.rs`) with the note-agnostic block model:

   ```rust
   /// A flow of sibling Markdown blocks — the children of a `ListItem`
   /// or `Quote`, and the top-level document. Lists hold `Vec<ListItem>`
   /// directly, locking the items-only contract at the type level.
   pub struct Flow<'a>(Vec<Block<'a>>);

   /// A list item: marker, child blocks, and whether a blank line
   /// separated it from the previous item. Loose gaps stay inside one
   /// `List` — CommonMark keeps blank-separated items in a single loose
   /// list.
   pub struct ListItem<'a> {
       pub blank_before: bool,
       pub marker: ListMarker<'a>,
       pub children: Flow<'a>,
   }

   /// A run of sibling inline spans — paragraph and heading content,
   /// table cells, link labels, and emphasis children all share this one
   /// representation.
   pub struct Inline<'a>(Vec<Span<'a>>);

   /// A parsed Markdown block. Variant names do not repeat `Block` —
   /// `Quote` and `Code`, not `BlockQuote`/`CodeBlock`.
   pub enum Block<'a> {
       /// One or more consecutive all-whitespace source lines, condensed.
       Blank,
       /// Paragraph: whitespace-normalized, joined text parsed into
       /// in-paragraph markup spans.
       Para { content: Inline<'a> },
       /// ATX heading: level (1–6) and title content, `#` markers stripped.
       Head { level: u8, content: Inline<'a> },
       /// A list: all of its items — tight or loose gaps alike — group
       /// here so the renderer can align markers across every item.
       List { items: Vec<ListItem<'a>> },
       /// Blockquote: child blocks; a nested quote is a `Quote` child.
       Quote { children: Flow<'a> },
       /// Fenced code: content lines (no fence markers), info string kept.
       /// Code preserves line breaks — no joining.
       Code { lang: Option<Cow<'a, str>>, lines: Vec<Cow<'a, str>> },
       Rule,
   }

   /// In-paragraph markup. Emphasis variants nest; the renderer maps each
   /// variant to theme color and SGR.
   pub enum Span<'a> {
       Text(Cow<'a, str>),
       Bold(Inline<'a>),
       Italic(Inline<'a>),
       Strike(Inline<'a>),
       /// Code span: inner text, backticks stripped.
       Code(Cow<'a, str>),
       Link { label: Inline<'a>, url: Cow<'a, str> },
       Task(TaskState),
   }

   pub enum TaskState {
       Empty,
       InProgress,
       Checked,
   }

   pub enum ListMarker<'a> {
       Bullet,
       Numbered(Cow<'a, str>),
   }

   /// Parses Markdown into a top-level block flow. Blank source runs
   /// condense into one `Blank`; blank gaps between list items stay
   /// inside the `List`, setting the next item's `blank_before`
   /// (CommonMark keeps blank-separated items in one loose list); blank
   /// lines inside a blockquote are `Blank` children of the quote.
   pub fn blocks(text: &str) -> Flow<'_>;
   ```

   Implementation: pulldown-cmark `Parser::new_ext` (GFM option set) with
   `into_offset_iter()`; recursive descent maintaining a stack of open
   containers (`List`, `Quote`, `ListItem`); leaf blocks sliced from
   source ranges with `Cow::Borrowed` whenever no rebuild is needed
   (`Cow::Owned` only for joined, normalized, or marker-stripped text).
   Runs of all-whitespace lines between blocks emit one condensed `Blank`.

2. **Add span parsing** in the same module:

   ```rust
   /// Parses inline markup over whitespace-normalized text into inline
   /// content, emitting no source markers.
   pub fn spans(text: &str) -> Inline<'_>;
   ```

   Walk pulldown-cmark inline events over the normalized block text:
   - `Strong`/`Emphasis`/`Strikethrough` → `Span::Bold`/`Italic`/`Strike`
     holding their recursively parsed child spans; opening and closing
     markers skipped.
   - `Code` → `Span::Code` with the inner text (backtick runs stripped
     from both ends).
   - `Link` → `Span::Link { label, url }`; brackets and parens never
     display (rendering draws `label (url)`).
   - Gaps between event ranges: skipped when they start with a marker
     character (`* _ ~ \` [ ] > - #`); otherwise emitted as `Span::Text`
     prose.
   - `TaskListMarker` and the non-standard `[·]` in-progress marker →
     `Span::Task` with `TaskState::Empty | InProgress | Checked`. A
     dedicated scanner recognizes `[·]` because pulldown-cmark parses
     only `[ ]`/`[x]`.
   - `Html`/`InlineHtml` → `Span::Text` (source passes through as prose).

3. **Test-first**: add `crates/diffuse-syntax/src/markdown/tests.rs`
   (following the `x/tests.rs` submodule pattern):
   - Block tests: paragraph join + whitespace normalization into a single
     text; heading level with `#`s stripped; one `List` holding all items
     with loose gaps setting `ListItem.blank_before`; blank lines followed
     by indented content becoming `Blank` children of the item vs blank
     lines followed by outer-level content ending the list;
     `>`-joined blank lines as `Blank` children of `Quote` vs plain
     blanks separating two `Quote`s; nested `Quote`-inside-`Quote`; fence
     `lang` extraction with markers excluded; rule detection; consecutive
     blanks condensing into one `Blank`.
   - Span tests: `**bold**`/`*italic*`/`~~strike~~` produce `Bold`/
     `Italic`/`Strike` spans with zero `*`/`~` characters in their text;
     `` `code` `` produces a `Code` span without backticks; `[label](url)`
     produces `Link { label, url }`; `[ ]`/`[·]`/`[x]` classify to the
     three `TaskState`s; literal `*` in prose (e.g. `2 * 3`) survives as
     `Text`; nested emphasis (`*a **b** c*`) nests spans.
   - Every branch of the container walk (nested quotes, nested lists,
     quotes inside list items, unterminated fences) is exercised.

---

## 2. Phase 2 — Markdown Line Assembly and Reflow (`crates/diffuse-render/`)

### Objective

Assemble classified blocks into styled visual lines that fit the content
width, with blank-line semantics, glyph drawing, and theme-mediated ANSI.

### Steps

1. **Create `crates/diffuse-render/src/markdown/terminal.rs`** (submodule
   of the existing `markdown` module, re-exported; tests in
   `markdown/terminal/tests.rs`):

   ```rust
   /// A rendered visual line of Markdown: styled runs whose total display
   /// width fits the requested content width.
   pub struct MarkdownLine {
       pub runs: Vec<OutputRun>,
   }

   /// Renders Markdown text as styled visual lines reflowed to `width`.
   pub fn markdown_lines(text: &str, width: usize) -> Vec<MarkdownLine>;

   /// Writes one rendered line with the TUI prefix applied.
   pub fn write_markdown_line<W: Write>(
       out: &mut W,
       line: &MarkdownLine,
       prefix: &str,
       syntax_on: bool,
       theme: &DiffuseTheme,
   ) -> std::fmt::Result;

   /// `write_markdown_line` into an owned `String`.
   pub fn render_markdown_line(
       line: &MarkdownLine,
       prefix: &str,
       syntax_on: bool,
       theme: &DiffuseTheme,
   ) -> String;
   ```

2. **Line assembly rules** (recursive render over `markdown::Block` with
   a context of indent and stacked quote bars):
   - Tabs expanded once upfront (`expand_tabs(text, 4)`); all downstream
     slicing borrows from the expanded text (`Cow`).
   - Marker-indent wrapping: blocks with a leading marker wrap at the
     effective width `width − indent`, where `indent` is the marker column
     width plus one space; continuation lines start with an indent run of
     spaces (`Cow`-borrowed from a static padding slice). Paragraph spans
     flatten to runs and hard-wrap at the effective width (split with
     `Cow::Owned` only at boundaries).
   - `Blank` renders one empty visual line; inside quotes it renders
     bars-only. Headings and rules force exactly one `Blank` before and
     after (inserted when absent, doubles collapsed); top-level leading
     and trailing blanks are trimmed.
   - `Para`: `Inline` content flattens to runs — `Text` → `text` color
     (context
     styled), `Bold`/`Italic`/`Strike` → innermost span style wins
     (`MD_SGR_BOLD`/`MD_SGR_ITALIC`/`MD_SGR_STRIKE`, `mdBold`/`mdItalic`/
     `mdStrike`), `Code` → `syntaxString`, `Link` → underlined `mdLink`
     label + muted `border` parens + underlined `mdLink` url, `Task` →
     glyph (Phase 4).
   - `Head`: `###` (level hashes) `accent` run + title spans rendered
     bold (`MD_SGR_BOLD` context); continuation lines indent `level + 1`.
   - `Quote`: pushes one `▎` `border`-colored bar onto the bar stack; every
     visual line of the quote's children begins with the stacked bars,
     content starts at column `depth`, and children wrap at
     `width − depth`; continuation lines repeat the stacked bars.
   - `List`: compute the widest marker across all items; numbered markers
     right-align to it (` 1. One` / `10. Ten`), bullets draw `• `; each
     `ListItem` renders its marker at the current indent and its children
     at the content column (marker width + 1); items with `blank_before`
     render a blank line before them (never before the first item).
   - `Code`: lines rendered as-is, classified via
     `for_each_line_classified_token` with `lang_profile(fence_lang)`;
     hard-wrapped at the effective width; no fence markers.
   - `Rule`: one line of `─` × effective width, `mdRule` color +
     `MD_SGR_BOLD`.
   - Every styled line passes through `theme.write_token` (zero direct ANSI
     escapes; `sgr` and colors mediated by the theme).

3. **Test-first** (`crates/diffuse-render/src/markdown/terminal/tests.rs`):
   - Emphasis markers absent from rendered output while SGR codes present
     (`\x1b[1m` for bold, `\x1b[3m` italic, `\x1b[9m` strike).
   - Bullet `•` replaces dashes; numbered markers right-align to the
     widest in the `List` (` 1. One` … `10. Ten`).
   - Rule line: exactly the effective width in `─` characters, with
     forced blank lines on both sides.
   - Heading: `###` prefix + bold text + surrounding blank lines (exactly
     one each side, including when the source omits them).
   - Soft-wrapped paragraph joins and fills the full width across line
     breaks (single reflowed logical line, wrapped at `width`).
   - Marker-indent wrapping: a long list item wraps with continuations
     aligned after `• `; a long heading wraps aligned after `### `; a
     long checked task wraps aligned after the bullet + glyph.
   - Fence content syntax-highlighted with no ``` or language tag.
   - Blockquote joins across `>` lines; every visual line begins with `▎`;
     a nested quote renders stacked bars (`▎▎`) with continuation lines
     repeating them; a `>`-joined blank inside a quote renders bars-only;
     a plain blank between quotes separates two quote blocks.
   - Loose gaps render: blank lines between items of one `List` render
     between those items, and numbered alignment spans loose gaps.
   - Cow behavior: borrowed runs dominate when no split/join occurs
     (asserted via a `Cow`-variant probe in tests).

---

## 3. Phase 3 — TUI Note Callout Integration (`crates/diffuse-tui/`)

### Objective

Switch `AppState::note_callouts` to the new rendering mode; keep geometry,
anchoring, and the note modal (raw editing) untouched.

### Steps

1. **Rewire `crates/diffuse-tui/src/app/notes.rs`** — replace the
   `markdown_file_rows` + segment-count + `render_note_segment` machinery:

   ```rust
   let text_cols = content_width.saturating_sub(8).max(1);
   let lines = crate::render::markdown_lines(&n.text, text_cols);
   let total = lines.len();
   for (visual_idx, line) in lines.iter().enumerate() {
       let prefix = match visual_idx {
           0 => first_prefix.as_str(),
           n if total > 1 && n + 1 == total => final_prefix.as_str(),
           _ => line_prefix.as_str(),
       };
       let rendered = crate::render::render_markdown_line(
           line, prefix, self.syntax_on, &self.theme,
       );
       out.entry(target_row).or_default().push(rendered);
   }
   ```

   The existing `first_prefix`/`line_prefix`/`final_prefix` gutter band
   (💬 / │ / └┄) is unchanged. `note_runs`/`markdown_file_rows` remain for
   the wire contract.

2. **Update the TUI note tests**
   (`crates/diffuse-tui/src/app/notes/tests.rs`) to the new expectations:
   `test_note_callouts_full_markdown_multiline_and_wrap` asserts the first
   line as `▌███ 💬 Test Markdown formatting:` (markers gone), `• List item
   one` bullets, joined reflowed paragraphs, and fence content without
   backticks. `test_note_callout_gutter_band_and_terminal_line` keeps its
   gutter assertions.

3. **Verify geometry invariants**: blank rule/heading lines participate in
   `cached_geometry`/`visual_geometry` line counts; cursor navigation and
   click dispatch (`dispatch.rs` `note_lines` consumer) stay correct with
   multi-line blocks.

---

## 4. Phase 4 — Task List Checkbox Glyphs

### Objective

Draw task list states as unicode glyphs instead of bracket source text.

### Steps

1. **Glyph mapping** in `crates/diffuse-render/src/markdown/terminal.rs`
   (`Span::Task` rendering — implemented in Phase 2 for the wrapping test;
   this phase adds the dedicated mapping tests): `TaskState::Empty | InProgress | Checked` →
   glyph run + trailing space:
   `☐` (U+2610) for `[ ]`, `◐` (U+25D0) for `[·]`, `☑` (U+2611) for
   `[x]`. Task items live inside list items, so continuation lines indent
   past the full marker chain (bullet + glyph + space) per the
   marker-indent wrapping rule.
2. **Test-first**: each state maps to the chosen glyph; the bracket source
   (`[ ]`, `[·]`, `[x]`) never appears in rendered output; mixed task lists
   render in source order.

---

## 5. Phase 5 — Unicode Table Rendering with Cell Squashing

### Objective

Render tables as column-aligned unicode grids that degrade gracefully when
wider than the content width.

### Steps

1. **Use the Phase 1 block model** (`Block::Table` with header rows,
   body rows of `Inline` cells, and `ColumnAlign` per column — extraction
   already done; see Implementation Notes §4).
   pulldown-cmark's `ENABLE_TABLES` events drive extraction.
2. **Column layout algorithm** in `crates/diffuse-render/src/markdown/terminal.rs`:
   - Measure natural column widths = max display width of each column's
     cells.
   - Budget = `width` minus border and padding overhead
     (`│ ` + ` │` per cell, corner joints).
   - When the natural total fits: pad every cell to its column width.
   - When it overflows: squash — reduce column widths proportionally to
     their natural widths until the total fits the budget, then wrap each
     cell's content into multiple lines within its squashed width (word
     wrap; runs split with `Cow::Owned` at boundaries); rows grow vertically
     as needed, cells top-aligned.
3. **Borders**: unicode box drawing (`┌ ─ ┬ ┐ │ ├ ┼ ┤ └ ┴ ┘`) in the muted
   `border` color; header row separated by a joint rule.
4. **Test-first**: aligned table renders with exact column padding and
   borders; an over-wide table squashes with wrapped cell text and total
   display width ≤ content width; inline emphasis inside cells keeps SGR
   styling across wrapped cell lines; alignment markers center/right-align
   content within columns.

---

## 6. Phase 6 — Verification and Acceptance Gate

### Steps

1. **Wire/golden invariance**: run `scripts/check.sh`; the golden corpora
   (`.wire.jsonl`, `.render.txt`, `.markdown.md`) and both acceptance legs
   must be byte-identical to the pre-change baseline — the new mode touches
   no engine semantics or wire formats.
2. **Full gate**: `scripts/check.sh` passes with zero warnings (nightly
   `rustfmt`, `clippy --fix`, all cargo tests, extension equivalence via
   `npm test` in `extension/pi/` — the TS extension consumes the wire
   contract, which is unchanged).
3. **Performance budgets**: unit tests < 0.5s each; test binaries < 1.5s;
   `markdown_lines` adds no more than the existing
   `markdown_file_rows` call cost (both parse the text once; the
   `cached_geometry` cache already amortizes `note_callouts`).
4. **Human review**: visual inspection of a note exercising every block
   type (headings, rules, lists, quotes, fences, links, emphasis,
   checkboxes, tables) in the TUI at narrow and wide content widths;
   sign-off per the checkoff discipline in
   [`plan/Plan Guidelines.md`](plan%20Guidelines.md).
5. **Archive**: move this plan to `plan/done/Note Markdown Rendering.md`.

---

## Implementation Notes (review findings)

Deviations from the original design adopted during Phase 1 review:

1. **Offset ranges are hierarchical, not sequential.** `into_offset_iter`
   `Start` ranges cover whole subtrees (`Start(List)` spans the full list),
   so generic gap scanning was unusable. The blank cursor advances only on
   `End` and leaf events, with boundary-aware line analysis
   (`gap_has_blank`). Loose inter-item gaps are detected via trailing
   blanks *inside* item ranges (`range_text_ends_blank`) — an undocumented
   pulldown-cmark 0.13 behavior pinned by contract tests
   (`test_pulldown_item_range_includes_trailing_blank`,
   `test_pulldown_bare_inline_under_item`).
2. **Bare inline under items is normalized by a pre-pass.**
   `with_item_paragraphs` wraps tight-item text and task markers (which
   carry no `Paragraph` events) in synthetic `Start/End(Paragraph)` using
   the first/last inline range, so the main walk only creates paragraphs in
   one place.
3. **Shared inline builder, explicit normalization.** `blocks()` builds
   `Inline` directly from the outer event stream through the same
   `InlineBuilder` that `spans()` drives (no double parse); **every**
   Unicode whitespace run (`char::is_whitespace`: NBSP, em/ideographic
   spaces, tabs, newlines) folds to one ASCII space via
   `normalize_inline_whitespace`, with join-aware collapsing across merged
   spans and clean text keeping its borrow. Code spans keep
   pulldown-cmark's exact content (no extra `.trim()`).
4. **`Block::Table` moved to Phase 1.** The header row carries no
   `TableRow` (cells land directly under `TableHead`), `Start(TableHead)`
   pushes no frame, and extraction (header, rows, `ColumnAlign`) lives in
   the block model so renderers match exhaustively. Phase 2 renders tables
   simply (aligned when they fit); Phase 5 keeps only cell squashing.
5. **Task glyphs land in Phase 2.** The marker-indent wrapping test needs
   the `☐`/`◐`/`☑` mapping, so rendering implements it; Phase 4 adds the
   dedicated mapping tests.
6. **Heading titles stack bold + underline** (Open Questions §2
   reversed): `write_token_stacked` combines SGR into one escape with
   reverse-order resets; `MarkdownRun` mirrors `OutputRun` plus an
   `underline` flag propagated through nested emphasis (deduped against
   link underlines). Markers, bars, and borders never underline.

---

## Open Questions

1. **Checkbox glyphs** — decided: `☐` (U+2610 BALLOT BOX) for `[ ]`,
   `◐` (U+25D0 CIRCLE WITH LEFT HALF BLACK) for `[·]`, `☑` (U+2611
   BALLOT BOX WITH CHECK) for `[x]`. The half-filled circle contrasts
   cleanly with the two ballot boxes for the in-progress state.

2. **Header emphasis** — reversed: bold title **plus underline** (`###`
   prefix accent, title `MD_SGR_BOLD` + `MD_SGR_UNDERLINE` via one combined
   `\x1b[1;4m` escape). `DiffuseTheme::write_token_stacked` emits combined
   SGR with reverse-order resets; `MarkdownLine` carries `MarkdownRun`
   (mirror of `OutputRun` plus an `underline` flag) so the single-SGR wire
   types stay untouched. Single-style output is byte-identical to before.

3. **Loose lists** — decided (re-aligned with CommonMark after two
   reversals): blank-separated items stay in ONE `List`; the gap sets the
   next item's `blank_before` and renders as a blank line between exactly
   those items. This keeps numbered-marker right-alignment correct across
   loose gaps and matches pulldown-cmark's parse (one `List` event for a
   loose list). Per-gap fidelity intentionally refines CommonMark's
   list-wide loose flag.
