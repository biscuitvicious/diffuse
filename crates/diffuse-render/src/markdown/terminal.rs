//! Terminal Markdown rendering mode: renders the *styling* of Markdown
//! rather than its source as styled visual lines reflowed to a content
//! width. Note-agnostic — notes are one consumer.
//!
//! Parsing and marker-free classification live in
//! [`crate::syntax::markdown`]; this module owns width reflow, glyph
//! substitution, and ANSI stylization through [`DiffuseTheme`]. No new
//! dependencies.

use std::{borrow::Cow, fmt::Write};

use unicode_segmentation::UnicodeSegmentation;

use crate::{
    row::expand_tabs,
    syntax::{
        classifier::for_each_line_classified_token,
        markdown::{Block, ColumnAlign, Inline, ListMarker, Span, TaskState, blocks},
        profile::{MD_SGR_BOLD, MD_SGR_ITALIC, MD_SGR_STRIKE, MD_SGR_UNDERLINE},
        registry::lang_profile,
    },
    theme::DiffuseTheme,
    width::{display_width, grapheme_width},
};

/// A rendered visual line of Markdown: styled runs whose total display
/// width fits the requested content width.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownLine {
    pub runs: Vec<MarkdownRun>,
}

/// A styled run in a rendered Markdown line. Mirrors [`OutputRun`] with an
/// added `underline` flag: heading titles stack bold and underline, which a
/// single SGR slot cannot express. The writer combines them into one escape.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownRun {
    pub color: Option<Cow<'static, str>>,
    pub sgr: Option<u32>,
    pub underline: bool,
    pub text: Cow<'static, str>,
}

/// Renders Markdown text as styled visual lines reflowed to `width`.
#[must_use]
pub fn markdown_lines(text: &str, width: usize) -> Vec<MarkdownLine> {
    let width = width.max(1);
    // Tabs expanded once upfront; all downstream slicing borrows from the
    // expanded text via `Cow` (content copies to `Owned` only because
    // `OutputRun` requires `'static`).
    let expanded = expand_tabs(text, 4);
    let flow = blocks(&expanded);
    let renderer = Renderer { full_width: width };
    let mut out: Vec<VisualLine> = Vec::new();
    let mut pending_blank = false;
    renderer.render_blocks(flow.blocks(), 0, 0, &mut out, &mut pending_blank);
    out.into_iter().map(flatten_line).collect()
}

/// Writes one rendered line with the TUI prefix applied.
pub fn write_markdown_line<W: Write>(
    out: &mut W,
    line: &MarkdownLine,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> std::fmt::Result {
    out.write_str(prefix)?;
    for run in &line.runs {
        if !syntax_on {
            out.write_str(&run.text)?;
            continue;
        }
        let color = run.color.as_deref().unwrap_or("text");
        // Stacked styles (heading titles) combine into one escape;
        // single styles keep the legacy single-code path byte-identical.
        let mut codes = [0u32; 2];
        let mut count = 0usize;
        if let Some(s) = run.sgr {
            codes[count] = s;
            count += 1;
        }
        if run.underline && !codes[..count].contains(&MD_SGR_UNDERLINE) {
            codes[count] = MD_SGR_UNDERLINE;
            count += 1;
        }
        if count <= 1 {
            theme.write_token(
                color,
                &run.text,
                "ctx",
                None,
                codes[..count].first().copied(),
                out,
            );
        } else {
            theme.write_token_stacked(color, &run.text, "ctx", None, &codes[..count], out);
        }
    }
    Ok(())
}

/// `write_markdown_line` into an owned `String`.
#[must_use]
pub fn render_markdown_line(
    line: &MarkdownLine,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(prefix.len() + 64);
    let _ = write_markdown_line(&mut out, line, prefix, syntax_on, theme);
    out
}

/// Static space padding: indent runs borrow slices, never allocate.
static SPACES: &str = "                                                                ";
/// Static quote bars (`▎`, U+258E, 3 bytes each): bar runs borrow slices.
static BARS: &str = "▎▎▎▎▎▎▎▎";

/// A styled run with `'static` text: content copies (`Owned`) or static
/// pieces (`Borrowed` — bars, indents, bullets, markers). `underline` stacks
/// with `sgr` (heading titles are bold and underlined); the writer combines
/// them into one escape.
struct Run {
    color: &'static str,
    sgr: Option<u32>,
    underline: bool,
    text: Cow<'static, str>,
}

/// A visual line before flattening: bars, indent, and an optional leading
/// marker (list bullet/number, heading hashes) stay structural so parents
/// (quotes, list items) can compose without string surgery.
struct VisualLine {
    bars: usize,
    indent: usize,
    marker: Vec<Run>,
    content: Vec<Run>,
}

/// Inline style context; nested emphasis replaces color and SGR
/// (innermost span wins — a single SGR fits `OutputRun`), while `underline`
/// propagates so a whole heading title stays underlined through nested
/// emphasis.
#[derive(Clone, Copy)]
struct InlineStyle {
    color: &'static str,
    sgr: Option<u32>,
    underline: bool,
}

fn task_glyph(state: TaskState) -> &'static str {
    match state {
        TaskState::Empty => "⬜",
        TaskState::InProgress => "🔄",
        TaskState::Checked => "✅",
    }
}

