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

Records are uniformly framed as JSON Objects (`{ ... }`) containing a type discriminator tag field `"t"`:
- `"file_header"`: File-level diff metadata header.
- `"hunk_header"`: Hunk boundary coordinates.
- `"row"`: Line-level token rows (`OutputRow`).
- `"file_footer"`: File summary statistics and review notes.

```
+----------------------------------------------------------------------------------------------------------------------+
| {"t":"file_header", "path":"src/main.rs", ...}                                                                       | -> FileHeader
|   {"t":"hunk_header", "oldStart":1, "newStart":1, ...}                                                               | -> HunkHeader
|     {"t":"row", "k":"ctx", "ol":1, "nl":1, "r":[[" ", "fn main() {"]]}                                              | -> OutputRow (Line)
|     {"t":"row", "k":"mod", "ol":2, "nl":2, "r":[["-", "    old();"], ["+", ["    new();", "kw"]]]}                  | -> OutputRow (Line)
|     {"t":"row", "k":"ctx", "ol":3, "nl":3, "r":[[" ", "}"]]}                                                        | -> OutputRow (Line)
|   {"t":"file_footer", "path":"src/main.rs", ...}                                                                     | -> FileFooter
+----------------------------------------------------------------------------------------------------------------------+
```

---

## 3. Row Record Specification (`OutputRow`)

Every line within a diff hunk is serialized as a single-letter keyed JSON dictionary record:

$$\text{RowRecord} = \{\text{"t"}: \text{"row"}, \text{"k"}: \text{kind}, \dots\}$$

### 3.1 Keyed Dictionary Schema

| Key | Field | JSON Type | Description |
| :---: | :--- | :--- | :--- |
| **`t`** | `type` | `string` | Record discriminator, always `"row"`. |
| **`k`** | `kind` | `string` | Canonical row classification: `"ctx"`, `"add"`, `"del"`, `"mod"`, `"mov"`, `"file"`, `"sep"`. |
| **`ol`** | `old_line` *(optional)* | `integer` | 1-based coordinate in pre-image file. Omitted when not present (e.g. additions). |
| **`nl`** | `new_line` *(optional)* | `integer` | 1-based coordinate in post-image file. Omitted when not present (e.g. deletions). |
| **`r`** | `runs` *(optional)* | `array` | Sequence of batched diff runs (`[sign, ...syntax_runs]`). **Omitted for blank rows** where runs are empty. |
| **`ms`** | `mov_side` *(optional)* | `string` | Move side indicator: `"src"` (origin) or `"tgt"` (destination). Omitted when not a relocation row. |
| **`ws`** | `wholesale` *(optional)* | `boolean` | `true` when subword alignment was bypassed due to low entropy/drastic rewrite. Omitted when `false`. |
| **`g`** | `group` *(optional)* | `object` | Peer reflow group identity: `{"id": <u32>, "slot": <usize>, "n": <usize>, "m": <usize>}`. Omitted when not part of an N \u2192 M reflow group. |

### 3.2 Concrete Row Examples

1. **Context Row**:
   ```json
   {"t": "row", "k": "ctx", "ol": 42, "nl": 42, "r": [[" ", "fn calculate() {"]]}
   ```
2. **Blank Context Row** *(omits `"r"`)*:
   ```json
   {"t": "row", "k": "ctx", "ol": 43, "nl": 43}
   ```
3. **Deletion Row**:
   ```json
   {"t": "row", "k": "del", "ol": 10, "r": [["-", "    let count = 0;"]]}
   ```
4. **Addition Row**:
   ```json
   {"t": "row", "k": "add", "nl": 15, "r": [["+", ["    let mut count = 0;", "kw"]]]}
   ```
5. **Modified Row with Subword Highlighting**:
   ```json
   {"t": "row", "k": "mod", "ol": 25, "nl": 27, "r": [[" ", "    let "], ["-", "old_value"], ["+", ["new_value", "ty"]], [" ", " = 1;"]]}
   ```
6. **Relocated Block Destination**:
   ```json
   {"t": "row", "k": "mov", "nl": 88, "ms": "tgt", "r": [["+", ["pub fn helper() {}", "fn"]]]}
   ```
7. **Grouped Peer Row**:
   ```json
   {"t": "row", "k": "add", "nl": 142, "r": [["+", "    additional_field: true,"]], "g": {"id": 7, "slot": 2, "n": 2, "m": 3}}
   ```

---

