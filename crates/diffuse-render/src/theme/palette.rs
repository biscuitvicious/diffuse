//! Theme palette definitions and color mapping specifications.

use diffuse_core::parse::RowKind;
use diffuse_syntax::color::TokenColor;
use yansi::Color;

/// Default border color shared by every `*bo`/`*_border` palette key on dark
/// backgrounds.
pub const DEFAULT_BORDER_COLOR_DARK: Color = Color::Fixed(243);

/// Default border color shared by every `*bo`/`*_border` palette key on light
/// backgrounds. Kept at text contrast (not the paler decorative border) so
/// border runs stay legible on light tints.
pub const DEFAULT_BORDER_COLOR_LIGHT: Color = Color::Rgb(101, 109, 118);

/// Complete color palette for syntax highlighting, diff rows, and UI
/// components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_base: Color,
    pub bg_modal: Color,
    pub warning: Color,
    pub note: Color,
    pub accent: Color,

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
    pub keyword: Color,
    pub typ: Color,
    pub function: Color,
    pub number: Color,
    pub string: Color,
    pub add_string: Color,
    pub del_string: Color,
    pub comment: Color,
    pub operator: Color,
    pub text: Color,

    // Markdown
    pub bold: Color,
    pub italic: Color,
    pub strike: Color,
    pub link: Color,
    pub url: Color,
    pub delim: Color,
    pub rule: Color,
    pub code: Color,
    pub table_border: Color,
    pub table_header: Color,
    pub link_border: Color,
    pub quote_border: Color,
    pub fence_border: Color,

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
    pub add_keyword: Color,
    pub add_typ: Color,
    pub add_function: Color,
    pub add_number: Color,
    pub add_comment: Color,
    pub add_operator: Color,
    pub add_text: Color,
    pub add_bold: Color,
    pub add_italic: Color,
    pub add_link: Color,
    pub add_url: Color,
    pub add_code: Color,
    pub add_table_border: Color,
    pub add_table_header: Color,
    pub add_link_border: Color,
    pub add_quote_border: Color,
    pub add_fence_border: Color,
    pub add_delim: Color,

    // Syntax Overrides for Deleted lines
    pub del_keyword: Color,
    pub del_typ: Color,
    pub del_function: Color,
    pub del_number: Color,
    pub del_comment: Color,
    pub del_operator: Color,
    pub del_text: Color,
    pub del_bold: Color,
    pub del_italic: Color,
    pub del_link: Color,
    pub del_url: Color,
    pub del_code: Color,
    pub del_table_border: Color,
    pub del_table_header: Color,
    pub del_link_border: Color,
    pub del_quote_border: Color,
    pub del_fence_border: Color,
    pub del_delim: Color,
}

impl ThemePalette {
    /// Default Theme Palette with deeper background and muted red/green
    /// shades.
    #[must_use]
    pub const fn default_palette() -> Self {
        Self {
            bg_base: Color::Fixed(233),
            bg_modal: Color::Fixed(234),
            warning: Color::Fixed(220),
            note: Color::Fixed(220),
            accent: Color::Fixed(51),

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

            keyword: Color::Fixed(147),
            typ: Color::Fixed(220),
            function: Color::Fixed(123),
            number: Color::Fixed(221),
            string: Color::Fixed(120),
            add_string: Color::Fixed(120),
            del_string: Color::Fixed(203),
            comment: Color::Fixed(150),
            operator: Color::Fixed(117),

            bold: Color::Fixed(214),
            italic: Color::Fixed(229),
            strike: Color::Fixed(250),
            link: Color::Fixed(117),
            url: Color::Fixed(250),
            delim: Color::Fixed(51),
            rule: Color::Fixed(220),
            code: Color::Fixed(81),
            table_border: DEFAULT_BORDER_COLOR_DARK,
            table_header: Color::Fixed(229),
            link_border: DEFAULT_BORDER_COLOR_DARK,
            quote_border: DEFAULT_BORDER_COLOR_DARK,
            fence_border: DEFAULT_BORDER_COLOR_DARK,

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
            add_keyword: Color::Fixed(147),
            add_typ: Color::Fixed(220),
            add_function: Color::Fixed(123),
            add_number: Color::Fixed(221),
            add_comment: Color::Fixed(150),
            add_operator: Color::Fixed(117),
            add_bold: Color::Fixed(214),
            add_italic: Color::Fixed(229),
            add_link: Color::Fixed(117),
            add_url: Color::Fixed(250),
            add_code: Color::Fixed(81),
            add_table_border: DEFAULT_BORDER_COLOR_DARK,
            add_table_header: Color::Fixed(229),
            add_link_border: DEFAULT_BORDER_COLOR_DARK,
            add_quote_border: DEFAULT_BORDER_COLOR_DARK,
            add_fence_border: DEFAULT_BORDER_COLOR_DARK,
            add_delim: Color::Fixed(51),

            // Syntax Overrides for Deleted lines
            del_keyword: Color::Fixed(183),
            del_typ: Color::Fixed(215),
            del_function: Color::Fixed(159),
            del_number: Color::Fixed(222),
            del_comment: Color::Fixed(224),
            del_operator: Color::Fixed(153),
            del_text: Color::Primary,
            del_bold: Color::Fixed(222),
            del_italic: Color::Fixed(224),
            del_link: Color::Fixed(153),
            del_url: Color::Fixed(248),
            del_code: Color::Fixed(203),
            del_table_border: DEFAULT_BORDER_COLOR_DARK,
            del_table_header: Color::Fixed(224),
            del_link_border: DEFAULT_BORDER_COLOR_DARK,
            del_quote_border: DEFAULT_BORDER_COLOR_DARK,
            del_fence_border: DEFAULT_BORDER_COLOR_DARK,
            del_delim: Color::Fixed(51),
            text: Color::Primary,
            add_text: Color::Primary,
        }
    }

