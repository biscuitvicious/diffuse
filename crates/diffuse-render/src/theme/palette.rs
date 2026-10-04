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
    pub note: Color,
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

    // Notes
    pub note_tint_bg: Color,

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
    pub file_header_path_selected: Color,
    pub file_header_path_current: Color,
    pub file_header_path_rest: Color,

    pub hunk_divider_bg_selected: Color,
    pub hunk_divider_bg_current: Color,
    pub hunk_divider_bg_rest: Color,
    pub hunk_divider_fg: Color,
    pub hunk_divider_fg_selected: Color,

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
    pub syntax_text: Color,

    // Markdown
    pub md_bold: Color,
    pub md_italic: Color,
    pub md_strike: Color,
    pub md_link: Color,
    pub md_url: Color,
    pub md_delim: Color,
    pub md_rule: Color,

    // UI & Status Bar
    pub status_bar_bg: Color,
    pub status_bar_fg: Color,
    pub status_bar_dim: Color,
    pub status_bar_accent: Color,
    pub ribbon_divider: Color,
    pub ribbon_text: Color,

    // Header Bar Elements
    pub header_repo: Color,
    pub header_branch: Color,
    pub header_dim: Color,
    pub header_staged: Color,
    pub header_prefix: Color,

    // Modals & Overlays
    pub modal_bg: Color,
    pub modal_border: Color,
    pub modal_title: Color,
    pub modal_text: Color,
    pub modal_dim: Color,
    pub modal_selected_bg: Color,
    pub modal_selected_fg: Color,

    // Count Highlights (theme-specific)
    pub count_dels: Color,
    pub count_adds: Color,
    pub count_mods: Color,
    pub count_movs: Color,
    pub count_notes: Color,

    // Syntax Overrides for Added lines
    pub add_syntax_keyword: Color,
    pub add_syntax_type: Color,
    pub add_syntax_function: Color,
    pub add_syntax_number: Color,
    pub add_syntax_comment: Color,
    pub add_syntax_operator: Color,
    pub add_syntax_text: Color,
    pub add_md_bold: Color,
    pub add_md_italic: Color,
    pub add_md_link: Color,
    pub add_md_url: Color,

    // Syntax Overrides for Deleted lines
    pub del_syntax_keyword: Color,
    pub del_syntax_type: Color,
    pub del_syntax_function: Color,
    pub del_syntax_number: Color,
    pub del_syntax_comment: Color,
    pub del_syntax_operator: Color,
    pub del_syntax_text: Color,
    pub del_md_bold: Color,
    pub del_md_italic: Color,
    pub del_md_link: Color,
    pub del_md_url: Color,
}

