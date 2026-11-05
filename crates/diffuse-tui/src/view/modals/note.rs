//! Review note editor modal widget dialog.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear},
};

use super::geometry::centered_rect;
use crate::render::DiffuseTheme;

/// Render the Note Draft modal overlay with multi-line support and exact range
/// coordinates.
pub fn render_note_modal_widget(
    f: &mut Frame,
    loc: &str,
    draft: &str,
    editor: Option<&mut crate::editor::Editor>,
    theme: &DiffuseTheme,
    area: Rect,
) {
    let p = &theme.palette.ui;
    let margin_w = if area.width <= 60 { 2 } else { 4 };
    let modal_w = area.width.saturating_sub(margin_w).min(88);

    let min_h = 7.min(area.height);
    let max_h = area.height.saturating_sub(2).max(min_h);

    let inner_w = modal_w.saturating_sub(2).max(1) as usize;

    let mut fallback_editor;
    let ed = match editor {
        Some(e) => {
            e.set_width(inner_w);
            e
        }
        None => {
            fallback_editor = crate::editor::Editor::new(draft);
            fallback_editor.set_width(inner_w);
            let r = fallback_editor.lines().len().saturating_sub(1);
            let c = fallback_editor.lines()[r].chars().count();
            fallback_editor.set_cursor(r, c);
            &mut fallback_editor
        }
    };

    let total_visual_rows: usize = ed
        .lines()
        .iter()
        .map(|l| crate::editor::Editor::line_seg_count(l, inner_w))
        .sum();
    let modal_h = (total_visual_rows as u16 + 6).clamp(min_h, max_h);
    let modal_area = centered_rect(modal_w, modal_h, area);

    f.render_widget(Clear, modal_area);

    let title = if modal_w < 50 {
        format!(" NOTE on {loc} · Ctrl+S · Esc ")
    } else {
        format!(" NOTE on {loc} · Ctrl+S Save · Enter ↵ · Esc Cancel ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(
            Style::default()
                .fg(crate::view::to_color(p.warning))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(crate::view::to_color(p.modal_border)))
        .style(
            Style::default()
                .bg(crate::view::to_color(p.bg_modal))
                .fg(crate::view::to_color(p.modal_text)),
        );

    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    ed.render(f, inner);
}
