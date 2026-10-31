//! Proportional column squashing, box-drawing border synthesis, and table cell
//! wrapping.

use std::{borrow::Cow, cmp};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    types::{Run, VisualLine},
    wrap::push_merged,
};
use crate::width::{display_width, grapheme_width};

/// Proportionally squash column widths when total natural width exceeds the
/// available budget, maintaining at least 1 column width per column.
pub(crate) fn squash_table_widths(natural_widths: &[usize], budget: usize) -> Vec<usize> {
    let n = natural_widths.len();
    if n == 0 {
        return Vec::new();
    }
    let total_natural: usize = natural_widths.iter().sum();
    if total_natural <= budget {
        return natural_widths.to_vec();
    }
    let budget = budget.max(n);
    let float_widths: Vec<(usize, f64, f64)> = natural_widths
        .iter()
        .enumerate()
        .map(|(i, &w)| {
            let ideal = (w as f64 / total_natural as f64) * budget as f64;
            let frac = ideal - ideal.floor();
            (i, ideal, frac)
        })
        .collect();

    let mut widths = vec![1usize; n];
    let mut current_sum: usize = 0;
    for (i, ideal, _) in &float_widths {
        let w = (ideal.floor() as usize).max(1);
        widths[*i] = w;
        current_sum += w;
    }

    if current_sum < budget {
        let mut by_frac = float_widths.clone();
        by_frac.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(cmp::Ordering::Equal));
        let diff = budget - current_sum;
        for item in by_frac.iter().take(diff) {
            widths[item.0] += 1;
        }
    } else if current_sum > budget {
        while current_sum > budget {
            let mut reduced = false;
            let max_w = widths.iter().copied().max().unwrap_or(1);
            if max_w <= 1 {
                break;
            }
            for w in widths.iter_mut() {
                if *w == max_w && *w > 1 {
                    *w -= 1;
                    current_sum -= 1;
                    reduced = true;
                    if current_sum == budget {
                        break;
                    }
                }
            }
            if !reduced {
                break;
            }
        }
    }

    widths
}

/// Wrap cell runs to fit within `col_width`, breaking on word boundaries
/// where possible and falling back to grapheme hard-breaks for over-wide words.
pub(crate) fn wrap_table_cell(runs: &[Run], col_width: usize) -> Vec<Vec<Run>> {
    let col_width = col_width.max(1);
    if runs.is_empty() {
        return vec![Vec::new()];
    }
    let total_w: usize = runs.iter().map(|r| display_width(&r.text)).sum();
    if total_w <= col_width {
        return vec![runs.to_vec()];
    }

    struct Token<'a> {
        run: &'a Run,
        text: &'a str,
        is_space: bool,
        width: usize,
    }
    let mut tokens: Vec<Token<'_>> = Vec::new();
    for run in runs {
        let s = &run.text;
        let mut char_indices = s.char_indices().peekable();
        while let Some((start, c)) = char_indices.next() {
            let is_space = c.is_whitespace();
            let mut end = start + c.len_utf8();
            while let Some(&(_, next_c)) = char_indices.peek() {
                if next_c.is_whitespace() == is_space {
                    char_indices.next();
                    end += next_c.len_utf8();
                } else {
                    break;
                }
            }
            let text = &s[start..end];
            tokens.push(Token {
                run,
                text,
                is_space,
                width: display_width(text),
            });
        }
    }

    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut current_width = 0usize;

    for token in tokens {
        if token.is_space {
            if current_width == 0 {
                // Drop leading whitespace at start of a wrapped line.
                continue;
            }
            if current_width + token.width <= col_width {
                current_width += token.width;
                push_merged(
                    lines.last_mut().unwrap(),
                    Run {
                        color: token.run.color,
                        sgr: token.run.sgr,
                        underline: token.run.underline,
                        strike: token.run.strike,
                        dim: token.run.dim,
                        text: Cow::Owned(token.text.to_string()),
                    },
                );
            } else {
                lines.push(Vec::new());
                current_width = 0;
            }
        } else if current_width + token.width <= col_width {
            current_width += token.width;
            push_merged(
                lines.last_mut().unwrap(),
                Run {
                    color: token.run.color,
                    sgr: token.run.sgr,
                    underline: token.run.underline,
                    strike: token.run.strike,
                    dim: token.run.dim,
                    text: Cow::Owned(token.text.to_string()),
                },
            );
        } else if current_width > 0 {
            lines.push(Vec::new());
            current_width = 0;
            if token.width <= col_width {
                current_width = token.width;
                push_merged(
                    lines.last_mut().unwrap(),
                    Run {
                        color: token.run.color,
                        sgr: token.run.sgr,
                        underline: token.run.underline,
                        strike: token.run.strike,
                        dim: token.run.dim,
                        text: Cow::Owned(token.text.to_string()),
                    },
                );
            } else {
                for (g, gw) in token.text.graphemes(true).map(|g| (g, grapheme_width(g))) {
                    if current_width + gw > col_width && current_width > 0 {
                        lines.push(Vec::new());
                        current_width = 0;
                    }
                    current_width += gw;
                    push_merged(
                        lines.last_mut().unwrap(),
                        Run {
                            color: token.run.color,
                            sgr: token.run.sgr,
                            underline: token.run.underline,
                            strike: token.run.strike,
                            dim: token.run.dim,
                            text: Cow::Owned(g.to_string()),
                        },
                    );
                }
            }
        } else {
            for (g, gw) in token.text.graphemes(true).map(|g| (g, grapheme_width(g))) {
                if current_width + gw > col_width && current_width > 0 {
                    lines.push(Vec::new());
                    current_width = 0;
                }
                current_width += gw;
                push_merged(
                    lines.last_mut().unwrap(),
                    Run {
                        color: token.run.color,
                        sgr: token.run.sgr,
                        underline: token.run.underline,
                        strike: token.run.strike,
                        dim: token.run.dim,
                        text: Cow::Owned(g.to_string()),
                    },
                );
            }
        }
    }

    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// Unicode box-drawing separator sized to the column widths.
///
/// `chars` defines `[left, bar, joint, right]`.
pub(crate) fn table_rule(
    widths: &[usize],
    chars: [char; 4],
    indent: usize,
    bars: usize,
    border: &dyn Fn(Cow<'static, str>) -> Run,
) -> VisualLine {
    let [left, bar, joint, right] = chars;
    let mut text = String::new();
    text.push(left);
    for (i, w) in widths.iter().enumerate() {
        for _ in 0..w + 2 {
            text.push(bar);
        }
        text.push(if i + 1 == widths.len() { right } else { joint });
    }
    VisualLine {
        bars,
        indent,
        marker: Vec::new(),
        content: vec![border(Cow::Owned(text))],
    }
}
