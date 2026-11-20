# diffuse — Wire Format Specification

This document specifies the frozen JSON and JSON Lines (JSONL) wire format contracts implemented by the `diffuse` diff analysis engine. The wire format serves as the public serialized interface between the native Rust engine and consuming frontends, including the interactive terminal UI (`diffuse-tui`), the CLI `--json` and streaming output modes, and external editor extensions.

---

## 1. Design Principles & Architecture

The wire format is optimized around three core architectural tenets:

1. **Zero Text Redundancy**: Traditional diff formats duplicate line text multiple times (pre-image text, post-image text, and intra-line subword slices). Diffuse serializes text strictly within token runs (`OutputRun`). Complete pre- and post-image line contents are projected on demand via monoidal reconstruction:
   $$\text{new\_text}(R) = \sum_{r \in R, r.\text{kind} \neq \text{Del}} r.\text{text}, \quad \text{old\_text}(R) = \sum_{r \in R, r.\text{kind} \neq \text{Add}} r.\text{text}$$
2. **Compact Positional Tuple Encoding**: In line-by-line diff streams, 98% or more of payload records represent line modifications within hunks. Repeating dictionary keys (`"kind"`, `"runs"`, `"newLine"`) incurs substantial serialization and parsing overhead. High-frequency elements—token runs (`OutputRun`) and line rows (`OutputRow`)—are encoded as positional JSON tuples (arrays).
3. **Heterogeneous Protocol Framing**: Structural framing records (`FileHeader`, `HunkHeader`, `FileFooter`) are serialized as typed JSON objects with an explicit discriminator `"t"`. Row records are serialized strictly as JSON arrays, allowing line parsers to distinguish structural metadata from hunk content via single-byte prefix inspection (`{` vs. `[`).

---

## 2. Streaming JSONL Protocol (`wire.jsonl`)

The streaming protocol emits a line-delimited sequence of JSON records. Each line is an independent, valid JSON record terminated by a newline (`\n`).

### 2.1 Stream Record Grammar

A valid JSONL stream conforms to the following state grammar per file diff:

$$\text{Stream} \to (\text{FileHeader} \cdot (\text{HunkHeader} \cdot \text{Row}^*)^* \cdot \text{FileFooter})^*$$

Records are disambiguated by their top-level JSON type:
- **JSON Object (`{ ... }`)**: Framing envelope records containing a tag field `"t"`.
- **JSON Array (`[ ... ]`)**: Line rows within a hunk (`OutputRow`).

```
+-------------------------------------------------------------+
| {"t":"file_header", "path":"src/main.rs", ...}              | -> FileHeader
|   {"t":"hunk_header", "oldStart":1, "newStart":1, ...}      | -> HunkHeader
|     ["ctx", 1, 1, ["_fn main() {"]]                         | -> OutputRow (Line)
|     ["mod", 2, 2, ["-    old();", ["+    new();", "kwd"]]]  | -> OutputRow (Line)
|     ["ctx", 3, 3, ["_}"]]                                   | -> OutputRow (Line)
|   {"t":"file_footer", "path":"src/main.rs", ...}            | -> FileFooter
+-------------------------------------------------------------+
```

---

## 3. Row Record Specification (`OutputRow`)

Every line within a diff hunk is serialized as an `OutputRow` tuple.

### 3.1 Positional Tuple Schema

$$\text{RowTuple} = [\text{kind}, \text{old\_line}, \text{new\_line}, \text{runs}, \text{flags}?]$$

| Index | Field | JSON Type | Description |
| :---: | :--- | :--- | :--- |
| **0** | `kind` | `string` | Canonical row classification: `"ctx"`, `"add"`, `"del"`, `"mod"`, `"mov"`, `"file"`, `"sep"`. |
| **1** | `old_line` | `integer \| null` | 1-based coordinate in pre-image file, or `null` for additions. |
| **2** | `new_line` | `integer \| null` | 1-based coordinate in post-image file, or `null` for deletions. |
| **3** | `runs` | `array` | Sequence of classified token runs (`OutputRun`). |
| **4** | `flags` *(optional)* | `object` | Supplementary metadata flags. Omitted when empty. |

### 3.2 Supplementary Metadata Flags (`RowFlags`)

When a row exhibits special relocation or grouping characteristics, an optional 5th element object is appended:

