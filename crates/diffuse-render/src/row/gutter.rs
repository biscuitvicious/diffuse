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
        wire::{MovSide, OutputRun},
    },
    span::StyledSpan,
    theme::{Cursor, DiffKind, DiffuseTheme, FG_RESET, StyledColor},
};

/// Format 4-character right-aligned line numbers into a stack buffer without
/// heap allocation.
pub fn pad4_bytes(num: Option<u32>, buf: &mut [u8; 4]) -> &str {
    match num {
        None => "    ",
        Some(n) => {
            *buf = *b"    ";
            let mut val = n;
            let mut idx = 3;
            if val == 0 {
                buf[3] = b'0';
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
            // SAFETY: buf contains only ASCII spaces and digits.
            unsafe { str::from_utf8_unchecked(buf) }
        }
    }
}

/// Format 4-character right-aligned line numbers.
pub fn pad4(num: Option<u32>) -> String {
    let mut buf = [b' '; 4];
    pad4_bytes(num, &mut buf).to_string()
}

/// Generate plain 6-character prefix gutter (` 123 +`, ` 123 -`, ` 123 ~`, `
/// 123~ `, ` 123~+`, ` 123~-`, ` 123≈+`, ` 123≈-`).
pub fn prefix_of(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
) -> String {
    let mut buf = [b' '; 4];
    if is_common_only_row(kind, runs) {
        return format!("{}  ", pad4_bytes(new_line, &mut buf));
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            format!("{}~ ", pad4_bytes(new_line, &mut buf))
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => format!("{}~+", pad4_bytes(new_line, &mut buf)),
            RowKind::Del => format!("{}~-", pad4_bytes(old_line, &mut buf)),
            _ => format!("{}  ", pad4_bytes(new_line.or(old_line), &mut buf)),
        },
        _ => match kind {
            RowKind::Add => format!("{} +", pad4_bytes(new_line, &mut buf)),
            RowKind::Del => format!("{} -", pad4_bytes(old_line, &mut buf)),
            RowKind::Mod => format!("{} ~", pad4_bytes(new_line, &mut buf)),
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    format!("{}≈-", pad4_bytes(old_line, &mut buf))
                } else {
                    format!("{}≈+", pad4_bytes(new_line, &mut buf))
                }
            }
            _ => format!("{}  ", pad4_bytes(new_line.or(old_line), &mut buf)),
        },
    }
}

/// Generate the span-native 6-column coordinate gutter: the same
/// gutter (line number plus kind glyph) as ordered [`StyledSpan`]s riding on
/// `bg`, branching identically over groups, kinds, and move sides so the
/// ANSI and Ratatui paths cannot drift apart.
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix_spans(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    bg: Option<Color>,
    theme: &DiffuseTheme,
) -> Vec<StyledSpan> {
    let mut buf = [b' '; 4];
    let palette = theme.palette;
    let ctx_num = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num;
    if is_common_only_row(kind, runs) {
        let pad = pad4_bytes(new_line, &mut buf);
        return vec![ctx_num.span(format!("{pad}  "), bg)];
    }
    let add = &palette[Cursor::Rest][DiffKind::Add].gutter;
    let del = &palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            let pad = pad4_bytes(new_line, &mut buf);
            vec![r#mod.num.span(pad, bg), r#mod.icon.span("~ ", bg)]
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                vec![
                    add.num.span(pad, bg),
                    r#mod.icon.span("~", bg),
                    add.icon.span("+", bg),
                ]
            }
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                vec![
                    del.num.span(pad, bg),
                    r#mod.icon.span("~", bg),
                    del.icon.span("-", bg),
                ]
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
        },
        _ => match kind {
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                // The pad/space boundary carries the mid-prefix italic-off
                // on the ANSI path (`{pad}\x1b[23m -`), so the space is
                // its own span in the number foreground but without the
                // number italic: serializing it re-emits the active
                // foreground alone (a post-optimization no-op), keeping
                // the quirk byte in its exact position. A fused italic
                // re-emission would flip italic back on and survive the
                // optimizer, so the flags must stay off here.
                vec![
                    del.num.span(pad, bg),
                    StyledSpan::styled(" ", Some(del.num.color), bg),
                    del.icon.span("-", bg),
                ]
            }
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                vec![add.num.span(format!("{pad} "), bg), add.icon.span("+", bg)]
            }
            RowKind::Mod => {
                let pad = pad4_bytes(new_line, &mut buf);
                vec![
                    r#mod.num.span(format!("{pad} "), bg),
                    r#mod.icon.span("~", bg),
                ]
            }
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    let pad = pad4_bytes(old_line, &mut buf);
                    vec![
                        del.num.span(pad, bg),
                        mov.icon.span("≈", bg),
                        del.icon.span("-", bg),
                    ]
                } else {
                    let pad = pad4_bytes(new_line, &mut buf);
                    vec![
                        add.num.span(pad, bg),
                        mov.icon.span("≈", bg),
                        add.icon.span("+", bg),
                    ]
                }
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                vec![ctx_num.span(format!("{pad}  "), bg)]
            }
        },
    }
}

/// Generate the span-native twin of the wrapped-row continuation prefix:
/// six spaces in the dim gutter color riding on `bg`, matching the inline
/// `"\\x1b[38;5;242m      \\x1b[39m"` continuation prefix the ANSI viewport
/// emits for wrapped segments.
pub fn continuation_prefix_spans(bg: Option<Color>) -> Vec<StyledSpan> {
    vec![StyledSpan::styled("      ", Some(Color::Fixed(242)), bg)]
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
