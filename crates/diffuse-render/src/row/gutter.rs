//! Line number padding and coordinate gutter prefix rendering.

use std::{
    fmt::{self, Write},
    str,
};

use super::segment::is_common_only_row;
use crate::{
    ansi::Color,
    core::{
        parse::{PeerGroup, RowKind},
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

/// UTF-8 ellipsis character (`…`, U+2026, 3 bytes, 1 visual column).
pub const ELLIPSIS: &str = "…";

/// Visual width of the gutter prefix for old and new line number widths.
///
/// When `old_line_width == 0`, old line numbers are collapsed and only the
/// `new_line_width + 2` columns are occupied.
/// When `old_line_width > 0`, the gutter allocates `old_line_width + 1 +
/// new_line_width + 2` columns (including the 1-column space separator between
/// old and new line numbers).
#[inline]
pub const fn dual_gutter_width(old_line_width: usize, new_line_width: usize) -> usize {
    if old_line_width > 0 {
        old_line_width + 1 + new_line_width + 2
    } else {
        new_line_width + 2
    }
}

/// Visual width of the gutter prefix for a single line number width
/// (`line_num_width + 2`).
#[inline]
pub const fn gutter_width(line_num_width: usize) -> usize {
    dual_gutter_width(0, line_num_width)
}

/// Minimum character width required for each pane individually to enable
/// side-by-side mode by default.
pub const MIN_SIDE_BY_SIDE_PANE_CHARS: usize = 80;

/// Partition `content_width` into `(left_width, right_width)` reserving 1
/// column for the center vertical divider between split dual panes.
#[inline]
pub const fn split_pane_widths(content_width: usize) -> (usize, usize) {
    let avail = content_width.saturating_sub(1);
    let left = avail / 2;
    let right = avail - left;
    (left, right)
}

/// Returns true if `content_width` is wide enough that both the left and right
/// panes individually receive at least [`MIN_SIDE_BY_SIDE_PANE_CHARS`] columns.
#[inline]
pub const fn is_wide_enough_for_side_by_side(content_width: usize) -> bool {
    let (left, right) = split_pane_widths(content_width);
    left >= MIN_SIDE_BY_SIDE_PANE_CHARS && right >= MIN_SIDE_BY_SIDE_PANE_CHARS
}

/// Visual widths of the line number prefix gutters for split dual-pane diffs
/// as `(left_gutter_width, right_gutter_width)`.
#[inline]
pub const fn split_gutter_widths(old_line_width: usize, new_line_width: usize) -> (usize, usize) {
    (gutter_width(old_line_width), gutter_width(new_line_width))
}

/// Layout metrics and column budgets for side-by-side (split dual-pane) diffs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPaneLayout {
    /// Overall width of the left (pre-image) pane in columns.
    pub left_w: usize,
    /// Overall width of the right (post-image) pane in columns.
    pub right_w: usize,
    /// Line number gutter width of the left pane in columns.
    pub left_gw: usize,
    /// Line number gutter width of the right pane in columns.
    pub right_gw: usize,
    /// Available text column budget for the left pane (`max(1)`).
    pub left_cols: usize,
    /// Available text column budget for the right pane (`max(1)`).
    pub right_cols: usize,
}

/// Compute the full layout metrics and text column budgets for split dual-pane
/// diffs from total content width and pane line number widths.
#[inline]
pub const fn split_pane_layout(
    content_width: usize,
    old_line_width: usize,
    new_line_width: usize,
) -> SplitPaneLayout {
    let (left_w, right_w) = split_pane_widths(content_width);
    let (left_gw, right_gw) = split_gutter_widths(old_line_width, new_line_width);
    let left_avail = left_w.saturating_sub(left_gw);
    let right_avail = right_w.saturating_sub(right_gw);
    let left_cols = if left_avail > 0 { left_avail } else { 1 };
    let right_cols = if right_avail > 0 { right_avail } else { 1 };
    SplitPaneLayout {
        left_w,
        right_w,
        left_gw,
        right_gw,
        left_cols,
        right_cols,
    }
}

/// Returns whether the left pane displays content for this row.
#[inline]
pub const fn is_left_active(kind: RowKind, mov_side: Option<MovSide>) -> bool {
    match kind {
        RowKind::Ctx | RowKind::Mod | RowKind::Del => true,
        RowKind::Mov => match mov_side {
            Some(MovSide::Src) => true,
            Some(MovSide::Tgt) => false,
            None => true,
        },
        _ => false,
    }
}

/// Returns whether the right pane displays content for this row.
#[inline]
pub const fn is_right_active(kind: RowKind, mov_side: Option<MovSide>) -> bool {
    match kind {
        RowKind::Ctx | RowKind::Mod | RowKind::Add => true,
        RowKind::Mov => match mov_side {
            Some(MovSide::Tgt) => true,
            Some(MovSide::Src) => false,
            None => true,
        },
        _ => false,
    }
}

/// Returns the 2-character gutter glyph string for the left (pre-image) pane of
/// a split row.
#[inline]
pub const fn split_left_glyph(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
) -> &'static str {
    match (group, kind) {
        (Some(g), RowKind::Mod) if g.is_multiline() => "~ ",
        (Some(g), RowKind::Del) if g.is_multiline() => "~-",
        _ => match kind {
            RowKind::Mod => " ~",
            RowKind::Del => " -",
            RowKind::Mov => match mov_side {
                Some(MovSide::Src) => "≈-",
                _ => "  ",
            },
            _ => "  ",
        },
    }
}

