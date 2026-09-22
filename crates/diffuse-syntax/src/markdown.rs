//! Note-agnostic terminal Markdown classification.
//!
//! Parses Markdown into a block model ([`Block`]/[`Flow`]) and inline spans
//! ([`Span`]/[`Inline`]) without emitting source marker characters. Pure
//! classification — no width, no ANSI. The renderer consumes this.

use std::{borrow::Cow, mem, ops::Range};

use pulldown_cmark::{
    Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

/// Convert pulldown-cmark text into a [`Cow`], borrowing when the event
/// borrows from the input and allocating only for boxed/inlined text.
fn cow_from(text: CowStr<'_>) -> Cow<'_, str> {
    match text {
        CowStr::Borrowed(s) => Cow::Borrowed(s),
        CowStr::Boxed(s) => Cow::Owned(s.into_string()),
        CowStr::Inlined(s) => Cow::Owned(s.to_string()),
    }
}

/// GFM parsing options shared by block and inline parsing.
fn md_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}

/// A flow of sibling Markdown blocks — the children of a [`ListItem`] or
/// [`Quote`](Block::Quote), and the top-level document. Lists hold
/// `Vec<ListItem>` directly, locking the items-only contract at the type
/// level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flow<'a>(Vec<Block<'a>>);

/// A list item: marker, child blocks, and whether a blank line separated it
/// from the previous item. Loose gaps stay inside one [`Block::List`] —
/// CommonMark keeps blank-separated items in a single loose list.
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem<'a> {
    pub blank_before: bool,
    pub marker: ListMarker<'a>,
    pub children: Flow<'a>,
}

/// A run of sibling inline spans — paragraph and heading content, table
/// cells, link labels, and emphasis children all share this one
/// representation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline<'a>(Vec<Span<'a>>);

/// A parsed Markdown block. Variant names do not repeat `Block` — [`Quote`]
/// and [`Code`], not `BlockQuote`/`CodeBlock`.
#[derive(Clone, Debug, PartialEq)]
pub enum Block<'a> {
    /// One or more consecutive all-whitespace source lines, condensed.
    Blank,
    /// Paragraph: whitespace-normalized, joined text parsed into
    /// in-paragraph markup spans.
    Para {
        content: Inline<'a>,
    },
    /// ATX heading: level (1–6) and title content, `#` markers stripped.
    Head {
        level: u8,
        content: Inline<'a>,
    },
    /// A list: all of its items — tight or loose gaps alike — group here so
    /// the renderer can align markers across every item.
    List {
        items: Vec<ListItem<'a>>,
    },
    /// Blockquote: child blocks; a nested quote is a `Quote` child.
    Quote {
        children: Flow<'a>,
    },
    /// Fenced code: content lines (no fence markers), info string kept.
    /// Code preserves line breaks — no joining.
    Code {
        lang: Option<Cow<'a, str>>,
        lines: Vec<Cow<'a, str>>,
    },
    /// GFM table: header cells, body rows of cells sharing the `Para`
    /// content representation, and per-column alignment. The model lives in
    /// Phase 1 so renderers match exhaustively; graceful degradation
    /// (squashing) is Phase 5's job.
    Table {
        header: Vec<Inline<'a>>,
        rows: Vec<Vec<Inline<'a>>>,
        alignments: Vec<ColumnAlign>,
    },
    Rule,
}

