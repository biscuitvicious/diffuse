//! Pluggable syntax highlighting, span slicing, and Ratatui frame rendering.

use std::sync::Arc;

use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::buffer::Editor;
use crate::{
    render::DiffuseTheme,
    view::{ansi_to_ratatui_line, to_color},
};

pub type HighlightingCallback = Arc<dyn Fn(&[String]) -> Vec<Vec<Span<'static>>> + Send + Sync>;

impl Editor {
    /// Renders the editor into `area`, handling viewport scrolling and
    /// positioning the terminal's hardware cursor.
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = area.width as usize;
        let height = area.height as usize;
        self.set_width(width);

        let mut all_vis_lines: Vec<Line<'static>> = Vec::new();
        let mut cursor_vis_y = 0usize;
        let mut cursor_vis_x = 0usize;
        let mut total_vis_rows = 0usize;

        let all_spans = if let Some(highlighter) = &self.highlighter {
            if let Some(ref cached) = self.cached_highlight {
                cached.clone()
            } else {
                let computed = highlighter(&self.lines);
                self.cached_highlight = Some(computed.clone());
                computed
            }
        } else {
            Vec::new()
        };

        for (l_idx, line) in self.lines.iter().enumerate() {
            let spans = all_spans
                .get(l_idx)
                .cloned()
                .unwrap_or_else(|| vec![Span::raw(line.to_string())]);

            let seg_count = Self::line_seg_count(line, width);
            if l_idx == self.cursor_row {
                let disp_col = Self::cursor_display_col(line, self.cursor_col);
                let vis_seg = (disp_col / width).min(seg_count.saturating_sub(1));
                let vis_col = disp_col.saturating_sub(vis_seg * width);
                cursor_vis_y = total_vis_rows + vis_seg;
                cursor_vis_x = vis_col;
            }

            for s_idx in 0..seg_count {
                let seg_spans = slice_spans(&spans, s_idx * width, width);
                all_vis_lines.push(Line::from(seg_spans));
            }
            total_vis_rows += seg_count;
        }

        // Auto-scroll viewport to keep cursor visible
        if cursor_vis_y < self.scroll_row {
            self.scroll_row = cursor_vis_y;
        } else if cursor_vis_y >= self.scroll_row + height {
            self.scroll_row = cursor_vis_y.saturating_sub(height - 1);
        }

        // Render visible visual lines
        for (v_offset, vis_idx) in (self.scroll_row..self.scroll_row + height).enumerate() {
            if let Some(line) = all_vis_lines.get(vis_idx) {
                let row_area = Rect::new(area.x, area.y + v_offset as u16, area.width, 1);
                f.render_widget(Paragraph::new(line.clone()), row_area);
            }
        }

        // Set native hardware cursor position
        if cursor_vis_y >= self.scroll_row && cursor_vis_y < self.scroll_row + height {
            let screen_x = area.x + (cursor_vis_x as u16).min(area.width.saturating_sub(1));
            let screen_y = area.y + (cursor_vis_y - self.scroll_row) as u16;
            f.set_cursor_position(Position {
                x: screen_x,
                y: screen_y,
            });
        }
    }
}

/// Creates a pluggable syntax highlighting callback that formats Markdown text
/// using diffuse's standard Markdown classifier and theme across the full
/// document.
pub fn markdown_highlighter(theme: DiffuseTheme) -> HighlightingCallback {
    Arc::new(move |lines: &[String]| {
        let doc = lines.join("\n");
        let doc_tokens = diffuse_syntax::classifier::markdown_document_tokens(&doc);
        let mut out = Vec::with_capacity(lines.len());
        for line_tokens in doc_tokens {
            let mut ansi_buf = String::new();
            for tok in line_tokens {
                theme.write_token(tok.color, tok.text, "ctx", None, tok.sgr, &mut ansi_buf);
            }
            let ratatui_line = ansi_to_ratatui_line(&ansi_buf);
            out.push(ratatui_line.spans);
        }
        while out.len() < lines.len() {
            out.push(vec![Span::styled(
                lines[out.len()].clone(),
                ratatui::style::Style::default().fg(to_color(theme.scoped_palette().ui.modal_text)),
            )]);
        }

        out
    })
}

/// Slices styled `Span`s by visual display column boundaries
/// `start_col..start_col + max_cols`.
pub(crate) fn slice_spans(
    spans: &[Span<'static>],
    start_col: usize,
    max_cols: usize,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut cur_col = 0usize;
    let seg_end = start_col + max_cols;

    for span in spans {
        let span_w = crate::render::width::display_width(&span.content);
        let span_start = cur_col;
        let span_end = cur_col + span_w;
        cur_col = span_end;

        if span_end <= start_col || span_start >= seg_end {
            continue;
        }

        let s = start_col.max(span_start);
        let e = seg_end.min(span_end);
        let piece = crate::render::slice_by_visual_width(
            &span.content,
            s.saturating_sub(span_start),
            e.saturating_sub(s),
        );
        if !piece.is_empty() {
            out.push(Span::styled(piece, span.style));
        }
    }
    out
}
