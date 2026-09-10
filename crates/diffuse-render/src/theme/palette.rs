//! Theme palette definitions and color mapping specifications.

use diffuse_core::parse::RowKind;
use yansi::Color;

/// Complete color palette for syntax highlighting, diff rows, and UI
/// components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_base: Color,
    pub bg_modal: Color,
    pub border: Color,
    pub warning: Color,
    pub accent: Color,
    pub muted: Color,

    // Context & Canvas
    pub ctx_tint_bg: Color,
    pub ctx_cursor_bg: Color,
    pub ctx_fg: Color,
    pub band_bg: Color,

    // Diff Rows
    pub add_tint_bg: Color,
    pub add_cursor_bg: Color,
    pub add_fill_bg: Color,
    pub add_fill_cursor_bg: Color,
    pub add_fg: Color,

    pub del_tint_bg: Color,
    pub del_cursor_bg: Color,
    pub del_fill_bg: Color,
    pub del_fill_cursor_bg: Color,
    pub del_fg: Color,

    pub mod_tint_bg: Color,
    pub mod_cursor_bg: Color,
    pub mod_fill_bg: Color,
    pub mod_fill_cursor_bg: Color,
    pub mod_fg: Color,

    pub mov_tint_bg: Color,
    pub mov_cursor_bg: Color,
    pub mov_fg: Color,

    // Gutter / Line Numbers / Icons
    pub ctx_gutter_in_hunk: Color,
    pub ctx_gutter_out_hunk: Color,
    pub add_gutter_num: Color,
    pub add_gutter_icon: Color,
    pub del_gutter_num: Color,
    pub del_gutter_icon: Color,
    pub mod_gutter_num: Color,
    pub mod_gutter_icon: Color,
    pub mov_gutter_num: Color,
    pub mov_gutter_icon: Color,

    // Header & Dividers
    pub file_header_bg_selected: Color,
    pub file_header_bg_current: Color,
    pub file_header_bg_rest: Color,
    pub file_header_fg_selected: Color,
    pub file_header_fg_current: Color,
    pub file_header_fg_rest: Color,

    pub hunk_divider_bg_current: Color,
    pub hunk_divider_bg_rest: Color,
    pub hunk_divider_fg: Color,

    pub header_bar_bg: Color,
    pub header_bar_fg: Color,

    pub scrollbar_thumb: Color,
    pub scrollbar_track: Color,

    // Syntax
    pub syntax_keyword: Color,
    pub syntax_type: Color,
    pub syntax_function: Color,
    pub syntax_number: Color,
    pub syntax_string: Color,
    pub add_syntax_string: Color,
    pub del_syntax_string: Color,
    pub syntax_comment: Color,
    pub syntax_operator: Color,

    // Markdown
    pub md_bold: Color,
    pub md_italic: Color,
    pub md_strike: Color,
    pub md_link: Color,
    pub md_url: Color,
    pub md_delim: Color,
    pub md_rule: Color,
}