impl ThemePalette {
    /// Default Theme Palette with deeper background and muted red/green
    /// shades.
    #[must_use]
    pub const fn default_palette() -> Self {
        Self {
            bg_base: Color::Fixed(233),
            bg_modal: Color::Fixed(234),
            border: Color::Fixed(243),
            warning: Color::Fixed(220),
            note: Color::Fixed(220),
            accent: Color::Fixed(51),
            muted: Color::Fixed(243),

            ctx_tint_bg: Color::Fixed(233),
            ctx_cursor_bg: Color::Fixed(237),
            ctx_fg: Color::Primary,
            band_bg: Color::Fixed(235),

            add_tint_bg: Color::Rgb(28, 50, 30),
            add_cursor_bg: Color::Rgb(46, 74, 48),
            add_fill_bg: Color::Rgb(30, 96, 50),
            add_fill_cursor_bg: Color::Rgb(37, 120, 65),
            add_fg: Color::Fixed(120),

            del_tint_bg: Color::Rgb(38, 12, 14),
            del_cursor_bg: Color::Rgb(68, 24, 26),
            del_fill_bg: Color::Rgb(82, 28, 32),
            del_fill_cursor_bg: Color::Rgb(100, 34, 38),
            del_fg: Color::Fixed(203),

            mod_tint_bg: Color::Rgb(38, 38, 38),
            mod_cursor_bg: Color::Rgb(66, 66, 66),
            mod_fill_bg: Color::Rgb(30, 96, 50),
            mod_fill_cursor_bg: Color::Rgb(37, 120, 65),
            mod_fg: Color::Fixed(245),

            mov_tint_bg: Color::Fixed(233),
            mov_cursor_bg: Color::Fixed(237),
            mov_fg: Color::Fixed(245),

            note_tint_bg: Color::Rgb(76, 64, 20),

            ctx_gutter_in_hunk: Color::Fixed(245),
            ctx_gutter_out_hunk: Color::Fixed(238),
            del_gutter_num: Color::Fixed(203),
            del_gutter_icon: Color::Fixed(203),
            add_gutter_num: Color::Fixed(120),
            add_gutter_icon: Color::Fixed(120),
            mod_gutter_num: Color::Fixed(214),
            mod_gutter_icon: Color::Fixed(214),
            mov_gutter_num: Color::Fixed(147),
            mov_gutter_icon: Color::Fixed(117),

            file_header_bg_selected: Color::Fixed(238),
            file_header_bg_current: Color::Fixed(235),
            file_header_bg_rest: Color::Fixed(233),
            file_header_fg_selected: Color::Fixed(221),
            file_header_fg_current: Color::Fixed(220),
            file_header_fg_rest: Color::Fixed(214),
            file_header_path_selected: Color::Fixed(255),
            file_header_path_current: Color::Fixed(254),
            file_header_path_rest: Color::Fixed(251),

            hunk_divider_bg_selected: Color::Fixed(238),
            hunk_divider_bg_current: Color::Fixed(235),
            hunk_divider_bg_rest: Color::Primary,
            hunk_divider_fg: Color::Fixed(213),
            hunk_divider_fg_selected: Color::Rgb(255, 155, 255),

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

            // UI & Status Bar
            status_bar_bg: Color::Fixed(234),
            status_bar_fg: Color::Fixed(252),
            status_bar_dim: Color::Fixed(244),
            status_bar_accent: Color::Fixed(220),
            ribbon_divider: Color::Fixed(244),
            ribbon_text: Color::Fixed(244),

            // Header Bar Elements
            header_repo: Color::Fixed(117),
            header_branch: Color::Fixed(252),
            header_dim: Color::Fixed(244),
            header_staged: Color::Fixed(120),
            header_prefix: Color::Fixed(152),

            // Modals & Overlays
            modal_bg: Color::Fixed(236),
            modal_border: Color::Fixed(240),
            modal_title: Color::Fixed(231),
            modal_text: Color::Fixed(250),
            modal_dim: Color::Fixed(242),
            modal_selected_bg: Color::Fixed(239),
            modal_selected_fg: Color::Fixed(248),

            // Count Highlights (theme-specific)
            count_dels: Color::Fixed(203),
            count_adds: Color::Fixed(120),
            count_mods: Color::Fixed(214),
            count_movs: Color::Fixed(147),
            count_notes: Color::Fixed(220),

            // Syntax Overrides for Added lines
            add_syntax_keyword: Color::Fixed(147),
            add_syntax_type: Color::Fixed(220),
            add_syntax_function: Color::Fixed(123),
            add_syntax_number: Color::Fixed(221),
            add_syntax_comment: Color::Fixed(150),
            add_syntax_operator: Color::Fixed(117),
            add_md_bold: Color::Fixed(214),
            add_md_italic: Color::Fixed(229),
            add_md_link: Color::Fixed(117),
            add_md_url: Color::Fixed(250),

            // Syntax Overrides for Deleted lines
            del_syntax_keyword: Color::Fixed(183),
            del_syntax_type: Color::Fixed(215),
            del_syntax_function: Color::Fixed(159),
            del_syntax_number: Color::Fixed(222),
            del_syntax_comment: Color::Fixed(224),
            del_syntax_operator: Color::Fixed(153),
            del_syntax_text: Color::Primary,
            del_md_bold: Color::Fixed(222),
            del_md_italic: Color::Fixed(224),
            del_md_link: Color::Fixed(153),
            del_md_url: Color::Fixed(248),
            syntax_text: Color::Primary,
            add_syntax_text: Color::Primary,
        }
    }