```json
{
  "movSide": "src" | "tgt",
  "wholesale": true,
  "group": { "role": "peer", "id": 7, "slot": 2, "n": 2, "m": 3 }
}
```

- **`movSide`**: `"src"` indicates the row represents the origin site of a relocated block; `"tgt"` indicates the destination site.
- **`wholesale`**: `true` indicates a modification block where the pre-image and post-image differ so completely that intra-line subword alignment is bypassed in favor of discrete deletion and addition blocks.
- **`group`**: Identifies rows belonging to an (N \u2192 M) peer reflow group:
  - `{"role": "peer", "id": <u32>, "slot": <usize>, "n": <usize>, "m": <usize>}`: `id` is the group identity shared by all member rows, `slot` is the member's position within the group, and `n` / `m` are the old-side / new-side row cardinalities.

### 3.3 Concrete Row Examples

1. **Context Row**:
   ```json
   ["ctx", 42, 42, ["_fn calculate() {"]]
   ```
2. **Deletion Row**:
   ```json
   ["del", 10, null, ["-    let count = 0;"]]
   ```
3. **Addition Row**:
   ```json
   ["add", null, 15, [["+    let mut count = 0;", "kwd"]]]
   ```
4. **Modified Row with Subword Highlighting**:
   ```json
   ["mod", 25, 27, ["_    let ", "-old_value", ["+new_value", "typ"], "_ = 1;"]]
   ```
5. **Relocated Block Destination**:
   ```json
   ["mov", null, 88, [["+pub fn helper() {}", "fun"]], {"movSide": "tgt"}]
   ```
6. **Grouped Peer Row**:
   ```json
   ["add", null, 142, ["+    additional_field: true,"], {"group": {"role": "peer", "id": 7, "slot": 2, "n": 2, "m": 3}}]
   ```

---

## 4. Token Run Specification (`OutputRun`)

A token run represents a contiguous slice of text sharing identical diff change status (`kind`), semantic syntax coloring (`color`), and ANSI style attribute modifiers (`sgr`).

### 4.1 Sign Prefix Encoding

Every token run embeds its diff classification as a single-character ASCII prefix prepended directly to the run text:

| Prefix | Change Kind | Semantic Representation |
| :---: | :--- | :--- |
| `'_'` | Context (`Ctx`) | Unchanged text present in both pre- and post-image. |
| `'-'` | Deletion (`Del`) | Text deleted from pre-image. |
| `'+'` | Addition (`Add`) | Text added in post-image. |

### 4.2 Compact Run Representations

Runs serialize as either a string scalar or a compact array based on presence of styling:

1. **Unstyled / Default Text** *(scalar string)*:
   $$\text{"<sign><text>"}$$
   *Example*: `"_    let x = "` or `"-old_fn()"`
2. **Syntax Highlighted Run** *(2-element array)*:
   $$[\text{"<sign><text>"}, \text{"<color>" package}]$$
   *Example*: `["+struct", "kwd"]` or `["_Point", "typ"]`
3. **Syntax Highlighted with SGR Emphasis** *(3-element array)*:
   $$[\text{"<sign><text>"}, \text{"<color>" package}, \text{sgr}]$$
   *Example*: `["_# Architecture", "dlm", 1]` (heading with SGR 1 bold)

Default text color (`TokenColor::Default`) serializes to `null` (or is omitted entirely in scalar format when no SGR modifier is present).

---

## 5. Canonical Semantic Color Abbreviations

The engine uses canonical 3-letter semantic color identifiers. Renderers map these identifiers to active terminal themes (`DiffuseTheme`).