/// In-paragraph markup. Emphasis variants nest; the renderer maps each
/// variant to theme color and SGR.
#[derive(Clone, Debug, PartialEq)]
pub enum Span<'a> {
    Text(Cow<'a, str>),
    Bold(Inline<'a>),
    Italic(Inline<'a>),
    Strike(Inline<'a>),
    /// Code span: inner text, backticks stripped.
    Code(Cow<'a, str>),
    Link {
        label: Inline<'a>,
        url: Cow<'a, str>,
    },
    Task(TaskState),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Empty,
    InProgress,
    Checked,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListMarker<'a> {
    Bullet,
    Numbered(Cow<'a, str>),
}

/// Per-column alignment of a [`Block::Table`], from the GFM delimiter row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

impl<'a> Flow<'a> {
    /// The blocks in this flow.
    pub fn blocks(&self) -> &[Block<'a>] {
        &self.0
    }

    /// Consume the flow into its blocks.
    pub fn into_blocks(self) -> Vec<Block<'a>> {
        self.0
    }

    /// Whether the flow holds no blocks.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of blocks in the flow.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'a> Inline<'a> {
    /// Build a run from spans (used to strip a leading task span).
    pub fn from_spans(spans: Vec<Span<'a>>) -> Self {
        Self(spans)
    }

    /// The spans in this run.
    pub fn spans(&self) -> &[Span<'a>] {
        &self.0
    }

    /// Consume the run into its spans.
    pub fn into_spans(self) -> Vec<Span<'a>> {
        self.0
    }

    /// Whether the run holds no spans.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of spans in the run.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Parses Markdown into a top-level block flow. Blank source runs condense
/// into one [`Block::Blank`]; blank gaps between list items stay inside the
/// [`Block::List`], setting the next item's `blank_before` (CommonMark keeps
/// blank-separated items in one loose list); blank lines inside a blockquote
/// are `Blank` children of the quote.
pub fn blocks(text: &str) -> Flow<'_> {
    let collected: Vec<(Event<'_>, Range<usize>)> = Parser::new_ext(text, md_options())
        .into_offset_iter()
        .collect();
    // Tight items and task markers carry no `Paragraph` events, so a
    // pre-pass wraps their bare inline runs in synthetic paragraphs. The
    // main walk below then only ever sees explicit paragraphs.
    let events = with_item_paragraphs(collected);
    let mut stack: Vec<Frame<'_>> = vec![Frame::Root(Vec::new())];
    let mut pos = 0usize;
    for (event, range) in events {
        let start = range.start.min(text.len());
        let end = range.end.min(text.len());
        // Sibling block ranges are sequential; parent `Start` ranges cover
        // their whole subtree, so only `End` and leaf events advance the
        // blank-scan cursor. Item trailing blanks live inside the item range
        // and are handled via `ends_blank` instead.
        let advances = !matches!(event, Event::Start(_));
        if start > pos && current_is_block_container(&stack) && gap_has_blank(text, pos, start) {
            push_blank(&mut stack);
        }
        if advances {
            pos = pos.max(end);
        }
        feed_block_event(&mut stack, event, text, start, end);
    }
    if pos < text.len()
        && current_is_block_container(&stack)
        && gap_has_blank(text, pos, text.len())
    {
        push_blank(&mut stack);
    }
    // Unwind unclosed frames (e.g. unterminated fences): flush their buffers.
    while stack.len() > 1 {
        let frame = stack.pop().expect("stack has parent frame");
        flush_frame(&mut stack, frame);
    }
    match stack.pop() {
        Some(Frame::Root(blocks)) => Flow(blocks),
        _ => Flow(Vec::new()),
    }
}

/// Parses inline markup over whitespace-normalized text into inline content,
/// emitting no source markers.
pub fn spans(text: &str) -> Inline<'_> {
    let mut builder = InlineBuilder::new();
    // Leading task markers (`[ ]`, `[x]`, `[·]`) are only recognized by
    // pulldown-cmark inside list items; scan them here so standalone
    // paragraph text classifies too.
    let mut rest = text;
    if let Some((state, tail)) = split_leading_task(rest) {
        builder.push_task(state);
        rest = tail;
    }
    let parser = Parser::new_ext(rest, md_options());
    for event in parser {
        feed_inline_event(&mut builder, event);
    }
    builder.finish()
}

fn heading_level(level: HeadingLevel) -> u8 {
    level as u8
}

/// Wrap bare inline runs directly under list items in synthetic paragraphs.
///
/// Tight items (`- one`) and task markers (`- [x] done`) carry no
/// `Paragraph` events — pulldown-cmark emits their `Text`/`TaskListMarker`
/// events straight under `Item` — so the main walk would drop them. This
/// pre-pass brackets each such run in synthetic `Start/End(Paragraph)` using
/// the first/last inline range, keeping paragraph creation in one place.
fn with_item_paragraphs<'a>(
    events: Vec<(Event<'a>, Range<usize>)>,
) -> Vec<(Event<'a>, Range<usize>)> {
    let mut out: Vec<(Event<'a>, Range<usize>)> = Vec::with_capacity(events.len());
    // Container stack: `true` marks `Item` frames. Inline frames
    // (`Emphasis`, `Link`, …) push `false` — only a bare run whose nearest
    // container is an `Item` gets wrapped.
    let mut containers: Vec<bool> = Vec::new();
    let mut synth_open = false;
    let mut synth_last: Option<Range<usize>> = None;
    let close_synth = |out: &mut Vec<(Event<'a>, Range<usize>)>,
                       synth_open: &mut bool,
                       synth_last: &mut Option<Range<usize>>| {
        if *synth_open {
            let range = synth_last.take().unwrap_or(0..0);
            out.push((Event::End(TagEnd::Paragraph), range));
            *synth_open = false;
        }
    };
    for (event, range) in events {
        match &event {
            Event::Start(tag) if is_block_start(tag) => {
                close_synth(&mut out, &mut synth_open, &mut synth_last);
                containers.push(matches!(tag, Tag::Item));
                out.push((event, range));
            }
            Event::End(TagEnd::Item) => {
                close_synth(&mut out, &mut synth_open, &mut synth_last);
                containers.pop();
                out.push((event, range));
            }
            Event::End(tag) if is_block_end(tag) => {
                // A block closing inside an item ends the bare run, but the
                // container pop belongs to the matching `Start`.
                if !matches!(tag, TagEnd::Paragraph | TagEnd::Heading { .. }) {
                    close_synth(&mut out, &mut synth_open, &mut synth_last);
                }
                if is_container_end(tag) {
                    containers.pop();
                }
                out.push((event, range));
            }
            _ if is_inline_event(&event) => {
                if !synth_open && matches!(containers.last(), Some(true)) {
                    out.push((Event::Start(Tag::Paragraph), range.clone()));
                    synth_open = true;
                }
                if synth_open {
                    synth_last = Some(range.clone());
                }
                match &event {
                    Event::Start(_) => containers.push(false),
                    Event::End(_) => {
                        // Pop only inline frames; container frames belong to
                        // their block `End` arms even on truncated input.
                        if matches!(containers.last(), Some(false)) {
                            containers.pop();
                        }
                    }
                    _ => {}
                }
                out.push((event, range));
            }
            _ => {
                out.push((event, range));
            }
        }
    }
    close_synth(&mut out, &mut synth_open, &mut synth_last);
    out
}

/// Block-level `Start` tags — anything that can parent a paragraph.
fn is_block_start(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
    )
}

/// Block-level `End` tags whose matching `Start` pushed a container.
fn is_container_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
    )
}

fn is_block_end(tag: &TagEnd) -> bool {
    is_container_end(tag) || matches!(tag, TagEnd::Item)
}

/// Inline events that may appear bare under an `Item`.
fn is_inline_event(event: &Event<'_>) -> bool {
    match event {
        Event::Start(tag) => matches!(
            tag,
            Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Superscript
                | Tag::Subscript
                | Tag::Link { .. }
                | Tag::Image { .. }
        ),
        Event::End(tag) => matches!(
            tag,
            TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Link
                | TagEnd::Image
        ),
        Event::Text(_)
        | Event::Code(_)
        | Event::Html(_)
        | Event::InlineHtml(_)
        | Event::TaskListMarker(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::FootnoteReference(_) => true,
        _ => false,
    }
}

/// Whether the source range `[gap_start, gap_end)` contains a blank line.
/// Only complete lines count: the first segment counts when the gap starts
/// at a line start, the last when it ends at a line end; middle segments are
/// always complete. Lines empty after stripping quote bars and whitespace
/// count as blank.
fn gap_has_blank(text: &str, gap_start: usize, gap_end: usize) -> bool {
    let gap = &text[gap_start..gap_end];
    if !gap.contains('\n') {
        return false;
    }
    let starts_at_line_start = gap_start == 0 || text[..gap_start].ends_with('\n');
    let ends_at_line_end = gap_end >= text.len() || text[gap_end..].starts_with('\n');
    let lines: Vec<&str> = gap.split('\n').collect();
    let mut complete: Vec<&str> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let first = i == 0;
        let last = i + 1 == lines.len();
        if first && !starts_at_line_start {
            continue;
        }
        if last && !ends_at_line_end {
            continue;
        }
        complete.push(line);
    }
    complete
        .iter()
        .any(|line| strip_container_markers(line).is_empty())
}

/// Strip blockquote bars and surrounding whitespace for blank detection.
fn strip_container_markers(line: &str) -> &str {
    let mut s = line.trim();
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s
}

/// Parse an ordered item's source number from its range head (`10. …`,
/// `3) …`). Returns the digits; the renderer appends the dot.
fn source_item_number(item_src: &str) -> Option<String> {
    let head = item_src.trim_start();
    let digits: String = head.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    match head[digits.len()..].chars().next() {
        Some('.') | Some(')') => Some(digits),
        _ => None,
    }
}

/// Whether `src` ends with a blank line (a trailing newline whose preceding
/// line is blank after marker stripping). List item ranges include their
/// trailing blanks, so this detects loose inter-item gaps.
fn range_text_ends_blank(src: &str) -> bool {
    let Some(without_final) = src.strip_suffix('\n') else {
        return false;
    };
    let last_line = without_final.rsplit('\n').next().unwrap_or("");
    strip_container_markers(last_line).is_empty() && without_final.contains('\n')
}

enum Frame<'a> {
    Root(Vec<Block<'a>>),
    Quote(Vec<Block<'a>>),
    List(ListFrame<'a>),
    Item(ItemFrame<'a>),
    Para(ParaFrame<'a>),
    Heading {
        level: u8,
        inner: InlineBuilder<'a>,
    },
    Code {
        lang: Option<Cow<'a, str>>,
        buf: String,
    },
    Table(TableFrame<'a>),
    TableCell(TableCellFrame<'a>),
    Ignored,
}

struct ListFrame<'a> {
    ordered: bool,
    next_number: u64,
    items: Vec<ListItem<'a>>,
    pending_blank: bool,
}

struct ItemFrame<'a> {
    blank_before: bool,
    marker: ListMarker<'a>,
    children: Vec<Block<'a>>,
}

struct ParaFrame<'a> {
    inner: InlineBuilder<'a>,
}

struct TableFrame<'a> {
    alignments: Vec<ColumnAlign>,
    header: Vec<Inline<'a>>,
    rows: Vec<Vec<Inline<'a>>>,
    current_row: Vec<Inline<'a>>,
}

struct TableCellFrame<'a> {
    inner: InlineBuilder<'a>,
}

fn column_align(align: Alignment) -> ColumnAlign {
    match align {
        Alignment::Left | Alignment::None => ColumnAlign::Left,
        Alignment::Center => ColumnAlign::Center,
        Alignment::Right => ColumnAlign::Right,
    }
}

fn current_is_block_container(stack: &[Frame<'_>]) -> bool {
    matches!(
        stack.last(),
        Some(Frame::Root(_) | Frame::Quote(_) | Frame::Item(_) | Frame::List(_))
    )
}

fn push_block_to_current<'a>(stack: &mut Vec<Frame<'a>>, block: Block<'a>) {
    match stack.last_mut() {
        Some(Frame::Root(blocks)) | Some(Frame::Quote(blocks)) => blocks.push(block),
        Some(Frame::Item(item)) => item.children.push(block),
        // A block directly under a list without an item: attach to the last
        // item when one exists, else drop (pulldown-cmark never emits this).
        Some(Frame::List(list)) => {
            if let Some(last) = list.items.last_mut() {
                last.children.0.push(block);
            }
        }
        _ => {}
    }
}

fn push_blank<'a>(stack: &mut Vec<Frame<'a>>) {
    match stack.last_mut() {
        Some(Frame::List(list)) => {
            list.pending_blank = true;
        }
        Some(Frame::Root(blocks)) | Some(Frame::Quote(blocks)) => {
            if !matches!(blocks.last(), Some(Block::Blank)) {
                blocks.push(Block::Blank);
            }
        }
        Some(Frame::Item(item)) if !matches!(item.children.last(), Some(Block::Blank)) => {
            item.children.push(Block::Blank);
        }
        _ => {}
    }
}

fn feed_block_event<'a>(
    stack: &mut Vec<Frame<'a>>,
    event: Event<'a>,
    source: &'a str,
    ev_start: usize,
    ev_end: usize,
) {
    match event {
        Event::Start(tag) => match tag {
            Tag::Paragraph => stack.push(Frame::Para(ParaFrame {
                inner: InlineBuilder::new(),
            })),
            Tag::Heading { level, .. } => stack.push(Frame::Heading {
                level: heading_level(level),
                inner: InlineBuilder::new(),
            }),
            Tag::BlockQuote(_) => stack.push(Frame::Quote(Vec::new())),
            Tag::List(start) => stack.push(Frame::List(ListFrame {
                ordered: start.is_some(),
                next_number: start.unwrap_or(1),
                items: Vec::new(),
                pending_blank: false,
            })),
            Tag::Item => {
                // Ordered markers keep their source numbers (`1.` … `10.`),
                // parsed from the item range (pulldown-cmark only reports
                // the list's start number); the counter is a fallback.
                let item_src = &source[ev_start.min(source.len())..source.len()];
                let (marker, blank_before) = match stack.last_mut() {
                    Some(Frame::List(list)) => {
                        let blank_before = list.pending_blank;
                        list.pending_blank = false;
                        let marker = if list.ordered {
                            match source_item_number(item_src) {
                                Some(n) => ListMarker::Numbered(Cow::Owned(n)),
                                None => {
                                    let n = list.next_number;
                                    list.next_number += 1;
                                    ListMarker::Numbered(Cow::Owned(n.to_string()))
                                }
                            }
                        } else {
                            ListMarker::Bullet
                        };
                        (marker, blank_before && !list.items.is_empty())
                    }
                    _ => (ListMarker::Bullet, false),
                };
                stack.push(Frame::Item(ItemFrame {
                    blank_before,
                    marker,
                    children: Vec::new(),
                }));
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let tag = info.split_whitespace().next().unwrap_or("");
                        if tag.is_empty() {
                            None
                        } else {
                            Some(Cow::Owned(tag.to_string()))
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                stack.push(Frame::Code {
                    lang,
                    buf: String::new(),
                });
            }
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } => {
                // Inline inside a paragraph/heading/cell: forward to its
                // builder.
                feed_inline_start(stack, tag);
            }
            Tag::Table(alignments) => stack.push(Frame::Table(TableFrame {
                alignments: alignments.iter().map(|a| column_align(*a)).collect(),
                header: Vec::new(),
                rows: Vec::new(),
                current_row: Vec::new(),
            })),
            // The header row needs no frame: its cells land directly under
            // `TableHead` and `End(TableHead)` moves them into the header.
            Tag::TableHead => {}
            Tag::TableRow => {
                if let Some(Frame::Table(table)) = stack.last_mut() {
                    table.current_row.clear();
                }
            }
            Tag::TableCell => stack.push(Frame::TableCell(TableCellFrame {
                inner: InlineBuilder::new(),
            })),
            _ => stack.push(Frame::Ignored),
        },
        Event::End(tag) => match tag {
            TagEnd::Paragraph => {
                if let Some(Frame::Para(frame)) = stack.pop() {
                    let content = frame.inner.finish();
                    if !content.is_empty() {
                        push_block_to_current(stack, Block::Para { content });
                    }
                }
            }
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading { level, inner }) = stack.pop() {
                    let content = inner.finish();
                    if !content.is_empty() {
                        push_block_to_current(stack, Block::Head { level, content });
                    }
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(Frame::Quote(children)) = stack.pop() {
                    push_block_to_current(
                        stack,
                        Block::Quote {
                            children: Flow(children),
                        },
                    );
                }
            }
            TagEnd::List(_) => {
                if let Some(Frame::List(list)) = stack.pop() {
                    if !list.items.is_empty() {
                        push_block_to_current(stack, Block::List { items: list.items });
                    }
                    // A trailing blank inside the last item separates the
                    // list from the following block.
                    if list.pending_blank {
                        push_blank(stack);
                    }
                }
            }
            TagEnd::Item => {
                // Bare inline runs were wrapped in synthetic paragraphs by
                // the pre-pass, so the item top holds only child blocks.
                if let Some(Frame::Item(item)) = stack.pop() {
                    let list_item = ListItem {
                        blank_before: item.blank_before,
                        marker: item.marker,
                        children: Flow(item.children),
                    };
                    // Item ranges include their trailing blanks: a blank at
                    // the end marks a loose gap before the next item (or the
                    // block following the list).
                    let item_src = &source[ev_start.min(source.len())..ev_end.min(source.len())];
                    let trailing_blank = range_text_ends_blank(item_src);
                    match stack.last_mut() {
                        Some(Frame::List(list)) => {
                            list.items.push(list_item);
                            if trailing_blank {
                                list.pending_blank = true;
                            }
                        }
                        Some(Frame::Root(blocks)) | Some(Frame::Quote(blocks)) => {
                            // Stray item without a list: wrap as a bullet list.
                            blocks.push(Block::List {
                                items: vec![list_item],
                            });
                        }
                        Some(Frame::Item(parent)) => parent.children.push(Block::List {
                            items: vec![list_item],
                        }),
                        _ => {}
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { lang, buf }) = stack.pop() {
                    let lines = buf.lines().map(|l| Cow::Owned(l.to_string())).collect();
                    push_block_to_current(stack, Block::Code { lang, lines });
                }
            }
            TagEnd::TableHead => {
                // The header row carries no `TableRow`: its cells land
                // directly under `TableHead`.
                if let Some(Frame::Table(table)) = stack.last_mut() {
                    table.header = mem::take(&mut table.current_row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table(table)) = stack.last_mut() {
                    let row = mem::take(&mut table.current_row);
                    if !row.is_empty() {
                        table.rows.push(row);
                    }
                }
            }
            TagEnd::TableCell => {
                if let Some(Frame::TableCell(cell)) = stack.pop() {
                    let content = cell.inner.finish();
                    if let Some(Frame::Table(table)) = stack.last_mut() {
                        table.current_row.push(content);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table(table)) = stack.pop() {
                    push_block_to_current(
                        stack,
                        Block::Table {
                            header: table.header,
                            rows: table.rows,
                            alignments: table.alignments,
                        },
                    );
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                feed_inline_end(stack, tag);
            }
            _ => {
                if matches!(stack.last(), Some(Frame::Ignored)) {
                    stack.pop();
                }
            }
        },
        Event::Text(text) => feed_text(stack, cow_from(text)),
        Event::Code(code) => feed_code(stack, cow_from(code)),
        Event::Html(html) => feed_text(stack, cow_from(html)),
        Event::InlineHtml(html) => feed_text(stack, cow_from(html)),
        Event::TaskListMarker(checked) => {
            let state = if checked {
                TaskState::Checked
            } else {
                TaskState::Empty
            };
            feed_task(stack, state);
        }
        Event::Rule => {
            if current_is_block_container(stack) {
                push_block_to_current(stack, Block::Rule);
            }
        }
        Event::SoftBreak | Event::HardBreak => feed_break(stack),
        _ => {}
    }
}

/// Flush an unclosed frame at end of input (e.g. unterminated fences).
fn flush_frame<'a>(stack: &mut Vec<Frame<'a>>, frame: Frame<'a>) {
    match frame {
        Frame::Para(para) => {
            let content = para.inner.finish();
            if !content.is_empty() {
                push_block_to_current(stack, Block::Para { content });
            }
        }
        Frame::Heading { level, inner } => {
            let content = inner.finish();
            if !content.is_empty() {
                push_block_to_current(stack, Block::Head { level, content });
            }
        }
        Frame::Quote(children) => push_block_to_current(
            stack,
            Block::Quote {
                children: Flow(children),
            },
        ),
        Frame::List(list) => {
            if !list.items.is_empty() {
                push_block_to_current(stack, Block::List { items: list.items });
            }
        }
        Frame::Item(item) => {
            let list_item = ListItem {
                blank_before: item.blank_before,
                marker: item.marker,
                children: Flow(item.children),
            };
            match stack.last_mut() {
                Some(Frame::List(list)) => list.items.push(list_item),
                _ => push_block_to_current(
                    stack,
                    Block::List {
                        items: vec![list_item],
                    },
                ),
            }
        }
        Frame::Code { lang, buf } => {
            let lines = buf.lines().map(|l| Cow::Owned(l.to_string())).collect();
            push_block_to_current(stack, Block::Code { lang, lines });
        }
        Frame::Table(table) => push_block_to_current(
            stack,
            Block::Table {
                header: table.header,
                rows: table.rows,
                alignments: table.alignments,
            },
        ),
        Frame::TableCell(cell) => {
            let content = cell.inner.finish();
            match stack.last_mut() {
                Some(Frame::Table(table)) => table.current_row.push(content),
                _ => {
                    if !content.is_empty() {
                        push_block_to_current(stack, Block::Para { content });
                    }
                }
            }
        }
        Frame::Root(_) | Frame::Ignored => {}
    }
}

fn para_builder_mut<'a, 'b>(stack: &'b mut Vec<Frame<'a>>) -> Option<&'b mut InlineBuilder<'a>> {
    match stack.last_mut() {
        Some(Frame::Para(para)) => Some(&mut para.inner),
        Some(Frame::Heading { inner, .. }) => Some(inner),
        Some(Frame::TableCell(cell)) => Some(&mut cell.inner),
        _ => None,
    }
}

fn feed_inline_start<'a>(stack: &mut Vec<Frame<'a>>, tag: Tag<'a>) {
    let Some(builder) = para_builder_mut(stack) else {
        return;
    };
    match tag {
        Tag::Emphasis => builder.start_italic(),
        Tag::Strong => builder.start_bold(),
        Tag::Strikethrough => builder.start_strike(),
        Tag::Link { dest_url, .. } => builder.start_link(Cow::Owned(dest_url.into_string())),
        _ => {}
    }
}

fn feed_inline_end(stack: &mut Vec<Frame<'_>>, tag: TagEnd) {
    let Some(builder) = para_builder_mut(stack) else {
        return;
    };
    match tag {
        TagEnd::Emphasis => builder.end_italic(),
        TagEnd::Strong => builder.end_bold(),
        TagEnd::Strikethrough => builder.end_strike(),
        TagEnd::Link => builder.end_link(),
        _ => {}
    }
}

fn feed_text<'a>(stack: &mut Vec<Frame<'a>>, text: Cow<'a, str>) {
    if let Some(builder) = para_builder_mut(stack) {
        builder.push_text(text);
        return;
    }
    if let Some(Frame::Code { buf, .. }) = stack.last_mut() {
        buf.push_str(&text)
    }
}

fn feed_code<'a>(stack: &mut Vec<Frame<'a>>, code: Cow<'a, str>) {
    if let Some(builder) = para_builder_mut(stack) {
        builder.push_code(code);
        return;
    }
    if let Some(Frame::Code { buf, .. }) = stack.last_mut() {
        buf.push_str(&code);
    }
}