/// Returns the 2-character gutter glyph string for the right (post-image) pane
/// of a split row.
#[inline]
pub const fn split_right_glyph(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
) -> &'static str {
    match (group, kind) {
        (Some(g), RowKind::Mod) if g.is_multiline() => "~ ",
        (Some(g), RowKind::Add) if g.is_multiline() => "~+",
        _ => match kind {
            RowKind::Mod => " ~",
            RowKind::Add => " +",
            RowKind::Mov => match mov_side {
                Some(MovSide::Tgt) => "≈+",
                _ => "  ",
            },
            _ => "  ",
        },
    }
}

/// Static space padding matching line number widths `0..=MAX_LINE_NUM_WIDTH` (0
/// to 16 spaces).
pub const LINE_NUM_SPACES: [&str; MAX_LINE_NUM_WIDTH + 1] = [
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

/// Static space padding matching full gutter widths (`line_num_width + 2`, 2 to
/// 18 spaces).
pub const GUTTER_SPACES: [&str; MAX_LINE_NUM_WIDTH + 1] = [
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

/// Static space string covering up to 40 columns for zero-allocation gutter
/// padding and continuation prefixes.
pub const STATIC_SPACES: &str = "                                        ";

/// Returns a static space slice of the requested `width` (up to 40 columns),
/// clamped to 40 if `width` exceeds the static buffer length.
#[inline]
pub fn static_spaces(width: usize) -> &'static str {
    let w = width.min(STATIC_SPACES.len());
    &STATIC_SPACES[..w]
}

#[inline]
fn blank_spaces(width: usize) -> &'static str {
    static_spaces(width)
}

/// Static continuation box prefixes matching `line_num_width` spaces followed
/// by `"│ "`.
pub const BOX_PREFIXES: [&str; MAX_LINE_NUM_WIDTH + 1] = [
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

/// Static continuation box bottom prefixes matching `line_num_width` spaces
/// followed by `"└ "`.
pub const BOX_BOTTOM_PREFIXES: [&str; MAX_LINE_NUM_WIDTH + 1] = [
    "└ ",
    " └ ",
    "  └ ",
    "   └ ",
    "    └ ",
    "     └ ",
    "      └ ",
    "       └ ",
    "        └ ",
    "         └ ",
    "          └ ",
    "           └ ",
    "            └ ",
    "             └ ",
    "              └ ",
    "               └ ",
    "                └ ",
];

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
        return LINE_NUM_SPACES[w];
    };

    let digits = if n == 0 { 1 } else { (n.ilog10() + 1) as usize };
    if digits <= w {
        buf[..w - digits].fill(b' ');
        let mut val = n;
        for idx in (w - digits..w).rev() {
            buf[idx] = b'0' + (val % 10) as u8;
            val /= 10;
        }
        unsafe { str::from_utf8_unchecked(&buf[..w]) }
    } else {
        let target = w - 1;
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
        buf[target..target + ELLIPSIS.len()].copy_from_slice(ELLIPSIS.as_bytes());
        let total_bytes = target + ELLIPSIS.len();
        unsafe { str::from_utf8_unchecked(&buf[..total_bytes]) }
    }
}