| Code | Semantic Category | Typical Highlight Description |
| :---: | :--- | :--- |
| **`opr`** | Syntax | Operators, punctuation, and structural symbols (`=`, `+`, `->`, `;`). |
| **`kwd`** | Syntax | Control flow and language keywords (`fn`, `pub`, `match`, `return`). |
| **`fun`** | Syntax | Function, method, and macro identifiers. |
| **`typ`** | Syntax | Type names, structs, traits, and primitive types. |
| **`str`** | Syntax | String literals. |
| **`chr`** | Syntax | Single-character literals (e.g. `'c'`, `'\n'`). |
| **`num`** | Syntax | Numeric literals (integers, floats, hex values). |
| **`com`** | Syntax | Source comments and unstyled doc-comment prose. |
| **`bld`** | Markdown | Bold / strong emphasis text spans. |
| **`ita`** | Markdown | Italic / emphasized text spans. |
| **`stk`** | Markdown | Strikethrough text spans. |
| **`cde`** | Markdown | Inline code spans (distinct from strings for visual separation). |
| **`lnk`** | Markdown | Link anchor text spans. |
| **`url`** | Markdown | Link destination URL / target address. |
| **`rul`** | Markdown | Horizontal thematic divider rules (`---`, `***`). |
| **`dlm`** | Markdown | Delimiters, list bullets, heading hashes (`#`), blockquote bars (`>`). |
| **`tbo`** | Markdown | Markdown table grid borders and column dividers (`\|`). |
| **`thd`** | Markdown | Markdown table header cells. |
| **`lbo`** | Markdown | Markdown link formatting brackets (`[`, `]`, `(`, `)`). |
| **`qbo`** | Markdown | Markdown blockquote vertical indent bars. |
| **`fbo`** | Markdown | Fenced code block delimiters (```` ``` ````). |

---

## 6. Structural Framing Records

Structural framing records encapsulate file boundaries, metrics, and coordinate systems.

### 6.1 `FileHeader` (`t = "file_header"`)

Emitted at the initiation of a file diff.

```json
{
  "t": "file_header",
  "path": "crates/diffuse-core/src/wire.rs",
  "oldPath": null,
  "oldMode": null,
  "newMode": null,
  "status": "modified",
  "isBinary": false,
  "oldBytes": 38619,
  "newBytes": 41205,
  "newLineCount": 1109,
  "oldLineCount": 1050,
  "hunkCount": 2
}
```

- `path`: Target file path in post-image.
- `oldPath`: Source file path in pre-image (populated on renames/copies).
- `status`: Change status: `"modified"`, `"added"`, `"deleted"`, `"renamed"`, `"copied"`, `"typechange"`.
- `oldBytes` / `newBytes`: Byte sizes of old and new file snapshots.

### 6.2 `HunkHeader` (`t = "hunk_header"`)

Emitted before the rows of each unified diff hunk.

```json
{
  "t": "hunk_header",
  "oldStart": 715,
  "oldCount": 25,
  "newStart": 715,
  "newCount": 90,
  "rowCount": 95,
  "section": "impl OutputRow"
}
```

- `oldStart`, `oldCount`: Pre-image line coordinate range.
- `newStart`, `newCount`: Post-image line coordinate range.
- `rowCount`: Exact number of `OutputRow` lines that follow under this hunk.
- `section`: Optional hunk header context banner string.

### 6.3 `FileFooter` (`t = "file_footer"`)

Emitted upon completion of a file diff to provide verified metrics, syntax fences, and attached review notes.

```json
{
  "t": "file_footer",
  "path": "crates/diffuse-core/src/wire.rs",
  "counts": {
    "adds": 75,
    "dels": 10,
    "mods": 5,
    "movs": 0,
    "notes": 2
  },
  "fences": [],
  "notes": []
}
```

- `counts`: Aggregate diff metrics across the file.
- `fences`: Identified fenced code blocks for syntax highlighting.
- `notes`: Review notes attached to coordinates within this file.

---

## 7. Full Tree Output Format (`--json`)

When invoked with `--json`, the engine emits the entire changeset as a single top-level `PipelineOutput` JSON object:

```json
{
  "branch": "main",
  "untrackedSkipped": [],
  "files": [
    {
      "path": "src/lib.rs",
      "oldPath": null,
      "oldMode": null,
      "newMode": null,
      "status": "modified",
      "isBinary": false,
      "oldBytes": 1200,
      "newBytes": 1350,
      "newLineCount": 45,
      "oldLineCount": 40,
      "hunks": [
        {
          "oldStart": 1,
          "oldCount": 10,
          "newStart": 1,
          "newCount": 15,
          "rows": [
            ["ctx", 1, 1, ["_//! Crate docs"]],
            ["add", null, 2, [["+pub mod wire;", "kwd"]]]
          ]
        }
      ],
      "counts": {
        "adds": 5,
        "dels": 0,
        "mods": 0,
        "movs": 0,
        "notes": 0
      },
      "fences": [],
      "notes": []
    }
  ]
}
```

Notice that inside `OutputHunk.rows`, lines serialize using the exact same compact positional tuples (`["add", null, 2, [...]]`) as in the streaming protocol.