    /// Light Theme Palette calibrated for light terminal backgrounds with
    /// high contrast (WCAG AA >= 4.5:1 for all text tokens).
    #[must_use]
    pub const fn light_palette() -> Self {
        Self {
            bg_base: Color::Rgb(250, 250, 250),
            bg_modal: Color::Rgb(255, 255, 255),
            border: Color::Rgb(185, 190, 198),
            warning: Color::Rgb(180, 80, 0),
            note: Color::Rgb(150, 95, 0),
            accent: Color::Rgb(9, 105, 218),
            muted: Color::Rgb(101, 109, 118),

            ctx_tint_bg: Color::Rgb(250, 250, 250),
            ctx_cursor_bg: Color::Rgb(232, 235, 240),
            ctx_fg: Color::Rgb(36, 41, 47),
            band_bg: Color::Rgb(243, 245, 248),

            add_tint_bg: Color::Rgb(230, 246, 236),
            add_cursor_bg: Color::Rgb(205, 238, 215),
            add_fill_bg: Color::Rgb(172, 224, 186),
            add_fill_cursor_bg: Color::Rgb(145, 210, 162),
            add_fg: Color::Rgb(26, 127, 55),

            del_tint_bg: Color::Rgb(255, 235, 236),
            del_cursor_bg: Color::Rgb(255, 212, 215),
            del_fill_bg: Color::Rgb(255, 178, 184),
            del_fill_cursor_bg: Color::Rgb(255, 150, 158),
            del_fg: Color::Rgb(207, 34, 46),

            mod_tint_bg: Color::Rgb(245, 245, 247),
            mod_cursor_bg: Color::Rgb(230, 232, 236),
            mod_fill_bg: Color::Rgb(172, 224, 186),
            mod_fill_cursor_bg: Color::Rgb(145, 210, 162),
            mod_fg: Color::Rgb(36, 41, 47),

            mov_tint_bg: Color::Rgb(244, 240, 255),
            mov_cursor_bg: Color::Rgb(230, 222, 252),
            mov_fg: Color::Rgb(90, 45, 160),

            note_tint_bg: Color::Rgb(254, 248, 225),

            ctx_gutter_in_hunk: Color::Rgb(101, 109, 118),
            ctx_gutter_out_hunk: Color::Rgb(175, 184, 193),
            del_gutter_num: Color::Rgb(207, 34, 46),
            del_gutter_icon: Color::Rgb(207, 34, 46),
            add_gutter_num: Color::Rgb(26, 127, 55),
            add_gutter_icon: Color::Rgb(26, 127, 55),
            mod_gutter_num: Color::Rgb(170, 90, 0),
            mod_gutter_icon: Color::Rgb(170, 90, 0),
            mov_gutter_num: Color::Rgb(110, 40, 130),
            mov_gutter_icon: Color::Rgb(110, 40, 130),

            file_header_bg_selected: Color::Rgb(215, 228, 250),
            file_header_bg_current: Color::Rgb(228, 236, 248),
            file_header_bg_rest: Color::Rgb(240, 243, 248),
            file_header_fg_selected: Color::Rgb(15, 60, 140),
            file_header_fg_current: Color::Rgb(25, 75, 160),
            file_header_fg_rest: Color::Rgb(60, 80, 110),
            file_header_path_selected: Color::Rgb(5, 25, 65),
            file_header_path_current: Color::Rgb(15, 35, 75),
            file_header_path_rest: Color::Rgb(35, 50, 80),

            hunk_divider_bg_selected: Color::Rgb(225, 225, 238),
            hunk_divider_bg_current: Color::Rgb(238, 238, 246),
            hunk_divider_bg_rest: Color::Primary,
            hunk_divider_fg: Color::Rgb(110, 40, 130),
            hunk_divider_fg_selected: Color::Rgb(140, 45, 165),

            header_bar_bg: Color::Rgb(235, 238, 242),
            header_bar_fg: Color::Rgb(36, 41, 47),

            scrollbar_thumb: Color::Rgb(180, 185, 195),
            scrollbar_track: Color::Rgb(240, 242, 245),

            syntax_keyword: Color::Rgb(170, 20, 120),
            syntax_type: Color::Rgb(15, 110, 130),
            syntax_function: Color::Rgb(10, 85, 185),
            syntax_number: Color::Rgb(165, 75, 0),
            syntax_string: Color::Rgb(20, 115, 40),
            add_syntax_string: Color::Rgb(20, 115, 40),
            del_syntax_string: Color::Rgb(180, 30, 40),
            syntax_comment: Color::Rgb(105, 115, 130),
            syntax_operator: Color::Rgb(65, 75, 90),

            md_bold: Color::Rgb(180, 75, 0),
            md_italic: Color::Rgb(125, 60, 150),
            md_strike: Color::Rgb(130, 135, 145),
            md_link: Color::Rgb(10, 85, 185),
            md_url: Color::Rgb(100, 105, 115),
            md_delim: Color::Rgb(15, 110, 130),
            md_rule: Color::Rgb(185, 190, 200),

            // UI & Status Bar
            status_bar_bg: Color::Rgb(235, 238, 242),
            status_bar_fg: Color::Rgb(36, 41, 47),
            status_bar_dim: Color::Rgb(105, 115, 130),
            status_bar_accent: Color::Rgb(170, 90, 0),
            ribbon_divider: Color::Rgb(185, 190, 200),
            ribbon_text: Color::Rgb(105, 115, 130),

            // Header Bar Elements
            header_repo: Color::Rgb(10, 85, 185),
            header_branch: Color::Rgb(36, 41, 47),
            header_dim: Color::Rgb(105, 115, 130),
            header_staged: Color::Rgb(26, 127, 55),
            header_prefix: Color::Rgb(15, 110, 130),

            // Modals & Overlays
            modal_bg: Color::Rgb(255, 255, 255),
            modal_border: Color::Rgb(185, 190, 200),
            modal_title: Color::Rgb(36, 41, 47),
            modal_text: Color::Rgb(36, 41, 47),
            modal_dim: Color::Rgb(105, 115, 130),
            modal_selected_bg: Color::Rgb(220, 232, 252),
            modal_selected_fg: Color::Rgb(10, 85, 185),

            // Count Highlights (theme-specific)
            count_dels: Color::Rgb(207, 34, 46),
            count_adds: Color::Rgb(26, 127, 55),
            count_mods: Color::Rgb(170, 90, 0),
            count_movs: Color::Rgb(110, 40, 130),
            count_notes: Color::Rgb(150, 95, 0),

            // Syntax Overrides for Added lines
            add_syntax_keyword: Color::Rgb(170, 20, 120),
            add_syntax_type: Color::Rgb(15, 110, 130),
            add_syntax_function: Color::Rgb(10, 85, 185),
            add_syntax_number: Color::Rgb(165, 75, 0),
            add_syntax_comment: Color::Rgb(105, 115, 130),
            add_syntax_operator: Color::Rgb(65, 75, 90),
            add_md_bold: Color::Rgb(180, 75, 0),
            add_md_italic: Color::Rgb(125, 60, 150),
            add_md_link: Color::Rgb(10, 85, 185),
            add_md_url: Color::Rgb(100, 105, 115),

            // Syntax Overrides for Deleted lines
            del_syntax_keyword: Color::Rgb(170, 20, 120),
            del_syntax_type: Color::Rgb(15, 110, 130),
            del_syntax_function: Color::Rgb(10, 85, 185),
            del_syntax_number: Color::Rgb(165, 75, 0),
            del_syntax_comment: Color::Rgb(105, 115, 130),
            del_syntax_operator: Color::Rgb(65, 75, 90),
            del_syntax_text: Color::Rgb(207, 34, 46),
            del_md_bold: Color::Rgb(180, 75, 0),
            del_md_italic: Color::Rgb(125, 60, 150),
            del_md_link: Color::Rgb(10, 85, 185),
            del_md_url: Color::Rgb(100, 105, 115),
            syntax_text: Color::Rgb(36, 41, 47),
            add_syntax_text: Color::Rgb(36, 41, 47),
        }
    }