## 4. Batched Token Run Specification (`OutputRun` & `Batch`)

Token runs are grouped into contiguous **batches** by diff classification (`RunKind`). Each batch is serialized as a variadic JSON array where the first element is the diff category sign, followed by zero or more syntax runs:

$$[\text{sign}, \text{syntax\_run}_1, \text{syntax\_run}_2, \dots]$$

### 4.1 Diff Category Sign

The first element of every batch identifies the diff category for all contained syntax runs:

| Sign | Change Kind | Semantic Representation |
| :---: | :--- | :--- |
| `" "` | Context (`Ctx`) | Unchanged text present in both pre- and post-image. |
| `"-"` | Deletion (`Del`) | Text deleted from pre-image. |
| `"+"` | Addition (`Add`) | Text added in post-image. |
| `"<"` | Moved Source (`MovedSrc`) | Reflow-moved source text that left this line. |
| `">"` | Moved Target (`MovedTgt`) | Reflow-moved target text that entered this line. |

### 4.2 Syntax Run Representations

Individual syntax runs inside a batch carry text, syntax coloring, and optional SGR attributes without repeating the diff sign:

1. **Unstyled / Default Text** *(scalar string)*:
   $$\text{"<text>"}$$
   *Example*: `"    let x = "` or `"old_fn();"`
2. **Syntax Highlighted Run** *(2-element array)*:
   $$[\text{"<text>"}, \text{"<color>" package}]$$
   *Example*: `["struct ", "kw"]` or `["Point", "ty"]`
3. **Syntax Highlighted with SGR Emphasis** *(3-element array)*:
   $$[\text{"<text>"}, \text{"<color>" package \| null}, \text{sgr}]$$
   *Example*: `["# Architecture", "dlm", 1]` (heading with SGR 1 bold)

### 4.3 Compression & Delimiter Whitespace Absorption

During serialization, contiguous runs within each batch are automatically optimized:
1. **Style Coalescing**: Adjacent runs with identical `(color, sgr)` combine into a single text slice.
2. **Whitespace Absorption**: Trailing delimiter spaces fold directly into preceding tokens (e.g. `["let ", "kw"]`), and surrounding spaces fold into operators (e.g. `[" = ", "op"]`).
3. **Zero Overhead for Blank Lines**: Lines containing no text serialize with an empty batches handle `[]`.

---

## 5. Canonical Semantic Color Abbreviations

The engine uses canonical semantic color identifiers. Renderers map these identifiers to active terminal themes (`DiffuseTheme`).

| Code | Semantic Category | Typical Highlight Description |
| :---: | :--- | :--- |
| **`op`** | Syntax | Operators, punctuation, and structural symbols (`=`, `+`, `->`, `;`). |
| **`kw`** | Syntax | Control flow and language keywords (`fn`, `pub`, `match`, `return`). |
| **`fn`** | Syntax | Function, method, and macro identifiers. |
| **`ty`** | Syntax | Type names, structs, traits, and primitive types. |
| **`str`** | Syntax | String literals. |
| **`chr`** | Syntax | Single-character literals (e.g. `'c'`, `'\n'`). |
| **`num`** | Syntax | Numeric literals (integers, floats, hex values). |
| **`lit`** | Syntax | Literal values (`true`, `false`, `null`, `None`). |
| **`com`** | Syntax | Source comments and unstyled doc-comment prose. |
| **`bold`** | Markdown | Bold / strong emphasis text spans. |
| **`ita`** | Markdown | Italic / emphasized text spans. |
| **`stk`** | Markdown | Strikethrough text spans. |
| **`cde`** | Markdown | Inline code spans (distinct from strings for visual separation). |
| **`lnk`** | Markdown | Link anchor text spans. |
| **`url`** | Markdown | Link destination URL / target address. |
| **`rul`** | Markdown | Horizontal thematic divider rules (`---`, `***`). |
| **`dlm`** | Markdown | Delimiters, list bullets, heading hashes (`#`), blockquote bars (`>`). |
| **`tbd`** | Markdown | Markdown table grid borders and column dividers (`\|`). |
| **`thd`** | Markdown | Markdown table header cells. |
| **`lbd`** | Markdown | Markdown link formatting brackets (`[`, `]`, `(`, `)`). |
| **`qbd`** | Markdown | Markdown blockquote vertical indent bars. |
| **`fbd`** | Markdown | Fenced code block delimiters (```` ``` ````). |

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