    /// Light Theme Palette calibrated for light terminal backgrounds with
    /// high contrast (WCAG AA >= 4.5:1 for all text tokens).
    #[must_use]
    pub const fn light_palette() -> Self {
        Self {
            bg_base: Color::Rgb(250, 250, 250),
            bg_modal: Color::Rgb(255, 255, 255),
            warning: Color::Rgb(180, 80, 0),
            note: Color::Rgb(150, 95, 0),
            accent: Color::Rgb(9, 105, 218),

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

            keyword: Color::Rgb(170, 20, 120),
            typ: Color::Rgb(15, 110, 130),
            function: Color::Rgb(10, 85, 185),
            number: Color::Rgb(165, 75, 0),
            string: Color::Rgb(20, 115, 40),
            add_string: Color::Rgb(20, 115, 40),
            del_string: Color::Rgb(180, 30, 40),
            comment: Color::Rgb(105, 115, 130),
            operator: Color::Rgb(65, 75, 90),

            bold: Color::Rgb(180, 75, 0),
            italic: Color::Rgb(125, 60, 150),
            strike: Color::Rgb(130, 135, 145),
            link: Color::Rgb(10, 85, 185),
            url: Color::Rgb(100, 105, 115),
            delim: Color::Rgb(15, 110, 130),
            rule: Color::Rgb(185, 190, 200),
            code: Color::Rgb(12, 100, 125),
            table_border: DEFAULT_BORDER_COLOR_LIGHT,
            table_header: Color::Rgb(125, 60, 150),
            link_border: DEFAULT_BORDER_COLOR_LIGHT,
            quote_border: DEFAULT_BORDER_COLOR_LIGHT,
            fence_border: DEFAULT_BORDER_COLOR_LIGHT,

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
            add_keyword: Color::Rgb(170, 20, 120),
            add_typ: Color::Rgb(15, 110, 130),
            add_function: Color::Rgb(10, 85, 185),
            add_number: Color::Rgb(165, 75, 0),
            add_comment: Color::Rgb(105, 115, 130),
            add_operator: Color::Rgb(65, 75, 90),
            add_bold: Color::Rgb(180, 75, 0),
            add_italic: Color::Rgb(125, 60, 150),
            add_link: Color::Rgb(10, 85, 185),
            add_url: Color::Rgb(100, 105, 115),
            add_code: Color::Rgb(12, 100, 125),
            add_table_border: DEFAULT_BORDER_COLOR_LIGHT,
            add_table_header: Color::Rgb(125, 60, 150),
            add_link_border: DEFAULT_BORDER_COLOR_LIGHT,
            add_quote_border: DEFAULT_BORDER_COLOR_LIGHT,
            add_fence_border: DEFAULT_BORDER_COLOR_LIGHT,
            add_delim: Color::Rgb(15, 110, 130),

            // Syntax Overrides for Deleted lines
            del_keyword: Color::Rgb(170, 20, 120),
            del_typ: Color::Rgb(15, 110, 130),
            del_function: Color::Rgb(10, 85, 185),
            del_number: Color::Rgb(165, 75, 0),
            del_comment: Color::Rgb(105, 115, 130),
            del_operator: Color::Rgb(65, 75, 90),
            del_text: Color::Rgb(207, 34, 46),
            del_bold: Color::Rgb(180, 75, 0),
            del_italic: Color::Rgb(125, 60, 150),
            del_link: Color::Rgb(10, 85, 185),
            del_url: Color::Rgb(100, 105, 115),
            del_code: Color::Rgb(180, 30, 40),
            del_table_border: DEFAULT_BORDER_COLOR_LIGHT,
            del_table_header: Color::Rgb(125, 60, 150),
            del_link_border: DEFAULT_BORDER_COLOR_LIGHT,
            del_quote_border: DEFAULT_BORDER_COLOR_LIGHT,
            del_fence_border: DEFAULT_BORDER_COLOR_LIGHT,
            del_delim: Color::Rgb(15, 110, 130),
            text: Color::Rgb(36, 41, 47),
            add_text: Color::Rgb(36, 41, 47),
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
    pub fn token_color(&self, color: TokenColor, row_kind: RowKind) -> Option<Color> {
        match row_kind {
            RowKind::Add => match color {
                TokenColor::Bold => Some(self.add_bold),
                TokenColor::Type => Some(self.add_typ),
                TokenColor::Number => Some(self.add_number),
                TokenColor::Keyword => Some(self.add_keyword),
                TokenColor::Function => Some(self.add_function),
                TokenColor::Operator => Some(self.add_operator),
                TokenColor::Comment => Some(self.add_comment),
                TokenColor::Italic => Some(self.add_italic),
                TokenColor::String => Some(self.add_string),
                TokenColor::Code => Some(self.add_code),
                TokenColor::TableBorder => Some(self.add_table_border),
                TokenColor::TableHeader => Some(self.add_table_header),
                TokenColor::LinkBorder => Some(self.add_link_border),
                TokenColor::QuoteBorder => Some(self.add_quote_border),
                TokenColor::FenceBorder => Some(self.add_fence_border),
                TokenColor::Link => Some(self.add_link),
                TokenColor::Url => Some(self.add_url),
                TokenColor::Strike => None,
                TokenColor::Delimiter => Some(self.add_delim),
                TokenColor::Rule => Some(self.rule),
                TokenColor::Default => (self.add_text != Color::Primary).then_some(self.add_text),
            },
            RowKind::Del => match color {
                TokenColor::Bold => Some(self.del_bold),
                TokenColor::Number => Some(self.del_number),
                TokenColor::Type => Some(self.del_typ),
                TokenColor::Keyword => Some(self.del_keyword),
                TokenColor::Function => Some(self.del_function),
                TokenColor::Operator => Some(self.del_operator),
                TokenColor::Comment => Some(self.del_comment),
                TokenColor::Italic => Some(self.del_italic),
                TokenColor::String => Some(self.del_string),
                TokenColor::Code => Some(self.del_code),
                TokenColor::TableBorder => Some(self.del_table_border),
                TokenColor::TableHeader => Some(self.del_table_header),
                TokenColor::LinkBorder => Some(self.del_link_border),
                TokenColor::QuoteBorder => Some(self.del_quote_border),
                TokenColor::FenceBorder => Some(self.del_fence_border),
                TokenColor::Link => Some(self.del_link),
                TokenColor::Url => Some(self.del_url),
                TokenColor::Strike => None,
                TokenColor::Delimiter => Some(self.del_delim),
                TokenColor::Rule => Some(self.rule),
                TokenColor::Default => (self.del_text != Color::Primary).then_some(self.del_text),
            },
            _ => match color {
                TokenColor::Bold => Some(self.bold),
                TokenColor::Type => Some(self.typ),
                TokenColor::Keyword => Some(self.keyword),
                TokenColor::Function => Some(self.function),
                TokenColor::Number => Some(self.number),
                TokenColor::Operator => Some(self.operator),
                TokenColor::Italic => Some(self.italic),
                TokenColor::String => Some(self.string),
                TokenColor::Code => Some(self.code),
                TokenColor::TableBorder => Some(self.table_border),
                TokenColor::TableHeader => Some(self.table_header),
                TokenColor::LinkBorder => Some(self.link_border),
                TokenColor::QuoteBorder => Some(self.quote_border),
                TokenColor::FenceBorder => Some(self.fence_border),
                TokenColor::Link => Some(self.link),
                TokenColor::Strike => None,
                TokenColor::Url | TokenColor::Comment => Some(self.comment),
                TokenColor::Rule => Some(self.rule),
                TokenColor::Delimiter => Some(self.delim),
                TokenColor::Default => (self.text != Color::Primary).then_some(self.text),
            },
        }
    }
}