/// Generate plain prefix gutter with default pager line number width.
pub fn prefix_of(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
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
    group: Option<PeerGroup>,
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
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    line_num_width: usize,
) -> String {
    prefix_of_with_mod_side_and_widths(
        kind,
        mov_side,
        mod_side,
        mod_glyph,
        group,
        new_line,
        old_line,
        runs,
        0,
        line_num_width,
    )
}

/// Generate plain prefix gutter with optional expanded mod side and specified
/// old and new line number widths.
#[allow(clippy::too_many_arguments)]
pub fn prefix_of_with_mod_side_and_widths(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    old_line_width: usize,
    new_line_width: usize,
) -> String {
    if old_line_width == 0 {
        let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
        if new_line_width == 0 {
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
                Some(g) if g.is_multiline() => match kind {
                    RowKind::Add => "~+".to_string(),
                    RowKind::Del => "~-".to_string(),
                    RowKind::Mod => "~ ".to_string(),
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
            return format!("{}  ", format_line_num(new_line, new_line_width, &mut buf));
        }
        if let Some(side) = mod_side {
            let glyph_str = mod_glyph
                .unwrap_or_else(|| ModGlyph::for_side(side))
                .as_str();
            return match side {
                ModSide::Del => format!(
                    "{}{glyph_str}-",
                    format_line_num(old_line, new_line_width, &mut buf)
                ),
                ModSide::Add => format!(
                    "{}{glyph_str}+",
                    format_line_num(new_line, new_line_width, &mut buf)
                ),
            };
        }
        return match group {
            Some(g) if g.is_multiline() => match kind {
                RowKind::Add => {
                    format!("{}~+", format_line_num(new_line, new_line_width, &mut buf))
                }
                RowKind::Del => {
                    format!("{}~-", format_line_num(old_line, new_line_width, &mut buf))
                }
                RowKind::Mod => {
                    format!("{}~ ", format_line_num(new_line, new_line_width, &mut buf))
                }
                _ => format!(
                    "{}  ",
                    format_line_num(new_line.or(old_line), new_line_width, &mut buf)
                ),
            },
            _ => match kind {
                RowKind::Add => {
                    format!("{} +", format_line_num(new_line, new_line_width, &mut buf))
                }
                RowKind::Del => {
                    format!("{} -", format_line_num(old_line, new_line_width, &mut buf))
                }
                RowKind::Mod => {
                    format!("{} ~", format_line_num(new_line, new_line_width, &mut buf))
                }
                RowKind::Mov => {
                    if mov_side == Some(MovSide::Src) {
                        format!("{}≈-", format_line_num(old_line, new_line_width, &mut buf))
                    } else {
                        format!("{}≈+", format_line_num(new_line, new_line_width, &mut buf))
                    }
                }
                _ => format!(
                    "{}  ",
                    format_line_num(new_line.or(old_line), new_line_width, &mut buf)
                ),
            },
        };
    }

    let mut old_buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let mut new_buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let old_pad = format_line_num(old_line, old_line_width, &mut old_buf);
    let new_pad = format_line_num(new_line, new_line_width, &mut new_buf);
    let blank_old = blank_spaces(old_line_width);
    let blank_new = blank_spaces(new_line_width);

    if is_common_only_row(kind, runs) {
        return format!("{old_pad} {new_pad}  ");
    }
    if let Some(side) = mod_side {
        let glyph_str = mod_glyph
            .unwrap_or_else(|| ModGlyph::for_side(side))
            .as_str();
        return match side {
            ModSide::Del => format!("{old_pad} {blank_new}{glyph_str}-"),
            ModSide::Add => format!("{blank_old} {new_pad}{glyph_str}+"),
        };
    }
    match group {
        Some(g) if g.is_multiline() => match kind {
            RowKind::Add => format!("{blank_old} {new_pad}~+"),
            RowKind::Del => format!("{old_pad} {blank_new}~-"),
            RowKind::Mod => format!("{old_pad} {new_pad}~ "),
            _ => format!("{old_pad} {new_pad}  "),
        },
        _ => match kind {
            RowKind::Add => format!("{blank_old} {new_pad} +"),
            RowKind::Del => format!("{old_pad} {blank_new} -"),
            RowKind::Mod => format!("{old_pad} {new_pad} ~"),
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    format!("{old_pad} {blank_new}≈-")
                } else {
                    format!("{blank_old} {new_pad}≈+")
                }
            }
            _ => format!("{old_pad} {new_pad}  "),
        },
    }
}