fn inline_runs(inline: &Inline<'_>, base: InlineStyle) -> Vec<Run> {
    let mut runs = Vec::new();
    inline_runs_into(inline, base, &mut runs);
    runs
}

fn inline_runs_into(inline: &Inline<'_>, base: InlineStyle, runs: &mut Vec<Run>) {
    for span in inline.spans() {
        match span {
            Span::Text(text) => {
                if !text.is_empty() {
                    runs.push(Run {
                        color: base.color,
                        sgr: base.sgr,
                        underline: base.underline,
                        text: Cow::Owned(text.to_string()),
                    });
                }
            }
            Span::Bold(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: "mdBold",
                    sgr: Some(MD_SGR_BOLD),
                    underline: base.underline,
                },
                runs,
            ),
            Span::Italic(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: "mdItalic",
                    sgr: Some(MD_SGR_ITALIC),
                    underline: base.underline,
                },
                runs,
            ),
            Span::Strike(inner) => inline_runs_into(
                inner,
                InlineStyle {
                    color: "mdStrike",
                    sgr: Some(MD_SGR_STRIKE),
                    underline: base.underline,
                },
                runs,
            ),
            // Surrounding SGR stacks; the color classifies as code.
            Span::Code(code) => runs.push(Run {
                color: "syntaxString",
                sgr: base.sgr,
                underline: base.underline,
                text: Cow::Owned(code.to_string()),
            }),
            Span::Link { label, url } => {
                inline_runs_into(
                    label,
                    InlineStyle {
                        color: "mdLink",
                        sgr: Some(MD_SGR_UNDERLINE),
                        underline: base.underline,
                    },
                    runs,
                );
                runs.push(Run {
                    color: "border",
                    sgr: None,
                    underline: false,
                    text: Cow::Borrowed(" ("),
                });
                runs.push(Run {
                    color: "mdLink",
                    sgr: Some(MD_SGR_UNDERLINE),
                    underline: base.underline,
                    text: Cow::Owned(url.to_string()),
                });
                runs.push(Run {
                    color: "border",
                    sgr: None,
                    underline: false,
                    text: Cow::Borrowed(")"),
                });
            }
            Span::Task(state) => {
                let mut glyph = String::with_capacity(8);
                glyph.push_str(task_glyph(*state));
                glyph.push(' ');
                runs.push(Run {
                    color: "accent",
                    sgr: None,
                    underline: false,
                    text: Cow::Owned(glyph),
                });
            }
        }
    }
}

