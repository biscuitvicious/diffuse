//! Line number padding and coordinate gutter prefix rendering.

use std::str;

use super::segment::is_common_only_row;
use crate::{
    ansi::ColorExt,
    core::{
        parse::{RowGroup, RowKind},
        wire::{MovSide, OutputRun},
    },
    theme::{DiffuseTheme, FG_RESET},
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
    if is_common_only_row(kind, runs) {
        let pad = pad4_bytes(new_line, &mut buf);
        let color = if in_hunk {
            theme.palette.ctx_gutter_in_hunk
        } else {
            theme.palette.ctx_gutter_out_hunk
        };
        let color_fg = color.fg_code();
        return format!("{color_fg}{pad}  {FG_RESET}\x1b[23m");
    }
    match group {
        Some(RowGroup::Anchor) if kind == RowKind::Mod => {
            let pad = pad4_bytes(new_line, &mut buf);
            let num_fg = theme.palette.mod_gutter_num.fg_code();
            let mod_fg = theme.palette.mod_gutter_icon.fg_code();
            format!("{num_fg}{pad}{mod_fg}~ {FG_RESET}\x1b[23m")
        }
        Some(RowGroup::Aux { .. }) => match kind {
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                let num_fg = theme.palette.add_gutter_num.fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                let add_fg = theme.palette.add_gutter_icon.fg_code();
                format!("{num_fg}{pad}{mod_fg}~{add_fg}+{FG_RESET}\x1b[23m")
            }
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                let num_fg = theme.palette.del_gutter_num.italic_fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                let del_fg = theme.palette.del_gutter_icon.fg_code();
                format!("{num_fg}{pad}\x1b[23m{mod_fg}~{del_fg}-{FG_RESET}\x1b[23m")
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                let color = if in_hunk {
                    theme.palette.ctx_gutter_in_hunk
                } else {
                    theme.palette.ctx_gutter_out_hunk
                };
                let color_fg = color.fg_code();
                format!("{color_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
        _ => match kind {
            RowKind::Del => {
                let pad = pad4_bytes(old_line, &mut buf);
                let num_fg = theme.palette.del_gutter_num.italic_fg_code();
                let icon_fg = theme.palette.del_gutter_icon.fg_code();
                format!("{num_fg}{pad}\x1b[23m {icon_fg}-{FG_RESET}\x1b[23m")
            }
            RowKind::Add => {
                let pad = pad4_bytes(new_line, &mut buf);
                let num_fg = theme.palette.add_gutter_num.fg_code();
                let icon_fg = theme.palette.add_gutter_icon.fg_code();
                format!("{num_fg}{pad} {icon_fg}+{FG_RESET}\x1b[23m")
            }
            RowKind::Mod => {
                let pad = pad4_bytes(new_line, &mut buf);
                let num_fg = theme.palette.mod_gutter_num.fg_code();
                let mod_fg = theme.palette.mod_gutter_icon.fg_code();
                format!("{num_fg}{pad} {mod_fg}~{FG_RESET}\x1b[23m")
            }
            RowKind::Mov => {
                let num_fg = theme.palette.mov_gutter_num.fg_code();
                let mov_fg = theme.palette.mov_gutter_icon.fg_code();
                if mov_side == Some(MovSide::Src) {
                    let pad = pad4_bytes(old_line, &mut buf);
                    let del_fg = theme.palette.del_gutter_icon.fg_code();
                    format!("{num_fg}{pad}{mov_fg}≈{del_fg}-{FG_RESET}\x1b[23m")
                } else {
                    let pad = pad4_bytes(new_line, &mut buf);
                    let add_fg = theme.palette.add_gutter_icon.fg_code();
                    format!("{num_fg}{pad}{mov_fg}≈{add_fg}+{FG_RESET}\x1b[23m")
                }
            }
            _ => {
                let pad = pad4_bytes(new_line.or(old_line), &mut buf);
                let color = if in_hunk {
                    theme.palette.ctx_gutter_in_hunk
                } else {
                    theme.palette.ctx_gutter_out_hunk
                };
                let color_fg = color.fg_code();
                format!("{color_fg}{pad}  {FG_RESET}\x1b[23m")
            }
        },
    }
}