fn feed_task(stack: &mut Vec<Frame<'_>>, state: TaskState) {
    if let Some(builder) = para_builder_mut(stack) {
        builder.push_task(state);
    }
}

fn feed_break(stack: &mut Vec<Frame<'_>>) {
    if let Some(builder) = para_builder_mut(stack) {
        builder.push_break();
        return;
    }
    if let Some(Frame::Code { buf, .. }) = stack.last_mut() {
        buf.push('\n')
    }
}

fn feed_inline_event<'a>(builder: &mut InlineBuilder<'a>, event: Event<'a>) {
    match event {
        Event::Start(tag) => match tag {
            Tag::Emphasis => builder.start_italic(),
            Tag::Strong => builder.start_bold(),
            Tag::Strikethrough => builder.start_strike(),
            Tag::Link { dest_url, .. } => builder.start_link(Cow::Owned(dest_url.into_string())),
            _ => {}
        },
        Event::End(tag) => match tag {
            TagEnd::Emphasis => builder.end_italic(),
            TagEnd::Strong => builder.end_bold(),
            TagEnd::Strikethrough => builder.end_strike(),
            TagEnd::Link => builder.end_link(),
            _ => {}
        },
        Event::Text(text) => builder.push_text(cow_from(text)),
        Event::Code(code) => builder.push_code(cow_from(code)),
        Event::Html(html) | Event::InlineHtml(html) => builder.push_text(cow_from(html)),
        Event::TaskListMarker(checked) => builder.push_task(if checked {
            TaskState::Checked
        } else {
            TaskState::Empty
        }),
        Event::SoftBreak | Event::HardBreak => builder.push_break(),
        _ => {}
    }
}

