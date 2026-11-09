//! Line number padding and coordinate gutter prefix rendering.

use std::{
    fmt::{self, Write},
    str,
};

use super::segment::is_common_only_row;
use crate::{
    ansi::Color,
    core::{
        parse::{RowGroup, RowKind},
        wire::{ModGlyph, ModSide, MovSide, OutputRun},
    },
    span::StyledSpan,
    theme::{Cursor, DiffKind, DiffuseTheme, FG_RESET, StyledColor},
};

/// Maximum line number width in columns.
pub const MAX_LINE_NUM_WIDTH: usize = 16;

/// Default line number width for the one-shot pager.
pub const DEFAULT_PAGER_LINE_NUM_WIDTH: usize = 5;

/// Buffer length needed for formatted line numbers: up to 15 digits plus 3
/// bytes for '…'.
pub const MAX_LINE_NUM_BUF_LEN: usize = 32;

/// Format variable-width line numbers into a stack buffer without heap
/// allocation.
///
/// - `0`: returns `""` (hides line numbers).
/// - `1..=16`:
///   - If `num` fits in `width` columns, it is rendered right-aligned with
///     leading spaces.
///   - Otherwise, repeatedly strips the least significant magnitude (`/ 10`)
///     until the remainder fits in `width - 1` columns, rendering the remainder
///     followed by a trailing ellipsis (`…`).
///   - If `num` is `None`, renders `width` spaces.
pub fn format_line_num(
    num: Option<u32>,
    width: usize,
    buf: &mut [u8; MAX_LINE_NUM_BUF_LEN],
) -> &str {
    if width == 0 {
        return "";
    }
    let w = width.min(MAX_LINE_NUM_WIDTH);
    let Some(n) = num else {
        buf[..w].fill(b' ');
        return unsafe { str::from_utf8_unchecked(&buf[..w]) };
    };

    let digits = if n == 0 { 1 } else { (n.ilog10() + 1) as usize };
    if digits <= w {
        buf[..w].fill(b' ');
        let mut val = n;
        let mut idx = w - 1;
        if val == 0 {
            buf[idx] = b'0';
        } else {
            while val > 0 {
                buf[idx] = b'0' + (val % 10) as u8;
                val /= 10;
                if idx == 0 {
                    break;
                }
                idx -= 1;
            }
        }
        unsafe { str::from_utf8_unchecked(&buf[..w]) }
    } else {
        let target = w - 1;
        if target == 0 {
            buf[..3].copy_from_slice("…".as_bytes());
            unsafe { str::from_utf8_unchecked(&buf[..3]) }
        } else {
            let limit = 10_u64.pow(target as u32);
            let mut rem = n as u64;
            while rem >= limit {
                rem /= 10;
            }
            let mut val = rem;
            for idx in (0..target).rev() {
                buf[idx] = b'0' + (val % 10) as u8;
                val /= 10;
            }
            buf[target..target + 3].copy_from_slice("…".as_bytes());
            let total_bytes = target + 3;
            unsafe { str::from_utf8_unchecked(&buf[..total_bytes]) }
        }
    }
}

/// Generate plain prefix gutter with default pager line number width.
pub fn prefix_of(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
) -> String {
    prefix_of_with_mod_side_and_width(
        kind,
        mov_side,
        None,
        None,
        group,
        new_line,
        old_line,
        runs,
        DEFAULT_PAGER_LINE_NUM_WIDTH,
    )
}

/// Generate plain prefix gutter with optional expanded mod side and default
/// pager width.
#[allow(clippy::too_many_arguments)]
pub fn prefix_of_with_mod_side(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
) -> String {
    prefix_of_with_mod_side_and_width(
        kind,
        mov_side,
        mod_side,
        mod_glyph,
        group,
        new_line,
        old_line,
        runs,
        DEFAULT_PAGER_LINE_NUM_WIDTH,
    )
}

