//! Grapheme-boundary visual wrapping, run merging, and line flattening.

use std::borrow::Cow;

use unicode_segmentation::UnicodeSegmentation;

use diffuse_syntax::color::TokenColor;

use super::types::{BARS, MarkdownLine, MarkdownRun, Run, VisualLine, owned_spaces};
use crate::width::grapheme_width;

/// Hard-wrap content runs at grapheme boundaries into lines fitting
/// `first_width`/`cont_width`. Run styling survives splits via `Cow::Owned`
/// pieces; unsplit runs keep exact source slices.
pub(crate) fn wrap_content(runs: Vec<Run>, first_width: usize, cont_width: usize) -> Vec<Vec<Run>> {
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
                strike: run.strike,
                dim: run.dim,
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
pub(crate) fn push_merged(line: &mut Vec<Run>, run: Run) {
    if let Some(last) = line.last_mut()
        && last.color == run.color
        && last.sgr == run.sgr
        && last.underline == run.underline
        && last.strike == run.strike
        && last.dim == run.dim
    {
        last.text.to_mut().push_str(&run.text);
        return;
    }
    line.push(run);
}

/// Flatten a structural line: bars, then indent (skipped when the line is
/// empty, so blanks stay clean and quotes render bars-only), then marker
/// and content.
pub(crate) fn flatten_line(line: VisualLine) -> MarkdownLine {
    let mut runs: Vec<MarkdownRun> = Vec::new();
    if line.bars > 0 {
        let bytes = line.bars * 3;
        let text = if bytes <= BARS.len() {
            Cow::Borrowed(&BARS[..bytes])
        } else {
            Cow::Owned("▎".repeat(line.bars))
        };
        runs.push(MarkdownRun {
            color: TokenColor::QuoteBorder,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
            text,
        });
    }
    let has_content = !line.content.is_empty() || !line.marker.is_empty();
    if has_content && line.indent > 0 {
        let text = owned_spaces(line.indent);
        runs.push(MarkdownRun {
            color: TokenColor::Default,
            sgr: None,
            underline: false,
            strike: false,
            dim: false,
            text,
        });
    }
    for run in line.marker.into_iter().chain(line.content) {
        runs.push(MarkdownRun {
            color: run.color,
            sgr: run.sgr,
            underline: run.underline,
            strike: run.strike,
            dim: run.dim,
            text: run.text,
        });
    }
    MarkdownLine { runs }
}