/// Whether inline text needs whitespace folding: any non-space Unicode
/// whitespace, or a run of two whitespace characters.
fn needs_normalize(text: &str) -> bool {
    let mut prev_ws = false;
    for c in text.chars() {
        let ws = c.is_whitespace();
        if ws && (c != ' ' || prev_ws) {
            return true;
        }
        prev_ws = ws;
    }
    false
}

/// Fold every Unicode whitespace run into a single ASCII space, preserving
/// at most one leading/trailing space for word separation across span
/// boundaries.
fn normalize_inline_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_ws = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// Split a leading task marker (`[ ]`, `[x]`/`[X]`, `[·]`) followed by a
/// space, tab, or end of text from paragraph text.
fn split_leading_task(text: &str) -> Option<(TaskState, &str)> {
    let (marker_len, state) = if text.starts_with("[ ]") {
        (3, TaskState::Empty)
    } else if text.starts_with("[x]") || text.starts_with("[X]") {
        (3, TaskState::Checked)
    } else if text.starts_with("[·]") {
        ("[·]".len(), TaskState::InProgress)
    } else {
        return None;
    };
    let rest = &text[marker_len..];
    if rest.is_empty() {
        return Some((state, ""));
    }
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some((state, rest.trim_start_matches(|c: char| c.is_whitespace())))
    } else {
        None
    }
}