/// Hard-wrap content runs at grapheme boundaries into lines fitting
/// `first_width`/`cont_width`. Run styling survives splits via `Cow::Owned`
/// pieces; unsplit runs keep exact source slices.
fn wrap_content(runs: Vec<Run>, first_width: usize, cont_width: usize) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut budget = first_width.max(1);
    let mut used = 0usize;
    for run in runs {
        let graphemes: Vec<(&str, usize)> = run
            .text
            .graphemes(true)
            .map(|g| (g, grapheme_width(g)))
            .collect();
        let mut i = 0usize;
        while i < graphemes.len() {
            let remaining = budget.saturating_sub(used);
            if remaining == 0 || (graphemes[i].1 > remaining && used > 0) {
                lines.push(Vec::new());
                budget = cont_width.max(1);
                used = 0;
                continue;
            }
            let mut piece = String::new();
            let mut piece_width = 0usize;
            while i < graphemes.len() {
                let (g, gw) = graphemes[i];
                if gw > budget.saturating_sub(used + piece_width) {
                    break;
                }
                piece.push_str(g);
                piece_width += gw;
                i += 1;
            }
            if piece.is_empty() {
                // A lone grapheme wider than the budget on an empty line:
                // place it anyway rather than looping forever.
                let (g, gw) = graphemes[i];
                piece.push_str(g);
                piece_width = gw;
                i += 1;
            }
            used += piece_width;
            let piece_run = Run {
                color: run.color,
                sgr: run.sgr,
                underline: run.underline,
                text: Cow::Owned(piece),
            };
            if let Some(last) = lines.last_mut() {
                push_merged(last, piece_run);
            }
        }
    }
    lines
}

/// Append a run, merging into the previous one when the style matches.
fn push_merged(line: &mut Vec<Run>, run: Run) {
    if let Some(last) = line.last_mut()
        && last.color == run.color
        && last.sgr == run.sgr
        && last.underline == run.underline
    {
        last.text.to_mut().push_str(&run.text);
        return;
    }
    line.push(run);
}

struct Renderer {
    full_width: usize,
}

impl Renderer {
    /// Available content columns after bars and indent.
    fn avail(&self, indent: usize, bars: usize) -> usize {
        self.full_width.saturating_sub(indent + bars).max(1)
    }

