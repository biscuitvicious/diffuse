//! Theme palette definitions and color mapping specifications.

use crate::ansi::AnsiColor;

/// Complete color palette for syntax highlighting, diff rows, and UI
/// components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_base: AnsiColor,
    pub bg_modal: AnsiColor,
    pub border: AnsiColor,
    pub warning: AnsiColor,
    pub accent: AnsiColor,
    pub muted: AnsiColor,

    // Diff Rows
    pub add_tint_bg: AnsiColor,
    pub add_cursor_bg: AnsiColor,
    pub add_fill_bg: AnsiColor,
    pub add_fill_cursor_bg: AnsiColor,
    pub add_fg: AnsiColor,

    pub del_tint_bg: AnsiColor,
    pub del_cursor_bg: AnsiColor,
    pub del_fill_bg: AnsiColor,
    pub del_fill_cursor_bg: AnsiColor,
    pub del_fg: AnsiColor,

    pub mod_tint_bg: AnsiColor,
    pub mod_cursor_bg: AnsiColor,
    pub mod_fill_bg: AnsiColor,
    pub mod_fill_cursor_bg: AnsiColor,
    pub mod_fg: AnsiColor,

    pub mov_tint_bg: AnsiColor,
    pub mov_cursor_bg: AnsiColor,
    pub mov_fg: AnsiColor,

    pub ctx_fg: AnsiColor,

    // Syntax
    pub syntax_keyword: AnsiColor,
    pub syntax_type: AnsiColor,
    pub syntax_function: AnsiColor,
    pub syntax_number: AnsiColor,
    pub syntax_string: AnsiColor,
    pub syntax_comment: AnsiColor,
    pub syntax_operator: AnsiColor,

    // Markdown
    pub md_bold: AnsiColor,
    pub md_italic: AnsiColor,
    pub md_strike: AnsiColor,
    pub md_link: AnsiColor,
    pub md_url: AnsiColor,
    pub md_delim: AnsiColor,
    pub md_rule: AnsiColor,
}

impl ThemePalette {
    /// Canonical Magic Theme Palette matching reviewer styling.
    #[must_use]
    pub const fn default_magic() -> Self {
        Self {
            bg_base: AnsiColor::Index(236),
            bg_modal: AnsiColor::Index(236),
            border: AnsiColor::Index(240),
            warning: AnsiColor::Index(220),
            accent: AnsiColor::Index(51),
            muted: AnsiColor::Index(243),

            add_tint_bg: AnsiColor::Index(22),
            add_cursor_bg: AnsiColor::Index(28),
            add_fill_bg: AnsiColor::Index(28),
            add_fill_cursor_bg: AnsiColor::Index(34),
            add_fg: AnsiColor::Index(150),

            del_tint_bg: AnsiColor::Index(52),
            del_cursor_bg: AnsiColor::Index(88),
            del_fill_bg: AnsiColor::Index(52),
            del_fill_cursor_bg: AnsiColor::Index(88),
            del_fg: AnsiColor::Index(167),

            mod_tint_bg: AnsiColor::Index(236),
            mod_cursor_bg: AnsiColor::Index(237),
            mod_fill_bg: AnsiColor::Index(28),
            mod_fill_cursor_bg: AnsiColor::Index(34),
            mod_fg: AnsiColor::Index(244),

            mov_tint_bg: AnsiColor::Index(236),
            mov_cursor_bg: AnsiColor::Index(237),
            mov_fg: AnsiColor::Index(244),

            ctx_fg: AnsiColor::Index(244),

            syntax_keyword: AnsiColor::Index(141),
            syntax_type: AnsiColor::Index(173),
            syntax_function: AnsiColor::Index(117),
            syntax_number: AnsiColor::Index(179),
            syntax_string: AnsiColor::Index(114),
            syntax_comment: AnsiColor::Index(243),
            syntax_operator: AnsiColor::Index(110),

            md_bold: AnsiColor::Index(172),
            md_italic: AnsiColor::Index(187),
            md_strike: AnsiColor::Index(243),
            md_link: AnsiColor::Index(75),
            md_url: AnsiColor::Index(243),
            md_delim: AnsiColor::Index(51),
            md_rule: AnsiColor::Index(220),
        }
    }

    /// Legacy Theme Palette.
    #[must_use]
    pub const fn legacy() -> Self {
        Self {
            bg_base: AnsiColor::Index(236),
            bg_modal: AnsiColor::Index(236),
            border: AnsiColor::Index(240),
            warning: AnsiColor::Index(220),
            accent: AnsiColor::Index(214),
            muted: AnsiColor::Index(244),

            add_tint_bg: AnsiColor::Index(22),
            add_cursor_bg: AnsiColor::Index(28),
            add_fill_bg: AnsiColor::Index(28),
            add_fill_cursor_bg: AnsiColor::Index(34),
            add_fg: AnsiColor::Index(150),

            del_tint_bg: AnsiColor::Index(52),
            del_cursor_bg: AnsiColor::Index(88),
            del_fill_bg: AnsiColor::Index(52),
            del_fill_cursor_bg: AnsiColor::Index(88),
            del_fg: AnsiColor::Index(167),

            mod_tint_bg: AnsiColor::Index(236),
            mod_cursor_bg: AnsiColor::Index(237),
            mod_fill_bg: AnsiColor::Index(28),
            mod_fill_cursor_bg: AnsiColor::Index(34),
            mod_fg: AnsiColor::Index(244),

            mov_tint_bg: AnsiColor::Index(236),
            mov_cursor_bg: AnsiColor::Index(237),
            mov_fg: AnsiColor::Index(244),

            ctx_fg: AnsiColor::Index(244),

            syntax_keyword: AnsiColor::Index(141),
            syntax_type: AnsiColor::Index(173),
            syntax_function: AnsiColor::Index(117),
            syntax_number: AnsiColor::Index(179),
            syntax_string: AnsiColor::Index(114),
            syntax_comment: AnsiColor::Index(244),
            syntax_operator: AnsiColor::Index(110),

            md_bold: AnsiColor::Index(173),
            md_italic: AnsiColor::Index(244),
            md_strike: AnsiColor::Index(244),
            md_link: AnsiColor::Index(252),
            md_url: AnsiColor::Index(244),
            md_delim: AnsiColor::Index(214),
            md_rule: AnsiColor::Index(220),
        }
    }
}