struct InlineBuilder<'a> {
    stack: Vec<InlineFrame<'a>>,
}

struct InlineFrame<'a> {
    kind: FrameKind,
    children: Vec<Span<'a>>,
    url: Option<Cow<'a, str>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Root,
    Bold,
    Italic,
    Strike,
    Link,
}

impl<'a> InlineBuilder<'a> {
    fn new() -> Self {
        Self {
            stack: vec![InlineFrame {
                kind: FrameKind::Root,
                children: Vec::new(),
                url: None,
            }],
        }
    }

    fn current_is_empty_root(&self) -> bool {
        self.stack.len() == 1 && self.stack[0].children.is_empty()
    }

    fn push_span(&mut self, span: Span<'a>) {
        if let Some(frame) = self.stack.last_mut() {
            frame.children.push(span);
        }
    }

    fn push_text(&mut self, text: Cow<'a, str>) {
        // First span in a paragraph may carry a non-standard task marker.
        if self.current_is_empty_root()
            && let Some((state, rest)) = split_leading_task(&text)
        {
            self.push_task(state);
            if !rest.is_empty() {
                self.push_span(Span::Text(Cow::Owned(rest.to_string())));
            }
            return;
        }
        // Explicit whitespace normalization: every Unicode whitespace run
        // inside a single `Text` event folds to one ASCII space (soft breaks
        // arrive separately and already join with one space). Clean text
        // keeps its borrow.
        let text = if needs_normalize(&text) {
            Cow::Owned(normalize_inline_whitespace(&text))
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        // Merge adjacent text spans to keep runs minimal, collapsing any
        // whitespace run re-formed at the join.
        let merged = match self.stack.last_mut().and_then(|f| f.children.last_mut()) {
            Some(Span::Text(prev)) => {
                let mut tail = text.as_ref();
                if prev.ends_with(' ') {
                    tail = tail.trim_start_matches(|c: char| c.is_whitespace());
                }
                match prev {
                    Cow::Owned(s) => s.push_str(tail),
                    Cow::Borrowed(_) => {
                        let mut joined = prev.to_string();
                        joined.push_str(tail);
                        *prev = Cow::Owned(joined);
                    }
                }
                true
            }
            _ => false,
        };
        if !merged {
            self.push_span(Span::Text(text));
        }
    }

    fn push_code(&mut self, code: Cow<'a, str>) {
        // Pulldown-cmark already strips the backtick runs; keep inner
        // spacing verbatim (no `.trim()` — `` ` x ` `` means ` x ` ).
        let inner = code.trim_matches('`');
        self.push_span(Span::Code(Cow::Owned(inner.to_string())));
    }

    fn push_task(&mut self, state: TaskState) {
        self.push_span(Span::Task(state));
    }

    fn push_break(&mut self) {
        self.push_span(Span::Text(Cow::Borrowed(" ")));
    }

    fn start_bold(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Bold,
            children: Vec::new(),
            url: None,
        });
    }

    fn end_bold(&mut self) {
        self.close_frame(FrameKind::Bold, Span::Bold);
    }

    fn start_italic(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Italic,
            children: Vec::new(),
            url: None,
        });
    }

    fn end_italic(&mut self) {
        self.close_frame(FrameKind::Italic, Span::Italic);
    }

    fn start_strike(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Strike,
            children: Vec::new(),
            url: None,
        });
    }

    fn end_strike(&mut self) {
        self.close_frame(FrameKind::Strike, Span::Strike);
    }

    fn start_link(&mut self, url: Cow<'a, str>) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Link,
            children: Vec::new(),
            url: Some(url),
        });
    }

    fn end_link(&mut self) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        if frame.kind != FrameKind::Link {
            self.stack.push(frame);
            return;
        }
        let url = frame.url.unwrap_or(Cow::Borrowed(""));
        self.push_span(Span::Link {
            label: Inline(frame.children),
            url,
        });
    }

    fn close_frame(&mut self, kind: FrameKind, wrap: fn(Inline<'a>) -> Span<'a>) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        if frame.kind != kind {
            // Unbalanced close: restore and ignore.
            self.stack.push(frame);
            return;
        }
        self.push_span(wrap(Inline(frame.children)));
    }

    fn finish(mut self) -> Inline<'a> {
        // Close any unclosed emphasis frames inside-out.
        while self.stack.len() > 1 {
            let Some(frame) = self.stack.pop() else {
                break;
            };
            let span = match frame.kind {
                FrameKind::Bold => Span::Bold(Inline(frame.children)),
                FrameKind::Italic => Span::Italic(Inline(frame.children)),
                FrameKind::Strike => Span::Strike(Inline(frame.children)),
                FrameKind::Link => Span::Link {
                    label: Inline(frame.children),
                    url: frame.url.unwrap_or(Cow::Borrowed("")),
                },
                FrameKind::Root => continue,
            };
            self.push_span(span);
        }
        Inline(self.stack.pop().map(|f| f.children).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests;