/// Plain text gutter prefix for the left (old / pre-image) pane of a split row.
#[inline]
pub fn split_left_prefix_of(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
    old_line: Option<u32>,
    old_line_width: usize,
) -> String {
    if !is_left_active(kind, mov_side) {
        return static_spaces(gutter_width(old_line_width)).to_string();
    }
    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let pad = format_line_num(old_line, old_line_width, &mut buf);
    let glyph = split_left_glyph(kind, mov_side, group);
    format!("{pad}{glyph}")
}

/// Plain text gutter prefix for the right (new / post-image) pane of a split
/// row.
#[inline]
pub fn split_right_prefix_of(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    new_line_width: usize,
) -> String {
    if !is_right_active(kind, mov_side) {
        return static_spaces(gutter_width(new_line_width)).to_string();
    }
    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let pad = format_line_num(new_line, new_line_width, &mut buf);
    let glyph = split_right_glyph(kind, mov_side, group);
    format!("{pad}{glyph}")
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
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
    line_num_width: usize,
) -> Vec<StyledSpan> {
    styled_prefix_spans_with_widths(
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
        0,
        line_num_width,
    )
}

/// Generate the span-native coordinate gutter for specified old and new line
/// number widths: line numbers plus kind glyph as ordered [`StyledSpan`]s
/// riding on `bg`. When `old_line_width == 0`, old line numbers are omitted
/// entirely and only the new line number and kind glyphs are returned.
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix_spans_with_widths(
    kind: RowKind,
    mov_side: Option<MovSide>,
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
    old_line_width: usize,
    new_line_width: usize,
) -> Vec<StyledSpan> {
    let palette = theme.palette;
    let ctx_num = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num;
    let add = &palette[Cursor::Rest][DiffKind::Add].gutter;
    let del = &palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;

    if old_line_width == 0 {
        let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
        if is_common_only_row(kind, runs) {
            if new_line_width == 0 {
                return vec![ctx_num.span("  ", bg)];
            }
            let pad = format_line_num(new_line, new_line_width, &mut buf);
            return vec![ctx_num.span(format!("{pad}  "), bg)];
        }
        if let Some(side) = mod_side {
            let glyph_str = mod_glyph
                .unwrap_or_else(|| ModGlyph::for_side(side))
                .as_str();
            return match side {
                ModSide::Del => {
                    if new_line_width == 0 {
                        vec![r#mod.icon.span(glyph_str, bg), del.icon.span("-", bg)]
                    } else {
                        let pad = format_line_num(old_line, new_line_width, &mut buf);
                        vec![
                            del.num.span(pad, bg),
                            r#mod.icon.span(glyph_str, bg),
                            del.icon.span("-", bg),
                        ]
                    }
                }
                ModSide::Add => {
                    if new_line_width == 0 {
                        vec![r#mod.icon.span(glyph_str, bg), add.icon.span("+", bg)]
                    } else {
                        let pad = format_line_num(new_line, new_line_width, &mut buf);
                        vec![
                            add.num.span(pad, bg),
                            r#mod.icon.span(glyph_str, bg),
                            add.icon.span("+", bg),
                        ]
                    }
                }
            };
        }
        if let Some(g) = group
            && g.is_multiline()
        {
            return match kind {
                RowKind::Mod => {
                    if new_line_width == 0 {
                        vec![r#mod.icon.span("~ ", bg)]
                    } else {
                        let pad = format_line_num(new_line, new_line_width, &mut buf);
                        vec![r#mod.num.span(pad, bg), r#mod.icon.span("~ ", bg)]
                    }
                }
                RowKind::Add => {
                    if new_line_width == 0 {
                        vec![r#mod.icon.span("~", bg), add.icon.span("+", bg)]
                    } else {
                        let pad = format_line_num(new_line, new_line_width, &mut buf);
                        vec![
                            add.num.span(pad, bg),
                            r#mod.icon.span("~", bg),
                            add.icon.span("+", bg),
                        ]
                    }
                }
                RowKind::Del => {
                    if new_line_width == 0 {
                        vec![r#mod.icon.span("~", bg), del.icon.span("-", bg)]
                    } else {
                        let pad = format_line_num(old_line, new_line_width, &mut buf);
                        vec![
                            del.num.span(pad, bg),
                            r#mod.icon.span("~", bg),
                            del.icon.span("-", bg),
                        ]
                    }
                }
                _ => {
                    if new_line_width == 0 {
                        vec![ctx_num.span("  ", bg)]
                    } else {
                        let pad = format_line_num(new_line.or(old_line), new_line_width, &mut buf);
                        vec![ctx_num.span(format!("{pad}  "), bg)]
                    }
                }
            };
        }
        return match kind {
            RowKind::Del => {
                if new_line_width == 0 {
                    vec![
                        StyledSpan::styled(" ", Some(del.num.color), bg),
                        del.icon.span("-", bg),
                    ]
                } else {
                    let pad = format_line_num(old_line, new_line_width, &mut buf);
                    vec![
                        del.num.span(pad, bg),
                        StyledSpan::styled(" ", Some(del.num.color), bg),
                        del.icon.span("-", bg),
                    ]
                }
            }
            RowKind::Add => {
                if new_line_width == 0 {
                    vec![StyledSpan::styled(" ", None, bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, new_line_width, &mut buf);
                    vec![add.num.span(format!("{pad} "), bg), add.icon.span("+", bg)]
                }
            }
            RowKind::Mod => {
                if new_line_width == 0 {
                    vec![StyledSpan::styled(" ", None, bg), r#mod.icon.span("~", bg)]
                } else {
                    let pad = format_line_num(new_line, new_line_width, &mut buf);
                    vec![
                        r#mod.num.span(format!("{pad} "), bg),
                        r#mod.icon.span("~", bg),
                    ]
                }
            }
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    if new_line_width == 0 {
                        vec![mov.icon.span("≈", bg), del.icon.span("-", bg)]
                    } else {
                        let pad = format_line_num(old_line, new_line_width, &mut buf);
                        vec![
                            del.num.span(pad, bg),
                            mov.icon.span("≈", bg),
                            del.icon.span("-", bg),
                        ]
                    }
                } else if new_line_width == 0 {
                    vec![mov.icon.span("≈", bg), add.icon.span("+", bg)]
                } else {
                    let pad = format_line_num(new_line, new_line_width, &mut buf);
                    vec![
                        add.num.span(pad, bg),
                        mov.icon.span("≈", bg),
                        add.icon.span("+", bg),
                    ]
                }
            }
            _ => {
                if new_line_width == 0 {
                    vec![ctx_num.span("  ", bg)]
                } else {
                    let pad = format_line_num(new_line.or(old_line), new_line_width, &mut buf);
                    vec![ctx_num.span(format!("{pad}  "), bg)]
                }
            }
        };
    }

    let mut old_buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let mut new_buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let old_pad = format_line_num(old_line, old_line_width, &mut old_buf);
    let new_pad = format_line_num(new_line, new_line_width, &mut new_buf);

    if is_common_only_row(kind, runs) {
        return vec![ctx_num.span(format!("{old_pad} {new_pad}  "), bg)];
    }

    let pad_before_new = static_spaces(old_line_width + 1);
    let pad_after_old = static_spaces(1 + new_line_width);
    let pad_del_after_old = static_spaces(new_line_width + 2);

    if let Some(side) = mod_side {
        let glyph_str = mod_glyph
            .unwrap_or_else(|| ModGlyph::for_side(side))
            .as_str();
        return match side {
            ModSide::Del => vec![
                del.num.span(old_pad, bg),
                StyledSpan::styled(pad_after_old, Some(del.num.color), bg),
                r#mod.icon.span(glyph_str, bg),
                del.icon.span("-", bg),
            ],
            ModSide::Add => vec![
                StyledSpan::styled(pad_before_new, Some(add.num.color), bg),
                add.num.span(new_pad, bg),
                r#mod.icon.span(glyph_str, bg),
                add.icon.span("+", bg),
            ],
        };
    }

    if let Some(g) = group
        && g.is_multiline()
    {
        return match kind {
            RowKind::Mod => vec![
                r#mod.num.span(old_pad, bg),
                StyledSpan::styled(" ", Some(r#mod.num.color), bg),
                r#mod.num.span(new_pad, bg),
                r#mod.icon.span("~ ", bg),
            ],
            RowKind::Add => vec![
                StyledSpan::styled(pad_before_new, Some(add.num.color), bg),
                add.num.span(new_pad, bg),
                r#mod.icon.span("~", bg),
                add.icon.span("+", bg),
            ],
            RowKind::Del => vec![
                del.num.span(old_pad, bg),
                StyledSpan::styled(pad_after_old, Some(del.num.color), bg),
                r#mod.icon.span("~", bg),
                del.icon.span("-", bg),
            ],
            _ => vec![ctx_num.span(format!("{old_pad} {new_pad}  "), bg)],
        };
    }
    match kind {
        RowKind::Del => vec![
            del.num.span(old_pad, bg),
            StyledSpan::styled(pad_del_after_old, Some(del.num.color), bg),
            del.icon.span("-", bg),
        ],
        RowKind::Add => vec![
            StyledSpan::styled(pad_before_new, Some(add.num.color), bg),
            add.num.span(format!("{new_pad} "), bg),
            add.icon.span("+", bg),
        ],
        RowKind::Mod => vec![
            r#mod.num.span(old_pad, bg),
            StyledSpan::styled(" ", Some(r#mod.num.color), bg),
            r#mod.num.span(format!("{new_pad} "), bg),
            r#mod.icon.span("~", bg),
        ],
        RowKind::Mov => {
            if mov_side == Some(MovSide::Src) {
                vec![
                    del.num.span(old_pad, bg),
                    StyledSpan::styled(pad_after_old, Some(del.num.color), bg),
                    mov.icon.span("≈", bg),
                    del.icon.span("-", bg),
                ]
            } else {
                vec![
                    StyledSpan::styled(pad_before_new, Some(add.num.color), bg),
                    add.num.span(new_pad, bg),
                    mov.icon.span("≈", bg),
                    add.icon.span("+", bg),
                ]
            }
        }
        _ => vec![ctx_num.span(format!("{old_pad} {new_pad}  "), bg)],
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
    group: Option<PeerGroup>,
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

/// Generate styled gutter prefix spans for the left (old / pre-image) pane of a
/// split row.
#[allow(clippy::too_many_arguments)]
pub fn styled_split_left_prefix_spans(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
    old_line: Option<u32>,
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
    old_line_width: usize,
) -> Vec<StyledSpan> {
    let total_w = gutter_width(old_line_width);
    if !is_left_active(kind, mov_side) {
        return vec![StyledSpan::styled(static_spaces(total_w), None, bg)];
    }
    let palette = theme.palette;
    let ctx_num = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num;
    let del = &palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;

    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let pad = format_line_num(old_line, old_line_width, &mut buf);

    match (group, kind) {
        (Some(g), RowKind::Mod) if g.is_multiline() => {
            vec![r#mod.num.span(pad, bg), r#mod.icon.span("~ ", bg)]
        }
        (Some(g), RowKind::Del) if g.is_multiline() => vec![
            del.num.span(pad, bg),
            r#mod.icon.span("~", bg),
            del.icon.span("-", bg),
        ],
        _ => match kind {
            RowKind::Ctx => {
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
            RowKind::Mod => {
                vec![
                    r#mod.num.span(format!("{pad} "), bg),
                    r#mod.icon.span("~", bg),
                ]
            }
            RowKind::Del => {
                vec![
                    del.num.span(pad, bg),
                    StyledSpan::styled(" ", Some(del.num.color), bg),
                    del.icon.span("-", bg),
                ]
            }
            RowKind::Mov if mov_side == Some(MovSide::Src) => {
                vec![
                    del.num.span(pad, bg),
                    mov.icon.span("≈", bg),
                    del.icon.span("-", bg),
                ]
            }
            _ => {
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
        },
    }
}

/// Generate styled gutter prefix spans for the right (new / post-image) pane of
/// a split row.
#[allow(clippy::too_many_arguments)]
pub fn styled_split_right_prefix_spans(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<PeerGroup>,
    new_line: Option<u32>,
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
    new_line_width: usize,
) -> Vec<StyledSpan> {
    let total_w = gutter_width(new_line_width);
    if !is_right_active(kind, mov_side) {
        return vec![StyledSpan::styled(static_spaces(total_w), None, bg)];
    }
    let palette = theme.palette;
    let ctx_num = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num;
    let add = &palette[Cursor::Rest][DiffKind::Add].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;

    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];
    let pad = format_line_num(new_line, new_line_width, &mut buf);

    match (group, kind) {
        (Some(g), RowKind::Mod) if g.is_multiline() => {
            vec![r#mod.num.span(pad, bg), r#mod.icon.span("~ ", bg)]
        }
        (Some(g), RowKind::Add) if g.is_multiline() => vec![
            add.num.span(pad, bg),
            r#mod.icon.span("~", bg),
            add.icon.span("+", bg),
        ],
        _ => match kind {
            RowKind::Ctx => {
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
            RowKind::Mod => {
                vec![
                    r#mod.num.span(format!("{pad} "), bg),
                    r#mod.icon.span("~", bg),
                ]
            }
            RowKind::Add => {
                vec![add.num.span(format!("{pad} "), bg), add.icon.span("+", bg)]
            }
            RowKind::Mov if mov_side == Some(MovSide::Tgt) => {
                vec![
                    add.num.span(pad, bg),
                    mov.icon.span("≈", bg),
                    add.icon.span("+", bg),
                ]
            }
            _ => {
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
        },
    }
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
    let pad = if line_num_width <= MAX_LINE_NUM_WIDTH {
        GUTTER_SPACES[line_num_width]
    } else {
        return vec![StyledSpan::styled(
            " ".repeat(gutter_width(line_num_width)),
            Some(Color::Fixed(242)),
            bg,
        )];
    };
    vec![StyledSpan::styled(pad, Some(Color::Fixed(242)), bg)]
}

#[inline]
fn mod_row_extends_box(mod_side: Option<ModSide>) -> bool {
    mod_side.is_some()
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
    continuation_prefix_spans_with_mod_side_and_widths(
        mod_side,
        mod_glyph,
        theme,
        bg,
        0,
        line_num_width,
    )
}

/// Continuation prefix spans taking `ModSide`, `ModGlyph`, and specified
/// old and new line number widths into account.
pub fn continuation_prefix_spans_with_mod_side_and_widths(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    theme: &DiffuseTheme,
    bg: Option<Color>,
    old_line_width: usize,
    new_line_width: usize,
) -> Vec<StyledSpan> {
    let effective_glyph = mod_glyph.or_else(|| mod_side.map(ModGlyph::for_side));
    let glyph_str = if effective_glyph == Some(ModGlyph::Bottom) {
        "└"
    } else {
        "│"
    };

    if old_line_width == 0 {
        if mod_row_extends_box(mod_side) {
            let r#mod = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;
            if new_line_width == 0 {
                return vec![
                    r#mod.icon.span(glyph_str, bg),
                    StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
                ];
            }
            return vec![
                StyledSpan::styled(static_spaces(new_line_width), Some(Color::Fixed(242)), bg),
                r#mod.icon.span(glyph_str, bg),
                StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
            ];
        }
        return continuation_prefix_spans_with_width(bg, new_line_width);
    }

    if mod_row_extends_box(mod_side) {
        let r#mod = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;
        let pad_len = old_line_width + 1 + new_line_width;
        vec![
            StyledSpan::styled(static_spaces(pad_len), Some(Color::Fixed(242)), bg),
            r#mod.icon.span(glyph_str, bg),
            StyledSpan::styled(" ", Some(Color::Fixed(242)), bg),
        ]
    } else {
        let total_w = dual_gutter_width(old_line_width, new_line_width);
        vec![StyledSpan::styled(
            static_spaces(total_w),
            Some(Color::Fixed(242)),
            bg,
        )]
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
    let extends_box = mod_row_extends_box(mod_side);
    let idx = line_num_width.min(MAX_LINE_NUM_WIDTH);
    if extends_box {
        let effective_glyph = mod_glyph.or_else(|| mod_side.map(ModGlyph::for_side));
        if effective_glyph == Some(ModGlyph::Bottom) {
            BOX_BOTTOM_PREFIXES[idx]
        } else {
            BOX_PREFIXES[idx]
        }
    } else {
        GUTTER_SPACES[idx]
    }
}

/// Continuation prefix plain text taking `ModSide`, `ModGlyph`, and
/// specified old and new line number widths into account.
pub fn continuation_prefix_plain_with_mod_side_and_widths(
    mod_side: Option<ModSide>,
    mod_glyph: Option<ModGlyph>,
    old_line_width: usize,
    new_line_width: usize,
) -> String {
    if old_line_width == 0 {
        return continuation_prefix_plain_with_mod_side_and_width(
            mod_side,
            mod_glyph,
            new_line_width,
        )
        .to_string();
    }
    if mod_row_extends_box(mod_side) {
        let pad_len = old_line_width + 1 + new_line_width;
        let effective_glyph = mod_glyph.or_else(|| mod_side.map(ModGlyph::for_side));
        let glyph_str = if effective_glyph == Some(ModGlyph::Bottom) {
            "└"
        } else {
            "│"
        };
        format!("{}{glyph_str} ", static_spaces(pad_len))
    } else {
        static_spaces(dual_gutter_width(old_line_width, new_line_width)).to_string()
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

/// Continuation prefix spans for the left pane of a split row.
#[inline]
pub fn styled_split_left_continuation_prefix_spans(
    bg: Option<Color>,
    old_line_width: usize,
) -> Vec<StyledSpan> {
    vec![StyledSpan::styled(
        static_spaces(gutter_width(old_line_width)),
        Some(Color::Fixed(242)),
        bg,
    )]
}

/// Continuation prefix spans for the right pane of a split row.
#[inline]
pub fn styled_split_right_continuation_prefix_spans(
    bg: Option<Color>,
    new_line_width: usize,
) -> Vec<StyledSpan> {
    vec![StyledSpan::styled(
        static_spaces(gutter_width(new_line_width)),
        Some(Color::Fixed(242)),
        bg,
    )]
}

/// Plain text continuation prefix for the left pane of a split row.
#[inline]
pub fn split_left_continuation_prefix_plain(old_line_width: usize) -> &'static str {
    static_spaces(gutter_width(old_line_width))
}

/// Plain text continuation prefix for the right pane of a split row.
#[inline]
pub fn split_right_continuation_prefix_plain(new_line_width: usize) -> &'static str {
    static_spaces(gutter_width(new_line_width))
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
