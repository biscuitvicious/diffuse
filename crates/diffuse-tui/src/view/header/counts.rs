pub use crate::render::counts::{CountCell, count_cells, count_color};
use crate::{core::counts::FileCounts, render::UiPalette};
use ratatui::{
    style::{Modifier, Style},
    text::Span,
};
use smallvec::SmallVec;

/// Format mod counts as a plain text string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. Returns an empty string when
/// all counts are zero.
pub fn format_counts_plain(counts: &FileCounts) -> String {
    counts
        .segments()
        .iter()
        .map(|seg| format!("{}{}", seg.marker, seg.value))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format mod counts as styled Ratatui [`Span`]s (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. At most five spans are ever
/// produced, stored inline without heap allocation. Returns an empty vec
/// when all counts are zero.
pub fn format_counts_spans(counts: &FileCounts, ui: &UiPalette) -> SmallVec<[Span<'static>; 5]> {
    let mut spans: SmallVec<[Span<'static>; 5]> = SmallVec::new();
    for (i, cell) in count_cells(counts, ui).iter().enumerate() {
        let prefix = if i == 0 { "" } else { " " };
        spans.push(Span::styled(
            format!("{prefix}{}{}", cell.marker, cell.value),
            Style::default()
                .fg(crate::view::to_color(cell.color))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}
