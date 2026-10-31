//! Inline span model, whitespace normalization, and event accumulation.

use std::{borrow::Cow, ops::Range};

use pulldown_cmark::{CowStr, Event, Parser, Tag, TagEnd};

use super::md_options;

/// Convert pulldown-cmark text into a [`Cow`], borrowing when the event
/// borrows from the input and allocating only for boxed/inlined text.
pub(crate) fn cow_from(text: CowStr<'_>) -> Cow<'_, str> {
    match text {
        CowStr::Borrowed(s) => Cow::Borrowed(s),
        CowStr::Boxed(s) => Cow::Owned(s.into_string()),
        CowStr::Inlined(s) => Cow::Owned(s.to_string()),
    }
}

/// A run of sibling inline spans — paragraph and heading content, table
/// cells, link labels, and emphasis children all share this one
/// representation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline<'a>(pub(crate) Vec<Span<'a>>);

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
    let collected: Vec<(Event<'_>, Range<usize>)> = Parser::new_ext(rest, md_options())
        .into_offset_iter()
        .collect();
    for (event, _) in coalesce_text_events(collected) {
        feed_inline_event(&mut builder, event);
    }
    builder.finish()
}

/// Coalesce adjacent [`Event::Text`] events into a single event with the
/// combined range. pulldown-cmark emits separate text events around bracket
/// characters (`[`, `]`, `·`), which prevents pattern matching on task
/// markers like `[·]` or standalone `[ ]`.
pub(crate) fn coalesce_text_events<'a, I>(events: I) -> Vec<(Event<'a>, Range<usize>)>
where
    I: IntoIterator<Item = (Event<'a>, Range<usize>)>,
{
    let mut out: Vec<(Event<'a>, Range<usize>)> = Vec::new();
    for (event, range) in events {
        match event {
            Event::Text(text) => {
                if let Some((Event::Text(prev), prev_range)) = out.last_mut() {
                    let mut s = prev.to_string();
                    s.push_str(&text);
                    *prev = CowStr::Boxed(s.into_boxed_str());
                    prev_range.end = range.end;
                } else {
                    out.push((Event::Text(text), range));
                }
            }
            _ => out.push((event, range)),
        }
    }
    out
}

/// Split a leading task marker (`[ ]`, `[x]`/`[X]`, `[·]`) optionally
/// preceded by whitespace, followed by a space, tab, or end of text from
/// paragraph text.
pub(crate) fn split_leading_task(text: &str) -> Option<(TaskState, &str)> {
    let trimmed = text.trim_start();
    let (marker_len, state) = if trimmed.starts_with("[ ]") {
        (3, TaskState::Empty)
    } else if trimmed.starts_with("[x]") || trimmed.starts_with("[X]") {
        (3, TaskState::Checked)
    } else if trimmed.starts_with("[·]") {
        ("[·]".len(), TaskState::InProgress)
    } else {
        return None;
    };
    let rest = &trimmed[marker_len..];
    if rest.is_empty() {
        return Some((state, ""));
    }
    if rest.starts_with(|c: char| c.is_whitespace()) {
        Some((state, rest.trim_start_matches(|c: char| c.is_whitespace())))
    } else {
        None
    }
}

pub(crate) fn starts_with_task_marker(text: &str) -> bool {
    split_leading_task(text).is_some()
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

pub(crate) fn feed_inline_event<'a>(builder: &mut InlineBuilder<'a>, event: Event<'a>) {
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

pub(crate) struct InlineBuilder<'a> {
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
    pub(crate) fn new() -> Self {
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

    pub(crate) fn push_span(&mut self, span: Span<'a>) {
        if let Some(frame) = self.stack.last_mut() {
            frame.children.push(span);
        }
    }

    pub(crate) fn push_text(&mut self, text: Cow<'a, str>) {
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

    pub(crate) fn push_code(&mut self, code: Cow<'a, str>) {
        // Pulldown-cmark already strips the backtick runs; keep inner
        // spacing verbatim (no `.trim()` — `` ` x ` `` means ` x ` ).
        let inner = code.trim_matches('`');
        self.push_span(Span::Code(Cow::Owned(inner.to_string())));
    }

    pub(crate) fn push_task(&mut self, state: TaskState) {
        self.push_span(Span::Task(state));
    }

    pub(crate) fn push_break(&mut self) {
        self.push_span(Span::Text(Cow::Borrowed(" ")));
    }

    pub(crate) fn start_bold(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Bold,
            children: Vec::new(),
            url: None,
        });
    }

    pub(crate) fn end_bold(&mut self) {
        self.close_frame(FrameKind::Bold, Span::Bold);
    }

    pub(crate) fn start_italic(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Italic,
            children: Vec::new(),
            url: None,
        });
    }

    pub(crate) fn end_italic(&mut self) {
        self.close_frame(FrameKind::Italic, Span::Italic);
    }

    pub(crate) fn start_strike(&mut self) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Strike,
            children: Vec::new(),
            url: None,
        });
    }

    pub(crate) fn end_strike(&mut self) {
        self.close_frame(FrameKind::Strike, Span::Strike);
    }

    pub(crate) fn start_link(&mut self, url: Cow<'a, str>) {
        self.stack.push(InlineFrame {
            kind: FrameKind::Link,
            children: Vec::new(),
            url: Some(url),
        });
    }

    pub(crate) fn end_link(&mut self) {
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

    pub(crate) fn finish(mut self) -> Inline<'a> {
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