    fn render_blocks(
        &self,
        blocks: &[Block<'_>],
        indent: usize,
        bars: usize,
        out: &mut Vec<VisualLine>,
        pending_blank: &mut bool,
    ) {
        for block in blocks {
            match block {
                Block::Blank => *pending_blank = true,
                Block::Para { content } => {
                    self.flush_blank(indent, bars, out, pending_blank);
                    let runs = inline_runs(
                        content,
                        InlineStyle {
                            color: "text",
                            sgr: None,
                            underline: false,
                        },
                    );
                    let avail = self.avail(indent, bars);
                    for content in wrap_content(runs, avail, avail) {
                        out.push(VisualLine {
                            bars,
                            indent,
                            marker: Vec::new(),
                            content,
                        });
                    }
                }
                Block::Head { level, content } => {
                    // Headings force exactly one blank around (collapsed,
                    // never doubled); top-level leading/trailing trims fall
                    // out of the pending machinery.
                    if !out.is_empty() {
                        *pending_blank = true;
                    }
                    self.flush_blank(indent, bars, out, pending_blank);
                    let hashes = "#".repeat((*level).min(6) as usize);
                    let marker_width = hashes.len() + 1;
                    let title = inline_runs(
                        content,
                        InlineStyle {
                            color: "text",
                            sgr: Some(MD_SGR_BOLD),
                            // Heading text — markers excluded — underlines
                            // on top of bold via stacked SGR emission.
                            underline: true,
                        },
                    );
                    let avail = self.avail(indent, bars).saturating_sub(marker_width).max(1);
                    let mut first = true;
                    for piece in wrap_content(title, avail, avail) {
                        let mut marker = Vec::new();
                        let line_indent = if first {
                            marker.push(Run {
                                color: "accent",
                                sgr: None,
                                underline: false,
                                text: Cow::Owned(hashes.clone()),
                            });
                            marker.push(Run {
                                color: "text",
                                sgr: None,
                                underline: false,
                                text: Cow::Borrowed(" "),
                            });
                            indent
                        } else {
                            indent + marker_width
                        };
                        first = false;
                        out.push(VisualLine {
                            bars,
                            indent: line_indent,
                            marker,
                            content: piece,
                        });
                    }
                    *pending_blank = true;
                }
                Block::List { items } => {
                    self.flush_blank(indent, bars, out, pending_blank);
                    self.render_list(items, indent, bars, out);
                }
                Block::Quote { children } => {
                    self.flush_blank(indent, bars, out, pending_blank);
                    // Children render with one more bar (width and bars-only
                    // blanks included); no post-increment — nesting stacks by
                    // depth alone.
                    let mut inner: Vec<VisualLine> = Vec::new();
                    let mut inner_pending = false;
                    self.render_blocks(
                        children.blocks(),
                        indent,
                        bars + 1,
                        &mut inner,
                        &mut inner_pending,
                    );
                    out.extend(inner);
                }
                Block::Code { lang, lines } => {
                    self.flush_blank(indent, bars, out, pending_blank);
                    let profile = lang.as_deref().and_then(lang_profile);
                    let avail = self.avail(indent, bars);
                    for code_line in lines {
                        let mut runs = Vec::new();
                        for_each_line_classified_token(code_line, profile, None, |t| {
                            runs.push(Run {
                                color: t.color,
                                sgr: t.sgr,
                                underline: false,
                                text: Cow::Owned(t.text.to_string()),
                            });
                        });
                        if runs.is_empty() {
                            out.push(VisualLine {
                                bars,
                                indent,
                                marker: Vec::new(),
                                content: Vec::new(),
                            });
                        } else {
                            for content in wrap_content(runs, avail, avail) {
                                out.push(VisualLine {
                                    bars,
                                    indent,
                                    marker: Vec::new(),
                                    content,
                                });
                            }
                        }
                    }
                }
                Block::Table {
                    header,
                    rows,
                    alignments,
                } => {
                    self.flush_blank(indent, bars, out, pending_blank);
                    out.extend(self.render_table(header, rows, alignments, indent, bars));
                }
                Block::Rule => {
                    if !out.is_empty() {
                        *pending_blank = true;
                    }
                    self.flush_blank(indent, bars, out, pending_blank);
                    let avail = self.avail(indent, bars);
                    out.push(VisualLine {
                        bars,
                        indent,
                        marker: Vec::new(),
                        content: vec![Run {
                            color: "mdRule",
                            sgr: Some(MD_SGR_BOLD),
                            underline: false,
                            text: Cow::Owned("─".repeat(avail)),
                        }],
                    });
                    *pending_blank = true;
                }
            }
        }
    }

    /// Emit one pending blank (bars-only inside quotes) unless at the very
    /// start — which also trims top-level leading blanks.
    fn flush_blank(
        &self,
        _indent: usize,
        bars: usize,
        out: &mut Vec<VisualLine>,
        pending_blank: &mut bool,
    ) {
        if *pending_blank {
            if !out.is_empty() {
                out.push(VisualLine {
                    bars,
                    indent: 0,
                    marker: Vec::new(),
                    content: Vec::new(),
                });
            }
            *pending_blank = false;
        }
    }

    /// Render a list: markers right-align to the widest, bullets draw `•`,
    /// `blank_before` gaps render between items (never before the first),
    /// and continuation lines align past the full marker column.
    fn render_list(
        &self,
        items: &[crate::syntax::markdown::ListItem<'_>],
        indent: usize,
        bars: usize,
        out: &mut Vec<VisualLine>,
    ) {
        let widths: Vec<usize> = items
            .iter()
            .map(|item| display_width(marker_string(item)))
            .collect();
        let widest = widths.into_iter().max().unwrap_or(0);
        let marker_col = if widest > 0 { widest + 1 } else { 0 };
        for (idx, item) in items.iter().enumerate() {
            if idx > 0 && item.blank_before {
                out.push(VisualLine {
                    bars,
                    indent: 0,
                    marker: Vec::new(),
                    content: Vec::new(),
                });
            }
            let marker_text = marker_string(item);
            let marker_width = display_width(&marker_text);
            let mut marker = Vec::new();
            if widest > 0 {
                if widest > marker_width {
                    marker.push(borrowed_spaces(widest - marker_width));
                }
                marker.push(Run {
                    color: "accent",
                    sgr: None,
                    underline: false,
                    text: marker_text,
                });
                marker.push(Run {
                    color: "text",
                    sgr: None,
                    underline: false,
                    text: Cow::Borrowed(" "),
                });
            }
            // A leading task span joins the marker chain (bullet + glyph +
            // space) so continuations indent past it; the glyph is stripped
            // from the paragraph content.
            let child_blocks = item.children.blocks();
            let task_state = match child_blocks.first() {
                Some(Block::Para { content }) => match content.spans().first() {
                    Some(Span::Task(state)) => Some(*state),
                    _ => None,
                },
                _ => None,
            };
            let glyph_col = if let Some(state) = task_state {
                display_width(task_glyph(state)) + 1
            } else {
                0
            };
            if let Some(state) = task_state {
                let mut glyph = String::with_capacity(8);
                glyph.push_str(task_glyph(state));
                glyph.push(' ');
                marker.push(Run {
                    color: "accent",
                    sgr: None,
                    underline: false,
                    text: Cow::Owned(glyph),
                });
            }
            let stripped: Vec<Block<'_>> = child_blocks
                .iter()
                .enumerate()
                .filter_map(|(i, block)| {
                    if i == 0 && task_state.is_some() {
                        match block {
                            Block::Para { content } if content.len() > 1 => Some(Block::Para {
                                content: Inline::from_spans(content.spans()[1..].to_vec()),
                            }),
                            // A task-only item leaves no content behind.
                            Block::Para { .. } => None,
                            _ => Some(block.clone()),
                        }
                    } else {
                        Some(block.clone())
                    }
                })
                .collect();
            // Children render at the content column into scratch lines so a
            // leading blank trims; the marker takes the first line.
            let mut inner: Vec<VisualLine> = Vec::new();
            let mut inner_pending = false;
            self.render_blocks(
                &stripped,
                indent + marker_col + glyph_col,
                bars,
                &mut inner,
                &mut inner_pending,
            );
            if inner.is_empty() {
                // Task-only (or blank-only) item: marker chain stands alone.
                out.push(VisualLine {
                    bars,
                    indent,
                    marker,
                    content: Vec::new(),
                });
                continue;
            }
            inner[0].indent = indent;
            inner[0].marker = marker;
            out.extend(inner);
        }
    }

    /// Simple aligned table render (fits-width only; squashing is Phase 5).
    fn render_table(
        &self,
        header: &[Inline<'_>],
        rows: &[Vec<Inline<'_>>],
        alignments: &[ColumnAlign],
        indent: usize,
        bars: usize,
    ) -> Vec<VisualLine> {
        let base = InlineStyle {
            color: "text",
            sgr: None,
            underline: false,
        };
        let head_runs: Vec<Vec<Run>> = header.iter().map(|c| inline_runs(c, base)).collect();
        let body_runs: Vec<Vec<Vec<Run>>> = rows
            .iter()
            .map(|row| row.iter().map(|c| inline_runs(c, base)).collect())
            .collect();
        let cols = head_runs
            .len()
            .max(body_runs.iter().map(Vec::len).max().unwrap_or(0))
            .max(alignments.len());
        if cols == 0 {
            return Vec::new();
        }
        let run_width = |runs: &[Run]| runs.iter().map(|r| display_width(&r.text)).sum::<usize>();
        let mut widths = vec![0usize; cols];
        for (i, cell) in head_runs.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(run_width(cell));
        }
        for row in &body_runs {
            for (i, cell) in row.iter().enumerate().take(cols) {
                widths[i] = widths[i].max(run_width(cell));
            }
        }
        let align = |i: usize| {
            if i < alignments.len() {
                alignments[i]
            } else {
                ColumnAlign::Left
            }
        };
        let mut out = Vec::new();
        let border = |text: Cow<'static, str>| Run {
            color: "border",
            sgr: None,
            underline: false,
            text,
        };
        // Top border.
        out.push(table_rule(&widths, '┌', '┬', '┐', indent, bars, &border));
        let render_row = |cells: &[Vec<Run>], out: &mut Vec<VisualLine>| {
            let mut content = vec![border(Cow::Borrowed("│ "))];
            for (i, col_width) in widths.iter().enumerate().take(cols) {
                let cell: &[Run] = cells.get(i).map(Vec::as_slice).unwrap_or(&[]);
                let text_width = run_width(cell);
                let pad = col_width.saturating_sub(text_width);
                let (left, right) = match align(i) {
                    ColumnAlign::Left => (0, pad),
                    ColumnAlign::Right => (pad, 0),
                    ColumnAlign::Center => (pad / 2, pad - pad / 2),
                };
                if left > 0 {
                    content.push(Run {
                        color: "text",
                        sgr: None,
                        underline: false,
                        text: owned_spaces(left),
                    });
                }
                content.extend(cell.iter().map(|r| Run {
                    color: r.color,
                    sgr: r.sgr,
                    underline: r.underline,
                    text: Cow::Owned(r.text.to_string()),
                }));
                if right > 0 {
                    content.push(Run {
                        color: "text",
                        sgr: None,
                        underline: false,
                        text: owned_spaces(right),
                    });
                }
                content.push(border(Cow::Borrowed(if i + 1 == cols {
                    " │"
                } else {
                    " │ "
                })));
            }
            out.push(VisualLine {
                bars,
                indent,
                marker: Vec::new(),
                // Overflow rows keep full content; Phase 5 squashes them to
                // the budget with internal cell wrapping.
                content,
            });
        };
        if !head_runs.is_empty() {
            render_row(&head_runs, &mut out);
            out.push(table_rule(&widths, '├', '┼', '┤', indent, bars, &border));
        }
        for row in &body_runs {
            render_row(row, &mut out);
        }
        out.push(table_rule(&widths, '└', '┴', '┘', indent, bars, &border));
        out
    }
}

/// Unicode box-drawing separator sized to the column widths.
fn table_rule(
    widths: &[usize],
    left: char,
    joint: char,
    right: char,
    indent: usize,
    bars: usize,
    border: &dyn Fn(Cow<'static, str>) -> Run,
) -> VisualLine {
    let mut text = String::new();
    text.push(left);
    for (i, w) in widths.iter().enumerate() {
        text.push_str(&"─".repeat(w + 2));
        text.push(if i + 1 == widths.len() { right } else { joint });
    }
    VisualLine {
        bars,
        indent,
        marker: Vec::new(),
        content: vec![border(Cow::Owned(text))],
    }
}

/// Ordered markers keep their source numbers with a dot (`1.` … `10.`);
/// bullets draw `•`.
fn marker_string(item: &crate::syntax::markdown::ListItem<'_>) -> Cow<'static, str> {
    match &item.marker {
        ListMarker::Bullet => Cow::Borrowed("•"),
        ListMarker::Numbered(n) => Cow::Owned(format!("{n}.")),
        ListMarker::None => Cow::Borrowed(""),
    }
}

/// Borrowed indent run from the static padding slice (`Owned` past 64).
fn borrowed_spaces(n: usize) -> Run {
    if n <= SPACES.len() {
        Run {
            color: "text",
            sgr: None,
            underline: false,
            text: Cow::Borrowed(&SPACES[..n]),
        }
    } else {
        Run {
            color: "text",
            sgr: None,
            underline: false,
            text: Cow::Owned(" ".repeat(n)),
        }
    }
}

fn owned_spaces(n: usize) -> Cow<'static, str> {
    if n <= SPACES.len() {
        Cow::Borrowed(&SPACES[..n])
    } else {
        Cow::Owned(" ".repeat(n))
    }
}

/// Flatten a structural line: bars, then indent (skipped when the line is
/// empty, so blanks stay clean and quotes render bars-only), then marker
/// and content.
fn flatten_line(line: VisualLine) -> MarkdownLine {
    let mut runs: Vec<MarkdownRun> = Vec::new();
    if line.bars > 0 {
        let bytes = line.bars * 3;
        let text = if bytes <= BARS.len() {
            Cow::Borrowed(&BARS[..bytes])
        } else {
            Cow::Owned("▎".repeat(line.bars))
        };
        runs.push(MarkdownRun {
            color: Some("border".into()),
            sgr: None,
            underline: false,
            text,
        });
    }
    let has_content = !line.content.is_empty() || !line.marker.is_empty();
    if has_content && line.indent > 0 {
        let text = owned_spaces(line.indent);
        runs.push(MarkdownRun {
            color: Some("text".into()),
            sgr: None,
            underline: false,
            text,
        });
    }
    for run in line.marker.into_iter().chain(line.content) {
        runs.push(MarkdownRun {
            color: Some(run.color.into()),
            sgr: run.sgr,
            underline: run.underline,
            text: run.text,
        });
    }
    MarkdownLine { runs }
}

#[cfg(test)]
mod tests;
