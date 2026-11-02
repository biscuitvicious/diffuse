//! Theme palette definitions and color mapping specifications.

pub mod dark;
pub mod light;
pub mod scoped;

pub use dark::DEFAULT_BORDER_COLOR_DARK;
use diffuse_core::parse::RowKind;
use diffuse_syntax::color::TokenColor;
pub use light::DEFAULT_BORDER_COLOR_LIGHT;
pub use scoped::{ScopePalette, UiPalette};
use yansi::Color;

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
    pub literal: Color,
    pub string: Color,
    pub add_string: Color,
    pub del_string: Color,
    pub char_literal: Color,
    pub add_char_literal: Color,
    pub del_char_literal: Color,
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
    pub add_literal: Color,
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
    pub del_literal: Color,
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
        dark::default_palette()
    }

    /// Light Theme Palette calibrated for light terminal backgrounds with
    /// high contrast (WCAG AA >= 4.5:1 for all text tokens).
    #[must_use]
    pub const fn light_palette() -> Self {
        light::light_palette()
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
                TokenColor::Literal => Some(self.add_literal),
                TokenColor::Keyword => Some(self.add_keyword),
                TokenColor::Function => Some(self.add_function),
                TokenColor::Operator => Some(self.add_operator),
                TokenColor::Comment => Some(self.add_comment),
                TokenColor::Italic => Some(self.add_italic),
                TokenColor::String => Some(self.add_string),
                TokenColor::CharLiteral => Some(self.add_char_literal),
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
                TokenColor::Literal => Some(self.del_literal),
                TokenColor::Type => Some(self.del_typ),
                TokenColor::Keyword => Some(self.del_keyword),
                TokenColor::Function => Some(self.del_function),
                TokenColor::Operator => Some(self.del_operator),
                TokenColor::Comment => Some(self.del_comment),
                TokenColor::Italic => Some(self.del_italic),
                TokenColor::String => Some(self.del_string),
                TokenColor::CharLiteral => Some(self.del_char_literal),
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
                TokenColor::Literal => Some(self.literal),
                TokenColor::Operator => Some(self.operator),
                TokenColor::Italic => Some(self.italic),
                TokenColor::String => Some(self.string),
                TokenColor::CharLiteral => Some(self.char_literal),
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