/// Generate plain prefix gutter with optional expanded mod side and specified
/// line number width.
#[allow(clippy::too_many_arguments)]
pub fn prefix_of_with_mod_side_and_width(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    line_num_width: usize,
) -> String {
    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    if line_num_width == 0 {
        if is_common_only_row(kind, runs) {
            return "  ".to_string();
        }
        if let Some(side) = mod_side {
            let glyph_str = mod_glyph
                .unwrap_or_else(|| ModGlyph::for_side(side))
                .as_str();
            return match side {
                ModSide::Del => format!("{glyph_str}-"),
                ModSide::Add => format!("{glyph_str}+"),
            };
        }
        return match group {
            Some(RowGroup::Anchor) if kind == RowKind::Mod => "~ ".to_string(),
            Some(RowGroup::Aux { .. }) => match kind {
                RowKind::Add => "~+".to_string(),
                RowKind::Del => "~-".to_string(),
                _ => "  ".to_string(),
            },
            _ => match kind {
                RowKind::Add => " +".to_string(),
                RowKind::Del => " -".to_string(),
                RowKind::Mod => " ~".to_string(),
                RowKind::Mov => {
                    if mov_side == Some(MovSide::Src) {
                        "≈-".to_string()
                    } else {
                        "≈+".to_string()
                    }
                }
                _ => "  ".to_string(),
            },
        };
    }
    if is_common_only_row(kind, runs) {
        return format!("{}  ", format_line_num(new_line, line_num_width, &mut buf));
    }
    if let Some(side) = mod_side {
        let glyph_str = mod_glyph
            .unwrap_or_else(|| ModGlyph::for_side(side))
            .as_str();
        return match side {
            ModSide::Del => format!(
                "{}{glyph_str}-",
                format_line_num(old_line, line_num_width, &mut buf)
            ),
            ModSide::Add => format!(
                "{}{glyph_str}+",
                format_line_num(new_line, line_num_width, &mut buf)
            ),
        };
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            format!("{}~ ", format_line_num(new_line, line_num_width, &mut buf))
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => format!("{}~+", format_line_num(new_line, line_num_width, &mut buf)),
            RowKind::Del => format!("{}~-", format_line_num(old_line, line_num_width, &mut buf)),
            _ => format!(
                "{}  ",
                format_line_num(new_line.or(old_line), line_num_width, &mut buf)
            ),
        },
        _ => match kind {
            RowKind::Add => format!("{} +", format_line_num(new_line, line_num_width, &mut buf)),
            RowKind::Del => format!("{} -", format_line_num(old_line, line_num_width, &mut buf)),
            RowKind::Mod => format!("{} ~", format_line_num(new_line, line_num_width, &mut buf)),
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    format!("{}≈-", format_line_num(old_line, line_num_width, &mut buf))
                } else {
                    format!("{}≈+", format_line_num(new_line, line_num_width, &mut buf))
                }
            }
            _ => format!(
                "{}  ",
                format_line_num(new_line.or(old_line), line_num_width, &mut buf)
            ),
        },
    }
}

