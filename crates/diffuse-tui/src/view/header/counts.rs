pub use crate::render::counts::{CountCell, count_cells, count_color};
use crate::{core::counts::FileCounts, render::ColorExt};
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

/// Format mod counts as an ANSI-highlighted string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. Returns an empty string when
/// all counts are zero.
pub fn format_counts_ansi(counts: &FileCounts, palette: &crate::render::ThemePalette) -> String {
    count_cells(counts, palette)
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let sep = if i == 0 { "" } else { " " };
            format!(
                "{sep}\x1b[1m{}{}{}\x1b[0m",
                cell.color.fg_code(),
                cell.marker,
                cell.value
            )
        })
        .collect::<String>()
}

/// Format mod counts as styled Ratatui [`Span`]s (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. At most five spans are ever
/// produced, stored inline without heap allocation. Returns an empty vec
/// when all counts are zero.
pub fn format_counts_spans(
    counts: &FileCounts,
    palette: &crate::render::ThemePalette,
) -> SmallVec<[Span<'static>; 5]> {
    let mut spans: SmallVec<[Span<'static>; 5]> = SmallVec::new();
    for (i, cell) in count_cells(counts, palette).iter().enumerate() {
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
