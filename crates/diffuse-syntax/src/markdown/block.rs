//! Block-level Markdown document AST and event reduction.

use std::{borrow::Cow, mem, ops::Range};

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use super::{
    inline::{
        Inline, InlineBuilder, TaskState, coalesce_text_events, cow_from, starts_with_task_marker,
    },
    md_options,
};

/// A flow of sibling Markdown blocks — the children of a [`ListItem`] or
/// [`Quote`](Block::Quote), and the top-level document. Lists hold
/// `Vec<ListItem>` directly, locking the items-only contract at the type
/// level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flow<'a>(pub(crate) Vec<Block<'a>>);

/// A list item: marker, child blocks, and whether a blank line separated it
/// from the previous item. Loose gaps stay inside one [`Block::List`] —
/// CommonMark keeps blank-separated items in a single loose list.
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem<'a> {
    pub blank_before: bool,
    pub marker: ListMarker<'a>,
    pub children: Flow<'a>,
}

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

#[derive(Clone, Debug, PartialEq)]
pub enum ListMarker<'a> {
    Bullet,
    Numbered(Cow<'a, str>),
    None,
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
    // main walk below then only ever sees explicit paragraphs. Adjacent
    // text events are coalesced so non-standard markers (`[·]`) and
    // bracketed task markers aren't split across events. Top-level paragraph
    // lines starting with task markers are split into bulletless list items.
    let events = with_item_paragraphs(split_paragraph_task_lists(coalesce_text_events(collected)));
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

/// Split top-level paragraph blocks containing lines that start with task
/// markers (`[ ]`, `[·]`, `[x]`, `[X]`) into dedicated list items so
/// bulletless task lists do not collapse into a single joined paragraph.
fn split_paragraph_task_lists<'a>(
    events: Vec<(Event<'a>, Range<usize>)>,
) -> Vec<(Event<'a>, Range<usize>)> {
    let mut out: Vec<(Event<'a>, Range<usize>)> = Vec::with_capacity(events.len());
    let mut item_depth = 0usize;
    let mut i = 0usize;
    while i < events.len() {
        match &events[i].0 {
            Event::Start(Tag::Item) => {
                item_depth += 1;
                out.push(events[i].clone());
                i += 1;
            }
            Event::End(TagEnd::Item) => {
                item_depth = item_depth.saturating_sub(1);
                out.push(events[i].clone());
                i += 1;
            }
            Event::Start(Tag::Paragraph) if item_depth == 0 => {
                let start_range = events[i].1.clone();
                i += 1;
                let mut para_events: Vec<(Event<'a>, Range<usize>)> = Vec::new();
                while i < events.len() && !matches!(events[i].0, Event::End(TagEnd::Paragraph)) {
                    para_events.push(events[i].clone());
                    i += 1;
                }
                let end_range = if i < events.len() {
                    let r = events[i].1.clone();
                    i += 1;
                    r
                } else {
                    start_range.clone()
                };

                let mut lines: Vec<Vec<(Event<'a>, Range<usize>)>> = Vec::new();
                let mut current_line: Vec<(Event<'a>, Range<usize>)> = Vec::new();
                for ev in para_events {
                    if matches!(ev.0, Event::SoftBreak | Event::HardBreak) {
                        lines.push(mem::take(&mut current_line));
                    } else {
                        current_line.push(ev);
                    }
                }
                if !current_line.is_empty() {
                    lines.push(current_line);
                }

                let line_starts_with_task = |line: &[(Event<'a>, Range<usize>)]| -> bool {
                    match line.first() {
                        Some((Event::Text(t), _)) => starts_with_task_marker(t),
                        _ => false,
                    }
                };

                let has_task_line = lines.iter().any(|l| line_starts_with_task(l));
                if !has_task_line {
                    out.push((Event::Start(Tag::Paragraph), start_range));
                    for (li, line) in lines.into_iter().enumerate() {
                        if li > 0 {
                            out.push((Event::SoftBreak, end_range.clone()));
                        }
                        out.extend(line);
                    }
                    out.push((Event::End(TagEnd::Paragraph), end_range));
                    continue;
                }

                let mut line_idx = 0usize;
                let mut leading_lines: Vec<Vec<(Event<'a>, Range<usize>)>> = Vec::new();
                while line_idx < lines.len() && !line_starts_with_task(&lines[line_idx]) {
                    leading_lines.push(mem::take(&mut lines[line_idx]));
                    line_idx += 1;
                }

                if !leading_lines.is_empty() {
                    let lead_start = leading_lines[0]
                        .first()
                        .map(|e| e.1.start)
                        .unwrap_or(start_range.start);
                    let lead_end = leading_lines
                        .last()
                        .and_then(|l| l.last())
                        .map(|e| e.1.end)
                        .unwrap_or(start_range.end);
                    out.push((Event::Start(Tag::Paragraph), lead_start..lead_end));
                    for (li, line) in leading_lines.into_iter().enumerate() {
                        if li > 0 {
                            out.push((Event::SoftBreak, lead_start..lead_end));
                        }
                        out.extend(line);
                    }
                    out.push((Event::End(TagEnd::Paragraph), lead_start..lead_end));
                }

                let list_start = lines[line_idx]
                    .first()
                    .map(|e| e.1.start)
                    .unwrap_or(start_range.start);
                let list_end = lines
                    .last()
                    .and_then(|l| l.last())
                    .map(|e| e.1.end)
                    .unwrap_or(end_range.end);
                out.push((Event::Start(Tag::List(None)), list_start..list_end));

                while line_idx < lines.len() {
                    let mut item_lines: Vec<Vec<(Event<'a>, Range<usize>)>> =
                        vec![mem::take(&mut lines[line_idx])];
                    line_idx += 1;
                    while line_idx < lines.len() && !line_starts_with_task(&lines[line_idx]) {
                        item_lines.push(mem::take(&mut lines[line_idx]));
                        line_idx += 1;
                    }
                    let item_start = item_lines[0]
                        .first()
                        .map(|e| e.1.start)
                        .unwrap_or(list_start);
                    let item_end = item_lines
                        .last()
                        .and_then(|l| l.last())
                        .map(|e| e.1.end)
                        .unwrap_or(list_end);
                    let item_range = item_start..item_end;

                    out.push((Event::Start(Tag::Item), item_range.clone()));
                    out.push((Event::Start(Tag::Paragraph), item_range.clone()));
                    for (li, line) in item_lines.into_iter().enumerate() {
                        if li > 0 {
                            out.push((Event::SoftBreak, item_range.clone()));
                        }
                        out.extend(line);
                    }
                    out.push((Event::End(TagEnd::Paragraph), item_range.clone()));
                    out.push((Event::End(TagEnd::Item), item_range));
                }

                out.push((Event::End(TagEnd::List(false)), list_start..list_end));
            }
            _ => {
                out.push(events[i].clone());
                i += 1;
            }
        }
    }
    out
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
                            let head = item_src.trim_start();
                            if starts_with_task_marker(head) && !head.starts_with(['-', '*', '+']) {
                                ListMarker::None
                            } else {
                                ListMarker::Bullet
                            }
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