/// Generate the span-native coordinate gutter for an arbitrary line number
/// width: line number plus kind glyph as ordered [`StyledSpan`]s riding on
/// `bg`. When `line_num_width == 0`, line numbers are omitted entirely and only
/// the 2-column kind glyphs / box borders are returned.
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix_spans_with_width(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let palette = theme.palette;
    let ctx_num = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num;
    if is_common_only_row(kind, runs) {
        if line_num_width == 0 {
            return vec![ctx_num.span("  ", bg)];
        }
        let pad = format_line_num(new_line, line_num_width, &mut buf);
        return vec![ctx_num.span(format!("{pad}  "), bg)];
    }
    let add = &palette[Cursor::Rest][DiffKind::Add].gutter;
    let del = &palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;
    if let Some(side) = mod_side {
        let glyph_str = mod_glyph
            .unwrap_or_else(|| ModGlyph::for_side(side))
            .as_str();
        return match side {
            ModSide::Del => {
                if line_num_width == 0 {
                    vec![r#mod.icon.span(glyph_str, bg), del.icon.span("-", bg)]
                } else {
                    let pad = format_line_num(old_line, line_num_width, &mut buf);
                    vec![
                        del.num.span(pad, bg),
                        r#mod.icon.span(glyph_str, bg),
                        del.icon.span("-", bg),
                    ]
                }
            }
            ModSide::Add => {
                if line_num_width == 0 {
                    vec![r#mod.icon.span(glyph_str, bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, line_num_width, &mut buf);
                    vec![
                        add.num.span(pad, bg),
                        r#mod.icon.span(glyph_str, bg),
                        add.icon.span("+", bg),
                    ]
                }
            }
        };
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            if line_num_width == 0 {
                vec![r#mod.icon.span("~ ", bg)]
            } else {
                let pad = format_line_num(new_line, line_num_width, &mut buf);
                vec![r#mod.num.span(pad, bg), r#mod.icon.span("~ ", bg)]
            }
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => {
                if line_num_width == 0 {
                    vec![r#mod.icon.span("~", bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, line_num_width, &mut buf);
                    vec![
                        add.num.span(pad, bg),
                        r#mod.icon.span("~", bg),
                        add.icon.span("+", bg),
                    ]
                }
            }
            RowKind::Del => {
                if line_num_width == 0 {
                    vec![r#mod.icon.span("~", bg), del.icon.span("-", bg)]
                } else {
                    let pad = format_line_num(old_line, line_num_width, &mut buf);
                    vec![
                        del.num.span(pad, bg),
                        r#mod.icon.span("~", bg),
                        del.icon.span("-", bg),
                    ]
                }
            }
            _ => {
                if line_num_width == 0 {
                    vec![ctx_num.span("  ", bg)]
                } else {
                    let pad = format_line_num(new_line.or(old_line), line_num_width, &mut buf);
                    vec![ctx_num.span(format!("{pad}  "), bg)]
                }
            }
        },
        _ => match kind {
            RowKind::Del => {
                if line_num_width == 0 {
                    vec![
                        StyledSpan::styled(" ", Some(del.num.color), bg),
                        del.icon.span("-", bg),
                    ]
                } else {
                    let pad = format_line_num(old_line, line_num_width, &mut buf);
                    vec![
                        del.num.span(pad, bg),
                        StyledSpan::styled(" ", Some(del.num.color), bg),
                        del.icon.span("-", bg),
                    ]
                }
            }
            RowKind::Add => {
                if line_num_width == 0 {
                    vec![StyledSpan::styled(" ", None, bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, line_num_width, &mut buf);
                    vec![add.num.span(format!("{pad} "), bg), add.icon.span("+", bg)]
                }
            }
            RowKind::Mod => {
                if line_num_width == 0 {
                    vec![StyledSpan::styled(" ", None, bg), r#mod.icon.span("~", bg)]
                } else {
                    let pad = format_line_num(new_line, line_num_width, &mut buf);
                    vec![
                        r#mod.num.span(format!("{pad} "), bg),
                        r#mod.icon.span("~", bg),
                    ]
                }
            }
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    if line_num_width == 0 {
                        vec![mov.icon.span("≈", bg), del.icon.span("-", bg)]
                    } else {
                        let pad = format_line_num(old_line, line_num_width, &mut buf);
                        vec![
                            del.num.span(pad, bg),
                            mov.icon.span("≈", bg),
                            del.icon.span("-", bg),
                        ]
                    }
                } else if line_num_width == 0 {
                    vec![mov.icon.span("≈", bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, line_num_width, &mut buf);
                    vec![
                        add.num.span(pad, bg),
                        mov.icon.span("≈", bg),
                        add.icon.span("+", bg),
                    ]
                }
            }
            _ => {
                if line_num_width == 0 {
                    vec![ctx_num.span("  ", bg)]
                } else {
                    let pad = format_line_num(new_line.or(old_line), line_num_width, &mut buf);
                    vec![ctx_num.span(format!("{pad}  "), bg)]
                }
            }
        },
    }
}

/// Generate the span-native coordinate gutter: the same
/// gutter (line number plus kind glyph) as ordered [`StyledSpan`]s riding on
/// `bg`, branching identically over groups, kinds, and move sides so the
/// ANSI and Ratatui paths cannot drift apart.
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix_spans(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
) -> Vec<StyledSpan> {
    styled_prefix_spans_with_width(
        kind,
        mov_side,
        mod_side,
        mod_glyph,
        group,
        new_line,
        old_line,
        runs,
        in_hunk,
        bg,
        theme,
        DEFAULT_PAGER_LINE_NUM_WIDTH,
    )
}

/// Generate the span-native twin of the wrapped-row continuation prefix
/// using default pager line number width.
pub fn continuation_prefix_spans(bg: Option<Color>) -> Vec<StyledSpan> {
    continuation_prefix_spans_with_width(bg, DEFAULT_PAGER_LINE_NUM_WIDTH)
}

/// Continuation prefix spans for an arbitrary line number width.
pub fn continuation_prefix_spans_with_width(
    bg: Option<Color>,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    const PADS: [&str; 17] = [
        "  ",
        "   ",
        "    ",
        "     ",
        "      ",
        "       ",
        "        ",
        "         ",
        "          ",
        "           ",
        "            ",
        "             ",
        "              ",
        "               ",
        "                ",
        "                 ",
        "                  ",
    ];
    let pad = if line_num_width <= MAX_LINE_NUM_WIDTH {
        PADS[line_num_width]
    } else {
        return vec![StyledSpan::styled(
            " ".repeat(line_num_width + 2),
            Some(Color::Fixed(242)),
            bg,
        )];
    };
    vec![StyledSpan::styled(pad, Some(Color::Fixed(242)), bg)]
}

#[inline]
fn mod_row_extends_box(mod_side: Option<ModSide>, mod_glyph: Option<ModGlyph>) -> bool {
    mod_side.is_some_and(|side| {
        !mod_glyph
            .unwrap_or_else(|| ModGlyph::for_side(side))
            .is_bottom()
    })
}

/// Continuation prefix spans taking `ModSide`, `ModGlyph`, and `line_num_width`
/// into account.
pub fn continuation_prefix_spans_with_mod_side_and_width(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    theme: &DiffuseTheme,
    bg: Option<Color>,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    if mod_row_extends_box(mod_side, mod_glyph) {
        let r#mod = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;
        if line_num_width == 0 {
            vec![
                r#mod.icon.span("│", bg),
                StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
            ]
        } else {
            const PADS: [&str; 17] = [
                "",
                " ",
                "  ",
                "   ",
                "    ",
                "     ",
                "      ",
                "       ",
                "        ",
                "         ",
                "          ",
                "           ",
                "            ",
                "             ",
                "              ",
                "               ",
                "                ",
            ];
            let pad = if line_num_width <= MAX_LINE_NUM_WIDTH {
                PADS[line_num_width]
            } else {
                return vec![
                    StyledSpan::styled(" ".repeat(line_num_width), Some(Color::Fixed(242)), bg),
                    r#mod.icon.span("│", bg),
                    StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
                ];
            };
            vec![
                StyledSpan::styled(pad, Some(Color::Fixed(242)), bg),
                r#mod.icon.span("│", bg),
                StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
            ]
        }
    } else {
        continuation_prefix_spans_with_width(bg, line_num_width)
    }
}

pub fn continuation_prefix_spans_with_mod_side(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    theme: &DiffuseTheme,
    bg: Option<Color>,
) -> Vec<StyledSpan> {
    continuation_prefix_spans_with_mod_side_and_width(
        mod_side,
        mod_glyph,
        theme,
        bg,
        DEFAULT_PAGER_LINE_NUM_WIDTH,
    )
}

/// Continuation prefix plain text taking `ModSide`, `ModGlyph`, and
/// `line_num_width` into account.
pub fn continuation_prefix_plain_with_mod_side_and_width(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    line_num_width: usize,
) -> &'static str {
    const BOX_PREFIXES: [&str; 17] = [
        "│ ",
        " │ ",
        "  │ ",
        "   │ ",
        "    │ ",
        "     │ ",
        "      │ ",
        "       │ ",
        "        │ ",
        "         │ ",
        "          │ ",
        "           │ ",
        "            │ ",
        "             │ ",
        "              │ ",
        "               │ ",
        "                │ ",
    ];
    const PLAIN_PREFIXES: [&str; 17] = [
        "  ",
        "   ",
        "    ",
        "     ",
        "      ",
        "       ",
        "        ",
        "         ",
        "          ",
        "           ",
        "            ",
        "             ",
        "              ",
        "               ",
        "                ",
        "                 ",
        "                  ",
    ];
    let extends_box = mod_row_extends_box(mod_side, mod_glyph);
    let idx = line_num_width.min(MAX_LINE_NUM_WIDTH);
    if extends_box {
        BOX_PREFIXES[idx]
    } else {
        PLAIN_PREFIXES[idx]
    }
}

/// Continuation prefix plain text taking `ModSide` and `ModGlyph` into account.
pub fn continuation_prefix_plain_with_mod_side(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
) -> &'static str {
    continuation_prefix_plain_with_mod_side_and_width(
        mod_side,
        mod_glyph,
        DEFAULT_PAGER_LINE_NUM_WIDTH,
    )
}

/// Write gutter prefix spans with the exact ANSI gutter spelling: fused
/// SGR attributes spliced into each foreground escape (as
/// [`StyledColor::fg_code`] fuses them), a mid-prefix italic-off after
/// the pad span on deletion-kind prefixes, and a single trailing
/// `FG_RESET` plus italic-off close. Both off-sequences are provable
/// no-ops post-optimization (italic is never on in prefix position), but
/// keeping them preserves pre-optimization byte-identity with
/// prefix builder. Backgrounds are never emitted: the row lead
/// background covers the prefix, as on the ANSI path.
pub fn write_gutter_prefix<W: Write>(
    out: &mut W,
    spans: &[StyledSpan],
    mid_italic_off: bool,
    trailing_italic_off: bool,
) -> fmt::Result {
    let mut emitted_fg = false;
    for (i, span) in spans.iter().enumerate() {
        // Styled prefixes always carry a foreground; plain ones (the
        // unstyled CLI path) emit bare text with no closes, exactly as
        // the ANSI path passes the plain string through untouched.
        let span_fg = span.fg.filter(|color| *color != Color::Primary);
        if let Some(fg) = span_fg {
            let styled = StyledColor {
                color: fg,
                bold: span.bold,
                italic: span.italic,
                dim: span.dim,
                underline: span.underline,
            };
            out.write_str(&styled.fg_code())?;
            emitted_fg = true;
        }
        out.write_str(&span.text)?;
        if mid_italic_off && i == 0 && span_fg.is_some() {
            out.write_str("\x1b[23m")?;
        }
    }
    if emitted_fg {
        out.write_str(FG_RESET)?;
    }
    if trailing_italic_off && emitted_fg {
        out.write_str("\x1b[23m")?;
    }
    Ok(())
}