    /// Resolve semantic named color token.
    #[must_use]
    pub fn named_color(&self, name: &str) -> Option<Color> {
        match name {
            "text" => (self.syntax_text != Color::Primary).then_some(self.syntax_text),
            "toolDiffAdded" => Some(self.add_fg),
            "toolDiffRemoved" => Some(self.del_fg),
            "toolDiffContext" | "muted" => Some(self.muted),
            "warning" => Some(self.warning),
            "note" => Some(self.note),
            "note_tint_bg" => Some(self.note_tint_bg),
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
    pub fn hunk_divider_colors(&self, is_selected: bool, is_current: bool) -> (Color, Color) {
        if is_selected {
            (self.hunk_divider_bg_selected, self.hunk_divider_fg_selected)
        } else if is_current {
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
                "mdBold" => Some(self.add_md_bold),
                "syntaxType" => Some(self.add_syntax_type),
                "syntaxNumber" => Some(self.add_syntax_number),
                "syntaxKeyword" => Some(self.add_syntax_keyword),
                "syntaxFunction" => Some(self.add_syntax_function),
                "syntaxOperator" => Some(self.add_syntax_operator),
                "syntaxComment" => Some(self.add_syntax_comment),
                "mdItalic" => Some(self.add_md_italic),
                "syntaxString" => Some(self.add_syntax_string),
                "mdLink" => Some(self.add_md_link),
                "mdUrl" => Some(self.add_md_url),
                "mdStrike" => None,
                "accent" | "mdDelim" => Some(self.accent),
                "text" => (self.add_syntax_text != Color::Primary).then_some(self.add_syntax_text),
                _ => self.named_color(color),
            },
            RowKind::Del => match color {
                "mdBold" => Some(self.del_md_bold),
                "syntaxNumber" => Some(self.del_syntax_number),
                "syntaxType" => Some(self.del_syntax_type),
                "syntaxKeyword" => Some(self.del_syntax_keyword),
                "syntaxFunction" => Some(self.del_syntax_function),
                "syntaxOperator" => Some(self.del_syntax_operator),
                "syntaxComment" => Some(self.del_syntax_comment),
                "mdItalic" => Some(self.del_md_italic),
                "syntaxString" => Some(self.del_syntax_string),
                "mdLink" => Some(self.del_md_link),
                "mdUrl" => Some(self.del_md_url),
                "mdStrike" => None,
                "accent" | "mdDelim" => Some(self.accent),
                "text" => (self.del_syntax_text != Color::Primary).then_some(self.del_syntax_text),
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
                "mdStrike" => None,
                "mdUrl" | "syntaxComment" => Some(self.syntax_comment),
                "mdRule" => Some(self.md_rule),
                "accent" | "mdDelim" => Some(self.accent),
                "text" => (self.syntax_text != Color::Primary).then_some(self.syntax_text),
                _ => self.named_color(color),
            },
        }
    }
}
