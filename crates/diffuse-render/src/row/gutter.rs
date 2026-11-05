//! Line number padding and coordinate gutter prefix rendering.

use std::str;

use super::segment::is_common_only_row;
use crate::{
    core::{
        parse::{RowGroup, RowKind},
        wire::{MovSide, OutputRun},
    },
    theme::{Cursor, DiffKind, DiffuseTheme, FG_RESET},
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

/// Generate styled ANSI 6-character prefix gutter with color highlights and
/// theme support.
///
/// Colors resolve through the theme palette: per-kind gutters at the rest
/// cursor tier, with cross-kind glyphs (the trailing `±` on relocation and
/// aux rows, the `~` on aux rows) composed from the sibling kind slots at
/// render time. The in-hunk flag selects the cursor tier for context
/// gutters ([`Cursor::Hunk`] vs [`Cursor::Rest`]).
#[allow(clippy::too_many_arguments)]
pub fn styled_prefix(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut buf = [b' '; 4];
    let palette = theme.palette;
    let ctx_num_fg = palette[if in_hunk { Cursor::Hunk } else { Cursor::Rest }][DiffKind::Ctx]
        .gutter
        .num
        .fg_code();
    if is_common_only_row(kind, runs) {
        let pad = pad4_bytes(new_line, &mut buf);
        return format!("{ctx_num_fg}{pad}  {FG_RESET}\x1b[23m");
    }
    let add = &palette[Cursor::Rest][DiffKind::Add].gutter;
    let del = &palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov = &palette[Cursor::Rest][DiffKind::Mov].gutter;
    let r#mod = &palette[Cursor::Rest][DiffKind::Mod].gutter;
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            let pad = pad4_bytes(new_line, &mut buf);
            format!(
                "{}{pad}{}~ {FG_RESET}\x1b[23m",
                r#mod.num.fg_code(),
                r#mod.icon.fg_code()
            )
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                format!(
                    "{num_fg}{pad}{mod_fg}~{add_fg}+{FG_RESET}\x1b[23m",
                    num_fg = add.num.fg_code(),
                    mod_fg = r#mod.icon.fg_code(),
                    add_fg = add.icon.fg_code(),
                )
            }
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                format!(
                    "{num_fg}{pad}\x1b[23m{mod_fg}~{del_fg}-{FG_RESET}\x1b[23m",
                    num_fg = del.num.fg_code(),
                    mod_fg = r#mod.icon.fg_code(),
                    del_fg = del.icon.fg_code(),
                )
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                format!("{ctx_num_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
        _ => match kind {
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                format!(
                    "{num_fg}{pad}\x1b[23m {icon_fg}-{FG_RESET}\x1b[23m",
                    num_fg = del.num.fg_code(),
                    icon_fg = del.icon.fg_code(),
                )
            }
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                format!(
                    "{num_fg}{pad} {icon_fg}+{FG_RESET}\x1b[23m",
                    num_fg = add.num.fg_code(),
                    icon_fg = add.icon.fg_code(),
                )
            }
            RowKind::Mod => {
                let pad = pad4_bytes(new_line, &mut buf);
                format!(
                    "{num_fg}{pad} {icon_fg}~{FG_RESET}\x1b[23m",
                    num_fg = r#mod.num.fg_code(),
                    icon_fg = r#mod.icon.fg_code(),
                )
            }
            RowKind::Mov => {
                if mov_side == Some(MovSide::Src) {
                    let pad = pad4_bytes(old_line, &mut buf);
                    format!(
                        "{num_fg}{pad}{mov_fg}≈{del_fg}-{FG_RESET}\x1b[23m",
                        num_fg = mov.num.fg_code(),
                        mov_fg = mov.icon.fg_code(),
                        del_fg = del.icon.fg_code(),
                    )
                } else {
                    let pad = pad4_bytes(new_line, &mut buf);
                    format!(
                        "{num_fg}{pad}{mov_fg}≈{add_fg}+{FG_RESET}\x1b[23m",
                        num_fg = mov.num.fg_code(),
                        mov_fg = mov.icon.fg_code(),
                        add_fg = add.icon.fg_code(),
                    )
                }
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                format!("{ctx_num_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
    }
}