impl ThemePalette {
    /// Default Theme Palette with deeper background and muted red/green
    /// shades.
    #[must_use]
    pub const fn default_palette() -> Self {
        Self {
            bg_base: Color::Fixed(233),
            bg_modal: Color::Fixed(234),
            border: Color::Fixed(238),
            warning: Color::Fixed(220),
            accent: Color::Fixed(51),
            muted: Color::Fixed(241),

            ctx_tint_bg: Color::Fixed(233),
            ctx_cursor_bg: Color::Fixed(235),
            ctx_fg: Color::Fixed(245),
            band_bg: Color::Fixed(235),

            add_tint_bg: Color::Rgb(22, 38, 23),
            add_cursor_bg: Color::Rgb(36, 56, 37),
            add_fill_bg: Color::Rgb(28, 99, 52),
            add_fill_cursor_bg: Color::Rgb(36, 126, 68),
            add_fg: Color::Fixed(120),

            del_tint_bg: Color::Rgb(38, 12, 14),
            del_cursor_bg: Color::Rgb(68, 24, 26),
            del_fill_bg: Color::Rgb(82, 28, 32),
            del_fill_cursor_bg: Color::Rgb(100, 34, 38),
            del_fg: Color::Fixed(203),

            mod_tint_bg: Color::Rgb(30, 30, 30),
            mod_cursor_bg: Color::Rgb(54, 54, 54),
            mod_fill_bg: Color::Rgb(28, 99, 52),
            mod_fill_cursor_bg: Color::Rgb(36, 126, 68),
            mod_fg: Color::Fixed(245),

            mov_tint_bg: Color::Fixed(233),
            mov_cursor_bg: Color::Fixed(235),
            mov_fg: Color::Fixed(245),

            ctx_gutter_in_hunk: Color::Fixed(245),
            ctx_gutter_out_hunk: Color::Fixed(238),
            del_gutter_num: Color::Fixed(203),
            del_gutter_icon: Color::Fixed(203),
            add_gutter_num: Color::Fixed(120),
            add_gutter_icon: Color::Fixed(120),
            mod_gutter_num: Color::Fixed(245),
            mod_gutter_icon: Color::Fixed(214),
            mov_gutter_num: Color::Fixed(147),
            mov_gutter_icon: Color::Fixed(117),

            file_header_bg_selected: Color::Fixed(237),
            file_header_bg_current: Color::Fixed(235),
            file_header_bg_rest: Color::Fixed(233),
            file_header_fg_selected: Color::Fixed(221),
            file_header_fg_current: Color::Fixed(220),
            file_header_fg_rest: Color::Fixed(214),

            hunk_divider_bg_current: Color::Fixed(235),
            hunk_divider_bg_rest: Color::Primary,
            hunk_divider_fg: Color::Fixed(213),

            header_bar_bg: Color::Fixed(234),
            header_bar_fg: Color::Fixed(231),

            scrollbar_thumb: Color::Fixed(244),
            scrollbar_track: Color::Fixed(233),

            syntax_keyword: Color::Fixed(147),
            syntax_type: Color::Fixed(220),
            syntax_function: Color::Fixed(123),
            syntax_number: Color::Fixed(221),
            syntax_string: Color::Fixed(120),
            add_syntax_string: Color::Fixed(120),
            del_syntax_string: Color::Fixed(203),
            syntax_comment: Color::Fixed(150),
            syntax_operator: Color::Fixed(117),

            md_bold: Color::Fixed(214),
            md_italic: Color::Fixed(229),
            md_strike: Color::Fixed(250),
            md_link: Color::Fixed(117),
            md_url: Color::Fixed(250),
            md_delim: Color::Fixed(51),
            md_rule: Color::Fixed(220),
        }
    }

    /// Resolve semantic named color token.
    #[must_use]
    pub fn named_color(&self, name: &str) -> Option<Color> {
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
    ) -> (Color, Color) {
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
    pub fn hunk_divider_colors(&self, is_current: bool) -> (Color, Color) {
        if is_current {
            (self.hunk_divider_bg_current, self.hunk_divider_fg)
        } else {
            (self.hunk_divider_bg_rest, self.hunk_divider_fg)
        }
    }

    /// Resolve syntax / markdown token color in the context of a specific row
    /// kind.
    #[must_use]
    pub fn syntax_token_color(&self, color: &str, row_kind: RowKind) -> Option<Color> {
        match row_kind {
            RowKind::Add => match color {
                "mdBold" => Some(self.md_bold),
                "syntaxType" => Some(self.syntax_type),
                "syntaxNumber" => Some(Color::Fixed(221)),
                "syntaxKeyword" => Some(Color::Fixed(147)),
                "syntaxFunction" => Some(Color::Fixed(123)),
                "syntaxOperator" | "mdLink" => Some(Color::Fixed(117)),
                "syntaxComment" => Some(Color::Fixed(150)),
                "mdItalic" | "muted" => Some(Color::Fixed(229)),
                "syntaxString" => Some(self.add_syntax_string),
                "mdUrl" | "mdStrike" => Some(Color::Fixed(250)),
                "accent" | "mdDelim" => Some(Color::Fixed(51)),
                _ => self.named_color(color),
            },
            RowKind::Del => match color {
                "mdBold" | "syntaxNumber" => Some(Color::Fixed(222)),
                "syntaxType" => Some(Color::Fixed(215)),
                "syntaxKeyword" => Some(Color::Fixed(183)),
                "syntaxFunction" => Some(Color::Fixed(159)),
                "syntaxOperator" | "mdLink" => Some(Color::Fixed(153)),
                "mdItalic" | "syntaxComment" | "muted" => Some(Color::Fixed(224)),
                "syntaxString" => Some(self.del_syntax_string),
                "mdUrl" | "mdStrike" => Some(Color::Fixed(248)),
                "accent" | "mdDelim" => Some(Color::Fixed(51)),
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
