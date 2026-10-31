//! Flow and block rendering dispatch for headings, lists, quotes, tables, and
//! rules.

use std::borrow::Cow;

use diffuse_syntax::color::TokenColor;

use super::{
    inline::{inline_runs, task_glyph},
    table::{squash_table_widths, table_rule, wrap_table_cell},
    types::{InlineStyle, Run, VisualLine, borrowed_spaces, owned_spaces},
    wrap::wrap_content,
};
use crate::{
    syntax::{
        classifier::for_each_line_classified_token,
        markdown::{Block, ColumnAlign, Inline, ListMarker, Span},
        profile::SGR_BOLD,
        registry::lang_profile,
    },
    width::display_width,
};

pub(crate) struct Renderer {
    pub(crate) full_width: usize,
}

impl Renderer {
    /// Available content columns after bars and indent.
    pub(crate) fn avail(&self, indent: usize, bars: usize) -> usize {
        self.full_width.saturating_sub(indent + bars).max(1)
    }

    pub(crate) fn render_blocks(
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
                            color: TokenColor::Default,
                            sgr: None,
                            underline: false,
                            strike: false,
                            dim: false,
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
                            color: TokenColor::Default,
                            sgr: Some(SGR_BOLD),
                            // Heading text — markers excluded — underlines
                            // on top of bold via stacked SGR emission.
                            underline: true,
                            strike: false,
                            dim: false,
                        },
                    );
                    let avail = self.avail(indent, bars).saturating_sub(marker_width).max(1);
                    let mut first = true;
                    for piece in wrap_content(title, avail, avail) {
                        let mut marker = Vec::new();
                        let line_indent = if first {
                            marker.push(Run {
                                color: TokenColor::Delimiter,
                                sgr: None,
                                underline: false,
                                strike: false,
                                dim: false,
                                text: Cow::Owned(hashes.clone()),
                            });
                            marker.push(Run {
                                color: TokenColor::Default,
                                sgr: None,
                                underline: false,
                                strike: false,
                                dim: false,
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
                                strike: false,
                                dim: false,
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
                            color: TokenColor::Rule,
                            sgr: Some(SGR_BOLD),
                            underline: false,
                            strike: false,
                            dim: false,
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
    pub(crate) fn flush_blank(
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
    pub(crate) fn render_list(
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
                    color: TokenColor::Delimiter,
                    sgr: None,
                    underline: false,
                    strike: false,
                    dim: false,
                    text: marker_text,
                });
                marker.push(Run {
                    color: TokenColor::Default,
                    sgr: None,
                    underline: false,
                    strike: false,
                    dim: false,
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
                    color: TokenColor::Delimiter,
                    sgr: None,
                    underline: false,
                    strike: false,
                    dim: false,
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

    /// Column-aligned table render with proportional squashing and multi-line
    /// cell wrapping when content overflows the available budget.
    pub(crate) fn render_table(
        &self,
        header: &[Inline<'_>],
        rows: &[Vec<Inline<'_>>],
        alignments: &[ColumnAlign],
        indent: usize,
        bars: usize,
    ) -> Vec<VisualLine> {
        let base = InlineStyle {
            color: TokenColor::Default,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
        };
        let head_base = InlineStyle {
            color: TokenColor::TableHeader,
            sgr: Some(SGR_BOLD),
            underline: true,
            strike: false,
            dim: false,
        };
        let head_runs: Vec<Vec<Run>> = header.iter().map(|c| inline_runs(c, head_base)).collect();
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
        let mut natural_widths = vec![1usize; cols];
        for (i, cell) in head_runs.iter().enumerate().take(cols) {
            natural_widths[i] = natural_widths[i].max(run_width(cell));
        }
        for row in &body_runs {
            for (i, cell) in row.iter().enumerate().take(cols) {
                natural_widths[i] = natural_widths[i].max(run_width(cell));
            }
        }

        let avail = self.avail(indent, bars);
        let overhead = 3 * cols + 1;
        let budget = avail.saturating_sub(overhead).max(cols);
        let widths = squash_table_widths(&natural_widths, budget);

        let align = |i: usize| {
            if i < alignments.len() {
                alignments[i]
            } else {
                ColumnAlign::Left
            }
        };
        let mut out = Vec::new();
        let border = |text: Cow<'static, str>| Run {
            color: TokenColor::TableBorder,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
            text,
        };

        // Top border.
        out.push(table_rule(
            &widths,
            ['┌', '─', '┬', '┐'],
            indent,
            bars,
            &border,
        ));

        let render_row = |cells: &[Vec<Run>], out: &mut Vec<VisualLine>| {
            let wrapped_cells: Vec<Vec<Vec<Run>>> = (0..cols)
                .map(|i| {
                    let cell_runs = cells.get(i).map(Vec::as_slice).unwrap_or(&[]);
                    wrap_table_cell(cell_runs, widths[i])
                })
                .collect();
            let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);

            for v in 0..row_height {
                let mut content = vec![border(Cow::Borrowed("│ "))];
                for (i, col_width) in widths.iter().enumerate().take(cols) {
                    let cell_line = wrapped_cells[i].get(v).map(Vec::as_slice).unwrap_or(&[]);
                    let text_width = run_width(cell_line);
                    let pad = col_width.saturating_sub(text_width);
                    let (left, right) = match align(i) {
                        ColumnAlign::Left => (0, pad),
                        ColumnAlign::Right => (pad, 0),
                        ColumnAlign::Center => (pad / 2, pad - pad / 2),
                    };
                    if left > 0 {
                        content.push(Run {
                            color: TokenColor::Default,
                            sgr: None,
                            underline: false,
                            strike: false,
                            dim: false,
                            text: owned_spaces(left),
                        });
                    }
                    content.extend(cell_line.iter().cloned());
                    if right > 0 {
                        content.push(Run {
                            color: TokenColor::Default,
                            sgr: None,
                            underline: false,
                            strike: false,
                            dim: false,
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
                    content,
                });
            }
        };

        if !head_runs.is_empty() {
            render_row(&head_runs, &mut out);
            out.push(table_rule(
                &widths,
                ['╞', '═', '╪', '╡'],
                indent,
                bars,
                &border,
            ));
        }
        for (idx, row) in body_runs.iter().enumerate() {
            if idx > 0 {
                out.push(table_rule(
                    &widths,
                    ['├', '─', '┼', '┤'],
                    indent,
                    bars,
                    &border,
                ));
            }
            render_row(row, &mut out);
        }
        out.push(table_rule(
            &widths,
            ['└', '─', '┴', '┘'],
            indent,
            bars,
            &border,
        ));
        out
    }
}

/// Ordered markers keep their source numbers with a dot (`1.` … `10.`);
/// bullets draw `•`.
pub(crate) fn marker_string(item: &crate::syntax::markdown::ListItem<'_>) -> Cow<'static, str> {
    match &item.marker {
        ListMarker::Bullet => Cow::Borrowed("•"),
        ListMarker::Numbered(n) => Cow::Owned(format!("{n}.")),
        ListMarker::None => Cow::Borrowed(""),
    }
}
