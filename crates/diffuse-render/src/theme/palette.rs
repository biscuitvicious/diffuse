//! Theme palette definitions and color mapping specifications.

use crate::ansi::AnsiColor;
use diffuse_core::parse::RowKind;

/// Complete color palette for syntax highlighting, diff rows, and UI
/// components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_base: AnsiColor,
    pub bg_modal: AnsiColor,
    pub border: AnsiColor,
    pub warning: AnsiColor,
    pub accent: AnsiColor,
    pub muted: AnsiColor,

    // Context & Canvas
    pub ctx_tint_bg: AnsiColor,
    pub ctx_cursor_bg: AnsiColor,
    pub ctx_fg: AnsiColor,
    pub band_bg: AnsiColor,

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

    // Gutter / Line Numbers / Icons
    pub ctx_gutter_in_hunk: AnsiColor,
    pub ctx_gutter_out_hunk: AnsiColor,
    pub add_gutter_num: AnsiColor,
    pub add_gutter_icon: AnsiColor,
    pub del_gutter_num: AnsiColor,
    pub del_gutter_icon: AnsiColor,
    pub mod_gutter_num: AnsiColor,
    pub mod_gutter_icon: AnsiColor,
    pub mov_gutter_num: AnsiColor,
    pub mov_gutter_icon: AnsiColor,

    // Header & Dividers
    pub file_header_bg_selected: AnsiColor,
    pub file_header_bg_current: AnsiColor,
    pub file_header_bg_rest: AnsiColor,
    pub file_header_fg_selected: AnsiColor,
    pub file_header_fg_current: AnsiColor,
    pub file_header_fg_rest: AnsiColor,

    pub hunk_divider_bg_current: AnsiColor,
    pub hunk_divider_bg_rest: AnsiColor,
    pub hunk_divider_fg: AnsiColor,

    pub header_bar_bg: AnsiColor,
    pub header_bar_fg: AnsiColor,

    pub scrollbar_thumb: AnsiColor,
    pub scrollbar_track: AnsiColor,

    // Syntax
    pub syntax_keyword: AnsiColor,
    pub syntax_type: AnsiColor,
    pub syntax_function: AnsiColor,
    pub syntax_number: AnsiColor,
    pub syntax_string: AnsiColor,
    pub add_syntax_string: AnsiColor,
    pub del_syntax_string: AnsiColor,
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

            ctx_tint_bg: AnsiColor::Reset,
            ctx_cursor_bg: AnsiColor::Index(237),
            ctx_fg: AnsiColor::Index(244),
            band_bg: AnsiColor::Index(240),

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

            ctx_gutter_in_hunk: AnsiColor::Index(252),
            ctx_gutter_out_hunk: AnsiColor::Index(242),
            del_gutter_num: AnsiColor::Index(174),
            del_gutter_icon: AnsiColor::Index(203),
            add_gutter_num: AnsiColor::Index(151),
            add_gutter_icon: AnsiColor::Index(120),
            mod_gutter_num: AnsiColor::Index(253),
            mod_gutter_icon: AnsiColor::Index(214),
            mov_gutter_num: AnsiColor::Index(147),
            mov_gutter_icon: AnsiColor::Index(117),

            file_header_bg_selected: AnsiColor::Index(241),
            file_header_bg_current: AnsiColor::Index(239),
            file_header_bg_rest: AnsiColor::Index(237),
            file_header_fg_selected: AnsiColor::Index(219),
            file_header_fg_current: AnsiColor::Index(219),
            file_header_fg_rest: AnsiColor::Index(183),

            hunk_divider_bg_current: AnsiColor::Index(237),
            hunk_divider_bg_rest: AnsiColor::Reset,
            hunk_divider_fg: AnsiColor::Index(220),

            header_bar_bg: AnsiColor::Index(237),
            header_bar_fg: AnsiColor::Index(231),

            scrollbar_thumb: AnsiColor::Index(248),
            scrollbar_track: AnsiColor::Index(236),

            syntax_keyword: AnsiColor::Index(141),
            syntax_type: AnsiColor::Index(173),
            syntax_function: AnsiColor::Index(117),
            syntax_number: AnsiColor::Index(179),
            syntax_string: AnsiColor::Index(114),
            add_syntax_string: AnsiColor::Index(121),
            del_syntax_string: AnsiColor::Index(122),
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

            ctx_tint_bg: AnsiColor::Reset,
            ctx_cursor_bg: AnsiColor::Index(237),
            ctx_fg: AnsiColor::Index(244),
            band_bg: AnsiColor::Index(240),

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

            ctx_gutter_in_hunk: AnsiColor::Index(252),
            ctx_gutter_out_hunk: AnsiColor::Index(242),
            del_gutter_num: AnsiColor::Index(174),
            del_gutter_icon: AnsiColor::Index(203),
            add_gutter_num: AnsiColor::Index(151),
            add_gutter_icon: AnsiColor::Index(120),
            mod_gutter_num: AnsiColor::Index(253),
            mod_gutter_icon: AnsiColor::Index(214),
            mov_gutter_num: AnsiColor::Index(147),
            mov_gutter_icon: AnsiColor::Index(117),

            file_header_bg_selected: AnsiColor::Index(241),
            file_header_bg_current: AnsiColor::Index(239),
            file_header_bg_rest: AnsiColor::Index(237),
            file_header_fg_selected: AnsiColor::Index(219),
            file_header_fg_current: AnsiColor::Index(219),
            file_header_fg_rest: AnsiColor::Index(183),

            hunk_divider_bg_current: AnsiColor::Index(237),
            hunk_divider_bg_rest: AnsiColor::Reset,
            hunk_divider_fg: AnsiColor::Index(220),

            header_bar_bg: AnsiColor::Index(237),
            header_bar_fg: AnsiColor::Index(231),

            scrollbar_thumb: AnsiColor::Index(248),
            scrollbar_track: AnsiColor::Index(236),

            syntax_keyword: AnsiColor::Index(141),
            syntax_type: AnsiColor::Index(173),
            syntax_function: AnsiColor::Index(117),
            syntax_number: AnsiColor::Index(179),
            syntax_string: AnsiColor::Index(114),
            add_syntax_string: AnsiColor::Index(114),
            del_syntax_string: AnsiColor::Index(114),
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

    /// Deep Black Theme Palette with deeper background and muted red/green
    /// shades.
    #[must_use]
    pub const fn deep_black() -> Self {
        Self {
            bg_base: AnsiColor::Index(233),
            bg_modal: AnsiColor::Index(234),
            border: AnsiColor::Index(238),
            warning: AnsiColor::Index(220),
            accent: AnsiColor::Index(51),
            muted: AnsiColor::Index(241),

            ctx_tint_bg: AnsiColor::Index(233),
            ctx_cursor_bg: AnsiColor::Index(235),
            ctx_fg: AnsiColor::Index(245),
            band_bg: AnsiColor::Index(235),

            add_tint_bg: AnsiColor::Rgb(29, 35, 31),
            add_cursor_bg: AnsiColor::Rgb(40, 47, 42),
            add_fill_bg: AnsiColor::Rgb(40, 75, 48),
            add_fill_cursor_bg: AnsiColor::Rgb(52, 98, 64),
            add_fg: AnsiColor::Index(120),

            del_tint_bg: AnsiColor::Rgb(48, 18, 20),
            del_cursor_bg: AnsiColor::Rgb(68, 24, 26),
            del_fill_bg: AnsiColor::Rgb(82, 28, 32),
            del_fill_cursor_bg: AnsiColor::Rgb(100, 34, 38),
            del_fg: AnsiColor::Index(203),

            mod_tint_bg: AnsiColor::Index(233),
            mod_cursor_bg: AnsiColor::Index(235),
            mod_fill_bg: AnsiColor::Rgb(40, 75, 48),
            mod_fill_cursor_bg: AnsiColor::Rgb(52, 98, 64),
            mod_fg: AnsiColor::Index(245),

            mov_tint_bg: AnsiColor::Index(233),
            mov_cursor_bg: AnsiColor::Index(235),
            mov_fg: AnsiColor::Index(245),

            ctx_gutter_in_hunk: AnsiColor::Index(245),
            ctx_gutter_out_hunk: AnsiColor::Index(238),
            del_gutter_num: AnsiColor::Index(203),
            del_gutter_icon: AnsiColor::Index(203),
            add_gutter_num: AnsiColor::Index(120),
            add_gutter_icon: AnsiColor::Index(120),
            mod_gutter_num: AnsiColor::Index(245),
            mod_gutter_icon: AnsiColor::Index(214),
            mov_gutter_num: AnsiColor::Index(147),
            mov_gutter_icon: AnsiColor::Index(117),

            file_header_bg_selected: AnsiColor::Index(237),
            file_header_bg_current: AnsiColor::Index(235),
            file_header_bg_rest: AnsiColor::Index(233),
            file_header_fg_selected: AnsiColor::Index(177),
            file_header_fg_current: AnsiColor::Index(176),
            file_header_fg_rest: AnsiColor::Index(140),

            hunk_divider_bg_current: AnsiColor::Index(235),
            hunk_divider_bg_rest: AnsiColor::Reset,
            hunk_divider_fg: AnsiColor::Index(220),

            header_bar_bg: AnsiColor::Index(234),
            header_bar_fg: AnsiColor::Index(231),

            scrollbar_thumb: AnsiColor::Index(244),
            scrollbar_track: AnsiColor::Index(233),

            syntax_keyword: AnsiColor::Index(147),
            syntax_type: AnsiColor::Index(221),
            syntax_function: AnsiColor::Index(123),
            syntax_number: AnsiColor::Index(221),
            syntax_string: AnsiColor::Index(120),
            add_syntax_string: AnsiColor::Index(120),
            del_syntax_string: AnsiColor::Index(203),
            syntax_comment: AnsiColor::Index(150),
            syntax_operator: AnsiColor::Index(117),

            md_bold: AnsiColor::Index(172),
            md_italic: AnsiColor::Index(229),
            md_strike: AnsiColor::Index(250),
            md_link: AnsiColor::Index(117),
            md_url: AnsiColor::Index(250),
            md_delim: AnsiColor::Index(51),
            md_rule: AnsiColor::Index(220),
        }
    }

    /// Resolve semantic named color token.
    #[must_use]
    pub fn named_color(&self, name: &str) -> Option<AnsiColor> {
        match name {
            "toolDiffAdded" => Some(self.add_fg),
            "toolDiffRemoved" => Some(self.del_fg),
            "toolDiffContext" | "muted" => Some(self.muted),
            "warning" => Some(self.warning),
            "border" => Some(self.border),
            "accent" => Some(self.accent),
            _ => None,
        }
    }

    /// Resolve visual colors for file header banners.
    #[must_use]
    pub fn file_header_colors(
        &self,
        is_selected: bool,
        is_current: bool,
        in_band: bool,
    ) -> (AnsiColor, AnsiColor) {
        if is_selected {
            (self.file_header_bg_selected, self.file_header_fg_selected)
        } else if in_band || is_current {
            (self.file_header_bg_current, self.file_header_fg_current)
        } else {
            (self.file_header_bg_rest, self.file_header_fg_rest)
        }
    }

    /// Resolve visual colors for hunk divider banners.
    #[must_use]
    pub fn hunk_divider_colors(&self, is_current: bool) -> (AnsiColor, AnsiColor) {
        if is_current {
            (self.hunk_divider_bg_current, self.hunk_divider_fg)
        } else {
            (self.hunk_divider_bg_rest, self.hunk_divider_fg)
        }
    }

    /// Resolve syntax / markdown token color in the context of a specific row
    /// kind.
    #[must_use]
    pub fn syntax_token_color(
        &self,
        color: &str,
        row_kind: RowKind,
        is_legacy: bool,
    ) -> Option<AnsiColor> {
        if is_legacy {
            let mut c = color;
            if c == "mdBold" {
                c = "syntaxType";
            } else if c == "mdItalic" || c == "mdUrl" || c == "mdStrike" {
                c = "muted";
            } else if c == "mdLink" {
                c = "text";
            } else if c == "mdDelim" {
                c = "accent";
            }

            if row_kind == RowKind::Add && (c == "syntaxComment" || c == "muted") {
                Some(AnsiColor::Index(250))
            } else if c == "syntaxType" {
                Some(AnsiColor::Index(173))
            } else {
                self.named_color(c)
            }
        } else {
            match row_kind {
                RowKind::Add => match color {
                    "mdBold" => Some(AnsiColor::Index(172)),
                    "syntaxType" | "syntaxNumber" => Some(AnsiColor::Index(221)),
                    "syntaxKeyword" => Some(AnsiColor::Index(147)),
                    "syntaxFunction" => Some(AnsiColor::Index(123)),
                    "syntaxOperator" | "mdLink" => Some(AnsiColor::Index(117)),
                    "syntaxComment" => Some(AnsiColor::Index(150)),
                    "mdItalic" | "muted" => Some(AnsiColor::Index(229)),
                    "syntaxString" => Some(self.add_syntax_string),
                    "mdUrl" | "mdStrike" => Some(AnsiColor::Index(250)),
                    "accent" | "mdDelim" => Some(AnsiColor::Index(51)),
                    _ => self.named_color(color),
                },
                RowKind::Del => match color {
                    "mdBold" | "syntaxType" | "syntaxNumber" => Some(AnsiColor::Index(222)),
                    "syntaxKeyword" => Some(AnsiColor::Index(183)),
                    "syntaxFunction" => Some(AnsiColor::Index(159)),
                    "syntaxOperator" | "mdLink" => Some(AnsiColor::Index(153)),
                    "mdItalic" | "syntaxComment" | "muted" => Some(AnsiColor::Index(224)),
                    "syntaxString" => Some(self.del_syntax_string),
                    "mdUrl" | "mdStrike" => Some(AnsiColor::Index(248)),
                    "accent" | "mdDelim" => Some(AnsiColor::Index(51)),
                    _ => self.named_color(color),
                },
                _ => match color {
                    "mdBold" => Some(self.md_bold),
                    "syntaxType" => Some(self.syntax_type),
                    "syntaxKeyword" => Some(self.syntax_keyword),
                    "syntaxFunction" => Some(self.syntax_function),
                    "syntaxNumber" => Some(self.syntax_number),
                    "syntaxOperator" => Some(self.syntax_operator),
                    "mdItalic" => Some(self.md_italic),
                    "syntaxString" => Some(self.syntax_string),
                    "mdLink" => Some(self.md_link),
                    "mdUrl" | "mdStrike" | "syntaxComment" | "muted" => Some(self.syntax_comment),
                    "mdRule" => Some(self.md_rule),
                    "accent" | "mdDelim" => Some(self.accent),
                    _ => self.named_color(color),
                },
            }
        }
    }
}
